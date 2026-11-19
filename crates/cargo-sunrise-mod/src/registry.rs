//! `cargo sunrise-mod registry build|review|check|list`: the registry repository's CI
//! (`sunrise-org/sunrise-mods`, docs/modding/src/reference/registry-index.md). Network-free: the
//! workflow downloads the files and publishes what this writes.
use std::path::Path;

#[cfg(test)]
mod tests;
use sunrise_mod_package::index::{
    registry::{self, Sources},
    resolve::uninstallable_versions,
};

/// Where this registry publishes its release assets and its site (icons and screenshots).
const RELEASE: &str = "https://github.com/sunrise-org/sunrise-mods/releases/download/";
const SITE: &str = "https://sunrise-org.github.io/sunrise-mods/";

const USAGE: &str = "usage: cargo sunrise-mod registry build <checkout> --files <dir> --out <index.json> [--release <url>] [--site <url>] [--generated <time>] [--counts <counts.json>]
       cargo sunrise-mod registry review <base checkout> <head checkout> --author <login> [--files <dir>] [--json]
       cargo sunrise-mod registry check <base checkout> <head checkout> --files <dir> [--json]
       cargo sunrise-mod registry list <checkout>
       cargo sunrise-mod registry release <checkout> <id> <file> --tag <tag> --published <YYYY-MM-DD> [--notes <file>] [--signature <file>] [--game <version>]
       cargo sunrise-mod registry page <checkout> <id> <sunrise-registry.toml>";

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    super::flag(args, name)
}

fn positional(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if a == "--json" {
        } else if a.starts_with("--") {
            skip = true;
        } else {
            out.push(a.as_str());
        }
    }
    out
}

fn lines(errors: Vec<String>) -> String {
    format!("{} problem(s):\n  {}", errors.len(), errors.join("\n  "))
}

pub fn main(args: &[String]) -> Result<(), String> {
    let rest = args.get(1..).unwrap_or_default();
    let pos = positional(rest);
    match args.first().map(String::as_str) {
        Some("build") => {
            let [checkout] = pos[..] else {
                return Err(USAGE.into());
            };
            let (files, out) = (flag(rest, "--files"), flag(rest, "--out"));
            let (Some(files), Some(out)) = (files, out) else {
                return Err(USAGE.into());
            };
            let generated = flag(rest, "--generated")
                .map(str::to_owned)
                .or_else(|| std::env::var("REGISTRY_GENERATED").ok())
                .unwrap_or_default();
            let mut index = registry::build(&Sources {
                registry: Path::new(checkout),
                files: Path::new(files),
                release: flag(rest, "--release").unwrap_or(RELEASE),
                site: flag(rest, "--site").unwrap_or(SITE),
                generated,
            })
            .map_err(lines)?;
            for problem in uninstallable_versions(&index, |_| true) {
                eprintln!("warning: {problem}");
            }
            // GitHub's download and star counts, which the workflow fetched.
            if let Some(path) = flag(rest, "--counts") {
                let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
                let counts = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
                registry::count(&mut index, &counts);
            }
            let text = serde_json::to_string_pretty(&index).map_err(|e| e.to_string())?;
            std::fs::write(out, text + "\n").map_err(|e| format!("{out}: {e}"))?;
            println!(
                "Wrote {out}: {} project(s), {} taken down",
                index.projects.len(),
                index.revoked.len()
            );
            Ok(())
        }
        Some("review") => {
            let [base, head] = pos[..] else {
                return Err(USAGE.into());
            };
            let author = flag(rest, "--author").ok_or(USAGE)?;
            let base = registry::load(Path::new(base)).map_err(lines)?;
            let checkout = Path::new(head);
            let head = registry::load(checkout).map_err(lines)?;
            let mut review =
                registry::review_with_revoked(&base.entries, &head.entries, &head.revoked, author);
            let files = flag(rest, "--files").map(Path::new);
            let sources = Sources {
                registry: checkout,
                files: files.unwrap_or(checkout),
                release: RELEASE,
                site: SITE,
                generated: String::new(),
            };
            if files.is_some() {
                match registry::candidate(&head, &sources) {
                    Ok(index) => {
                        review.notes.extend(registry::takedown_notes(
                            &base.entries,
                            &base.revoked,
                            &index,
                        ));
                        if let Err(error) = index.check() {
                            review.errors.push(error);
                        }
                    }
                    Err(errors) => review.errors.extend(errors),
                }
            } else {
                review.errors.extend(head.check_collections(&sources));
                if registry::new_takedowns(&base.revoked, &head.revoked)
                    .next()
                    .is_some()
                {
                    review.errors.push("to review a takedown, give --files <dir> with the head checkout's packages named by SHA-256".into());
                }
            }
            let collections = registry::review_collections(&base, &head, author);
            if rest.iter().any(|a| a == "--json") {
                let json = serde_json::json!({"errors": review.errors, "notes": review.notes, "collection_reviews": collections});
                println!("{json}");
            } else {
                for change in collections {
                    let status = match change.status {
                        registry::CollectionReviewStatus::Routine => "routine",
                        registry::CollectionReviewStatus::ReviewRequired => "review required",
                    };
                    println!(
                        "note: collection {}: {status}: {}",
                        change.id, change.reason
                    );
                }
                for n in &review.notes {
                    println!("note: {n}");
                }
                for e in &review.errors {
                    println!("error: {e}");
                }
            }
            if review.errors.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "{} change(s) the registry refuses",
                    review.errors.len()
                ))
            }
        }
        // `check` (and its trial run) on each file `head` lists and `base`
        // doesn't: a pull request's new files.
        Some("check") => {
            let [base, head] = pos[..] else {
                return Err(USAGE.into());
            };
            let files = Path::new(flag(rest, "--files").ok_or(USAGE)?);
            let old: std::collections::HashSet<String> = match registry::load(Path::new(base)) {
                Ok(base) => base
                    .entries
                    .iter()
                    .flat_map(|e| e.versions.iter().map(|v| v.sha256.clone()))
                    .collect(),
                // The first pull request has no entries to compare with.
                Err(_) => Default::default(),
            };
            let checkout = Path::new(head);
            let head = registry::load(checkout).map_err(lines)?;
            let json = rest.iter().any(|a| a == "--json");
            let assembled = registry::candidate(
                &head,
                &Sources {
                    registry: checkout,
                    files,
                    release: RELEASE,
                    site: SITE,
                    generated: String::new(),
                },
            );
            let mut reports = Vec::new();
            let errors = match assembled {
                Err(errors) => errors,
                Ok(index) => {
                    // Cache versions are already published. Check only the files added to the
                    // entries by this submission, not unchanged listings harmed by a takedown.
                    let submitted: std::collections::HashSet<_> = head
                        .entries
                        .iter()
                        .flat_map(|e| &e.versions)
                        .filter(|v| !old.contains(&v.sha256))
                        .map(|v| v.sha256.as_str())
                        .collect();
                    let mut errors = match index.check() {
                        Ok(()) => uninstallable_versions(&index, |v| {
                            submitted.contains(v.file.sha256.as_str())
                        }),
                        Err(error) => vec![error],
                    };
                    for e in &head.entries {
                        for v in e
                            .versions
                            .iter()
                            .filter(|v| submitted.contains(v.sha256.as_str()))
                        {
                            match crate::check::check(
                                &files.join(&v.sha256),
                                e.kind.clone(),
                                e.key.as_deref(),
                                v.signature.as_deref(),
                            ) {
                                Ok(report) if json => reports.push(serde_json::json!({
                                    "project": e.id, "version": v.version, "file": v.file,
                                    "report": report.json(),
                                })),
                                Ok(report) => println!(
                                    "{} {} ({}):\n{}",
                                    e.id,
                                    v.version,
                                    v.file,
                                    report.text().trim_end()
                                ),
                                Err(error) => {
                                    errors.push(format!("{} {}: {error}", e.id, v.version))
                                }
                            }
                        }
                    }
                    errors
                }
            };
            if json {
                println!(
                    "{}",
                    serde_json::json!({"files": reports, "errors": errors})
                );
            }
            if errors.is_empty() {
                Ok(())
            } else {
                Err(lines(errors))
            }
        }
        // The files CI must download for `build`: `<sha256> <id> <version> <file>`.
        Some("list") => {
            let [checkout] = pos[..] else {
                return Err(USAGE.into());
            };
            let loaded = registry::load(Path::new(checkout)).map_err(lines)?;
            for e in &loaded.entries {
                for v in &e.versions {
                    println!("{} {} {} {}", v.sha256, e.id, v.version, v.file);
                }
            }
            Ok(())
        }
        // A release of a project's repository, downloaded: the checks of a submission, then the
        // version as the index lists it (JSON on stdout), for the registry's release sync.
        Some("release") => {
            let [checkout, id, file] = pos[..] else {
                return Err(USAGE.into());
            };
            let (tag, published) = (
                flag(rest, "--tag").ok_or(USAGE)?,
                flag(rest, "--published").ok_or(USAGE)?,
            );
            let read = |name: &str| {
                flag(rest, name)
                    .map(|p| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}")))
                    .transpose()
            };
            let (notes, signature) = (read("--notes")?, read("--signature")?);
            let entry = entry_of(checkout, id)?;
            let path = Path::new(file);
            let cache = registry::Cache::read(Path::new(checkout), id)?;
            let entry = entry.resolved(&cache.page);
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            crate::check::check(
                path,
                entry.kind.clone(),
                entry.key.as_deref(),
                signature.as_deref(),
            )?;
            let version = registry::release_version(
                &entry,
                &registry::Release {
                    tag,
                    published,
                    notes: notes.as_deref().unwrap_or(""),
                    asset: flag(rest, "--name").unwrap_or(name),
                    game: flag(rest, "--game").unwrap_or("26.3"),
                    signature: signature.as_deref(),
                },
                path,
            )?;
            println!(
                "{}",
                serde_json::to_string(&version).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        // The page file of a project's repository, checked against its entry: the page as JSON.
        Some("page") => {
            let [checkout, id, file] = pos[..] else {
                return Err(USAGE.into());
            };
            let entry = entry_of(checkout, id)?;
            let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
            let page = registry::page(&entry, &text)?;
            println!(
                "{}",
                serde_json::to_string(&page).map_err(|e| e.to_string())?
            );
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

/// `mods/<id>.toml` of the checkout, which must name a repository.
fn entry_of(checkout: &str, id: &str) -> Result<registry::Entry, String> {
    registry::load(Path::new(checkout))
        .map_err(lines)?
        .entries
        .into_iter()
        .find(|e| e.id == id && e.repo.is_some())
        .ok_or_else(|| format!("{id}: no entry with a repo"))
}

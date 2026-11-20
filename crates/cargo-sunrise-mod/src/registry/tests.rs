use std::io::{Cursor, Write};
use sunrise_mod_package::index::{Index, resolve::RequestResult, sha256};

struct Registry(tempfile::TempDir);

impl Registry {
    fn new() -> Self {
        let r = Self(tempfile::tempdir().unwrap());
        for dir in ["head/mods", "head/icons", "files"] {
            std::fs::create_dir_all(r.0.path().join(dir)).unwrap();
        }
        r
    }

    fn add(&self, id: &str, version: &str, game: &str, dependencies: &str) -> String {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(
            sunrise_mod_package::manifest::MANIFEST,
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        write!(
            zip,
            "id='{id}'\nname='{id}'\nversion='{version}'\napi='0.2'\ncapabilities=['registries']\n[dependencies]\n{dependencies}"
        )
        .unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let hash = sha256(&bytes);
        std::fs::write(self.0.path().join("files").join(&hash), bytes).unwrap();
        let path = self.0.path().join(format!("head/mods/{id}.toml"));
        let mut entry = if path.exists() {
            std::fs::read_to_string(&path).unwrap()
        } else {
            let icon = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x80\0\0\0\x80\x08\x06\0\0\0";
            std::fs::write(self.0.path().join(format!("head/icons/{id}.png")), icon).unwrap();
            format!(
                "id='{id}'\nkind='mod'\nname='{id}'\nsummary='A content mod for registry validation.'\n\
                 description='An example package for testing version selection and registry policy.'\n\
                 owners=['author']\nlicense='MIT'\nsource='https://example.test/{id}'\n\
                 icon='icons/{id}.png'\ncategories=['utility']\n"
            )
        };
        entry.push_str(&format!(
            "\n[[versions]]\nversion='{version}'\npublished='2026-11-18'\ngame='{game}'\nfile='{id}-{version}.smod'\nsha256='{hash}'\n"
        ));
        std::fs::write(path, entry).unwrap();
        hash
    }

    fn run(&self, command: &str, paths: &[&str], flags: &[&str]) -> Result<(), String> {
        let path = |p| self.0.path().join(p).to_str().unwrap().to_owned();
        let mut args = vec![command.to_owned()];
        args.extend(paths.iter().copied().map(path));
        args.extend(["--files".to_owned(), path("files")]);
        args.extend(flags.iter().map(|s| (*s).to_owned()));
        super::main(&args)
    }

    fn check(&self) -> Result<(), String> {
        self.run("check", &["base", "head"], &["--json"])
    }
}

#[test]
fn new_versions_must_resolve_disjoint_transitive_ranges_together() {
    let r = Registry::new();
    r.add("library", "1.0.0", "26.3", "");
    r.add("library", "2.0.0", "26.3", "");
    r.add("first", "1.0.0", "26.3", "library='^1'");
    r.add("second", "1.0.0", "26.3", "library='^2'");
    r.add("root", "1.0.0", "26.3", "first='^1'\nsecond='^1'");
    let err = r.check().unwrap_err();
    for name in ["root", "library", "first", "second", "^1", "^2"] {
        assert!(err.contains(name), "{err}");
    }
}

#[test]
fn new_versions_need_their_dependencies_on_the_same_minecraft_release() {
    let r = Registry::new();
    r.add("library", "1.0.0", "26.4", "");
    r.add("root", "1.0.0", "26.3", "library='^1'");
    let err = r.check().unwrap_err();
    for name in ["root", "library", "^1", "Minecraft 26.3"] {
        assert!(err.contains(name), "{err}");
    }
}

#[test]
fn old_pairs_do_not_fail_publication_if_newer_releases_avoid_the_cycle() {
    let r = Registry::new();
    r.add("old_a", "1.0.0", "26.3", "old_b='^1'");
    r.add("old_a", "1.1.0", "26.3", "");
    r.add("old_b", "1.0.0", "26.3", "old_a='^1'");
    r.add("old_b", "1.1.0", "26.3", "");
    assert_eq!(r.check(), Ok(()));
}

#[test]
fn taking_down_the_last_matching_file_still_passes_review_check_and_build() {
    let r = Registry::new();
    r.add("library", "2.0.0", "26.3", "");
    let library = r.0.path().join("head/mods/library.toml");
    let kept = std::fs::read_to_string(&library).unwrap();
    // The old file is the only one that matches `root`; another release keeps the project listed.
    let taken = r.add("library", "1.0.0", "26.3", "");
    r.add("root", "1.0.0", "26.3", "library='^1'");
    assert_eq!(r.check(), Ok(()));
    let base = r.0.path().join("base/mods");
    std::fs::create_dir_all(&base).unwrap();
    for id in ["library", "root"] {
        let entry = format!("{id}.toml");
        std::fs::copy(r.0.path().join("head/mods").join(&entry), base.join(&entry)).unwrap();
    }
    std::fs::write(&library, kept).unwrap();
    std::fs::remove_file(r.0.path().join("files").join(&taken)).unwrap();
    std::fs::write(
        r.0.path().join("head/revoked.toml"),
        format!("[[revoked]]\nid='library'\nsha256='{taken}'\nreason='Withdrawn test file.'\n"),
    )
    .unwrap();
    assert_eq!(
        r.run(
            "review",
            &["base", "head"],
            &["--author", "author", "--json"]
        ),
        Ok(())
    );
    assert_eq!(r.check(), Ok(()));
    let out = r.0.path().join("index.json");
    assert_eq!(
        r.run("build", &["head"], &["--out", out.to_str().unwrap()]),
        Ok(())
    );
    let index = Index::parse(&std::fs::read_to_string(out).unwrap()).unwrap();
    let plan = sunrise_mod_package::index::resolve::install(
        &index,
        &[sunrise_mod_package::index::resolve::Request {
            id: "root".into(),
            version: None,
        }],
        &[],
        "26.3",
        sunrise_mod_package::manifest::Side::Client,
        |_, _| true,
    );
    let RequestResult::Refused { reason } = &plan.requested["root"] else {
        panic!("{plan:?}");
    };
    assert!(
        reason.contains("library") && reason.contains("^1"),
        "{reason}"
    );
}

#[test]
fn a_refused_repo_cache_fails_build_check_and_review_without_any_collection() {
    use std::fs;
    use sunrise_mod_package::index::registry::Cache;

    let r = Registry::new();
    let root = r.0.path();
    let out = root.join("index.json");
    let build = || r.run("build", &["head"], &["--out", out.to_str().unwrap()]);
    r.add("following", "1.0.0", "26.3", "");
    build().unwrap();
    let index = Index::parse(&fs::read_to_string(&out).unwrap()).unwrap();
    let cache = Cache {
        versions: index.project("following").unwrap().versions.clone(),
        ..Cache::default()
    };
    let entry = root.join("head/mods/following.toml");
    let inline = fs::read_to_string(&entry).unwrap();
    let (page, _) = inline.split_once("[[versions]]").unwrap();
    fs::write(&entry, format!("{page}repo='owner/following'\n")).unwrap();
    let cache_path = root.join("head/versions/following.json");
    fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
    fs::write(&cache_path, serde_json::to_vec(&cache).unwrap()).unwrap();
    build().unwrap();
    assert!(
        Index::parse(&fs::read_to_string(&out).unwrap())
            .unwrap()
            .project("following")
            .is_some()
    );

    fs::create_dir_all(root.join("base/mods")).unwrap();
    fs::copy(&entry, root.join("base/mods/following.toml")).unwrap();
    let review = || {
        super::main(&[
            "review".into(),
            root.join("base").to_str().unwrap().into(),
            root.join("head").to_str().unwrap().into(),
            "--author".into(),
            "author".into(),
        ])
    };
    fs::write(&cache_path, b"{invalid JSON").unwrap();
    for result in [build(), r.check(), review()] {
        let error = result.unwrap_err();
        assert!(error.contains("following.json"), "{error}");
    }
    fs::remove_file(&cache_path).unwrap();
    fs::create_dir(&cache_path).unwrap();
    let error = build().unwrap_err();
    assert!(error.contains("following.json"), "{error}");
}

#[test]
fn a_registry_command_reads_its_own_options_and_refuses_others() {
    let args = |line: &str| line.split(' ').map(str::to_owned).collect::<Vec<_>>();
    // Each is refused before the checkout is read, naming the option.
    for line in [
        "build repo --files f --out o --relase",
        "review a b --author x --jsno",
        "check a b --files f --jsno",
        "list repo --files f",
        "release repo id file --tag t --published d --nope",
        "page repo id file --nope",
    ] {
        let error = super::main(&args(line)).unwrap_err();
        let option = line.rsplit(' ').find(|w| w.starts_with("--")).unwrap();
        assert!(error.contains(option), "{line}: {error}");
        assert!(error.contains("does not take"), "{line}: {error}");
    }
    // Every option a parser reads is taken, `release --name` among them: the command goes on to
    // the checkout, which is not there.
    for line in [
        "build repo --files f --out o --release r --site s --generated g --counts c",
        "review a b --author x --files f --json",
        "check a b --files f --json",
        "release repo id file --tag t --published d --notes n --signature s --name n --game 26.3",
    ] {
        let error = super::main(&args(line)).unwrap_err();
        assert!(!error.contains("does not take"), "{line}: {error}");
    }
}

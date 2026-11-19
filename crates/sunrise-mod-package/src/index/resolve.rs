//! The registry's install and removal rules. Planning does not download or write any files.
use super::{Dependency, Index, Kind, Project, Version, dependencies::required_cycle};
use crate::manifest::parse_version;
use mod_vocab::Side;
use semver::{Comparator, Op, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

// Share the search budget across retries instead of multiplying it by the number of root subsets.
const MAX_STATES: usize = 20_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModVersion {
    pub id: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledMod {
    pub id: String,
    pub version: String,
    /// Only an exact hash match identifies the registry artifact. Otherwise this is a local copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

impl InstalledMod {
    fn is_file(&self, version: &Version) -> bool {
        self.version == version.version
            && self
                .sha256
                .as_ref()
                .is_some_and(|h| h.eq_ignore_ascii_case(&version.file.sha256))
    }
}

type Present<'a> = BTreeMap<&'a str, &'a InstalledMod>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    /// None keeps an eligible installed version or asks for the newest one that works.
    pub version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RequestResult {
    Installable { version: String },
    AlreadyInstalled { version: String },
    Refused { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Update {
    pub id: String,
    pub from: String,
    pub to: String,
    /// The replacement has lower SemVer precedence than the installed version.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub downgrade: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionalDependency {
    pub from: ModVersion,
    pub dependency: Dependency,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InstallPlan {
    pub requested: BTreeMap<String, RequestResult>,
    pub install: Vec<ModVersion>,
    pub kept: Vec<ModVersion>,
    /// Already on disk and in range, but the registry has no manifest facts for this version.
    pub kept_unindexed: Vec<ModVersion>,
    pub update: Vec<Update>,
    pub optional: Vec<OptionalDependency>,
    /// Dependency id to requested ids that need it, directly or transitively.
    pub required_by: BTreeMap<String, Vec<String>>,
}

/// Resolve requested mods and their required dependencies. A connected group of requests is
/// planned together or refused together; independent groups can still be installed. Every plan
/// list is sorted by id (optional declarations by declaring id then dependency id).
///
/// `native_fits` is the caller's per-file SDK fit decision. The index only knows what build a
/// native mod was made for, not whether its used API surface is unchanged on another build.
/// `side` is the installation (client or dedicated server); `Both` considers all declarations.
/// The caller checks takedowns by hash for installed files: only it has their actual bytes.
/// A local copy with no matching index hash has no dependency metadata to walk. This planner
/// checks its version range and any project-wide takedown; the loader checks its manifest.
pub fn install(
    index: &Index,
    requests: &[Request],
    installed: &[InstalledMod],
    game: &str,
    side: Side,
    native_fits: impl Fn(&Project, &Version) -> bool,
) -> InstallPlan {
    let mut plan = InstallPlan::default();
    if requests.is_empty() {
        return plan;
    }
    let mut present = BTreeMap::new();
    for m in installed {
        if let Some(previous) = present.insert(m.id.as_str(), m)
            && previous != m
        {
            let reason = format!(
                "{} has conflicting installed files ({} and {}): keep only one",
                m.id, previous.version, m.version
            );
            plan.requested.extend(requests.iter().map(|r| {
                (
                    r.id.clone(),
                    RequestResult::Refused {
                        reason: reason.clone(),
                    },
                )
            }));
            return plan;
        }
    }
    let catalog = Catalog::new(index, game, side, native_fits, &present);
    let installed_edges: Selected<'_> = present
        .keys()
        .filter_map(|&id| Some((id, catalog.mods.get(id)?.iter().find(|c| c.installed)?)))
        .collect();

    let mut roots = Roots::new();
    for r in requests {
        roots.entry(&r.id).or_default().extend(r.version.as_deref());
    }
    let mut groups = Vec::new();
    let mut blocked = Vec::new();
    for (&id, pins) in &roots {
        let single = Roots::from([(id, pins.clone())]);
        let mut budget = MAX_STATES;
        match catalog.solve(&single, &present, &installed_edges, &mut budget) {
            Ok(selected) => groups.push((single, selected)),
            Err(failure) => blocked.push((id, failure, budget)),
        }
    }
    for (id, failure, mut budget) in blocked {
        if groups.iter().any(|(r, _)| r.contains_key(id)) {
            continue; // A joint solve for another blocked request has already found a plan.
        }
        // Retry only the requested replacements that could change a failed assignment. Try
        // each one separately first so a broken pin on another branch cannot poison the plan.
        let mut ids = BTreeSet::from([id]);
        let mut tried = BTreeSet::from([ids.clone()]);
        let mut pending = VecDeque::new();
        let mut replacements = failure.replacements;
        let solved = loop {
            for replacement in replacements {
                if !roots.contains_key(replacement) || ids.contains(replacement) {
                    continue;
                }
                let mut joint = ids.clone();
                match groups.iter().find(|(r, _)| r.contains_key(replacement)) {
                    Some((group, _)) => joint.extend(group.keys().copied()),
                    None => {
                        joint.insert(replacement);
                    }
                }
                if tried.insert(joint.clone()) {
                    pending.push_back(joint);
                }
            }
            let Some(next) = pending.pop_front() else {
                break Err(failure.reason);
            };
            ids = next;
            let joint = ids.iter().map(|&id| (id, roots[id].clone())).collect();
            match catalog.solve(&joint, &present, &installed_edges, &mut budget) {
                Ok(selected) => break Ok((joint, selected)),
                Err(retry) if budget == 0 => break Err(retry.reason),
                Err(retry) => replacements = retry.replacements,
            }
        };
        match solved {
            Ok((joint, selected)) => {
                // The retry includes every root of any group it absorbed.
                groups.retain(|(r, _)| !r.keys().any(|id| joint.contains_key(id)));
                groups.push((joint, selected));
            }
            Err(reason) => {
                plan.requested
                    .insert(id.to_owned(), RequestResult::Refused { reason });
            }
        }
    }
    // Group only feasible selections. If joint solving picks different dependencies,
    // repeat: new overlaps or active optional edges may tie that plan to another group.
    loop {
        let owners: BTreeMap<_, _> = groups
            .iter()
            .enumerate()
            .flat_map(|(i, (_, selected))| selected.keys().map(move |&id| (id, i)))
            .collect();
        let pair = groups.iter().enumerate().find_map(|(i, (_, selected))| {
            selected.iter().find_map(|(&id, c)| {
                std::iter::once(id)
                    .chain(c.optional.iter().map(|r| r.row.id.as_str()))
                    // Replacing an installed owner can change another selection's guards.
                    .chain(
                        installed_edges
                            .get(id)
                            .into_iter()
                            .flat_map(|old| old.required.iter().chain(&old.optional))
                            .map(|r| r.row.id.as_str()),
                    )
                    .filter_map(|target| owners.get(target).copied())
                    .find(|&j| i != j)
                    .map(|j| (i.min(j), i.max(j)))
            })
        });
        let Some((i, j)) = pair else { break };
        let (other, _) = groups.remove(j);
        groups[i].0.extend(other);
        let roots = &groups[i].0;
        let mut budget = MAX_STATES;
        match catalog.solve(roots, &present, &installed_edges, &mut budget) {
            Ok(selected) => groups[i].1 = selected,
            Err(failure) => {
                let ids = roots.keys().copied().collect::<Vec<_>>().join(", ");
                let reason = format!("{ids} cannot be installed together: {}", failure.reason);
                for &id in roots.keys() {
                    plan.requested.insert(
                        id.to_owned(),
                        RequestResult::Refused {
                            reason: reason.clone(),
                        },
                    );
                }
                groups.remove(i);
            }
        }
    }
    let mut needed_by = BTreeMap::<_, BTreeSet<_>>::new();
    for (groups, selected) in groups {
        for &root in groups.keys() {
            let mut todo = vec![root];
            let mut visited = BTreeSet::new();
            while let Some(id) = todo.pop() {
                if !visited.insert(id) {
                    continue;
                }
                if id != root {
                    needed_by
                        .entry(id.to_owned())
                        .or_default()
                        .insert(root.to_owned());
                }
                todo.extend(selected[id].required.iter().map(|d| d.row.id.as_str()));
            }
        }
        for (id, candidate) in selected {
            let version = candidate.version;
            let row = ModVersion {
                id: id.into(),
                version: version.into(),
            };
            let outcome = match present.get(id) {
                Some(_) if candidate.installed => {
                    if candidate.unindexed {
                        plan.kept_unindexed.push(row.clone());
                    } else {
                        plan.kept.push(row.clone());
                    }
                    RequestResult::AlreadyInstalled {
                        version: version.into(),
                    }
                }
                Some(old) => {
                    let downgrade = candidate
                        .number
                        .as_ref()
                        .zip(parse_version(&old.version).ok().as_ref())
                        .is_some_and(|(next, previous)| next.cmp_precedence(previous).is_lt());
                    plan.update.push(Update {
                        id: id.into(),
                        from: old.version.clone(),
                        to: version.into(),
                        downgrade,
                    });
                    RequestResult::Installable {
                        version: version.into(),
                    }
                }
                None => {
                    plan.install.push(row.clone());
                    RequestResult::Installable {
                        version: version.into(),
                    }
                }
            };
            if groups.contains_key(id) {
                plan.requested.insert(id.into(), outcome);
            }
            plan.optional
                .extend(candidate.optional.iter().map(|dep| OptionalDependency {
                    from: row.clone(),
                    dependency: dep.row.clone(),
                }));
        }
    }
    plan.install.sort_by(|a, b| a.id.cmp(&b.id));
    plan.kept.sort_by(|a, b| a.id.cmp(&b.id));
    plan.kept_unindexed.sort_by(|a, b| a.id.cmp(&b.id));
    plan.update.sort_by(|a, b| a.id.cmp(&b.id));
    plan.optional
        .sort_by(|a, b| (&a.from.id, &a.dependency.id).cmp(&(&b.from.id, &b.dependency.id)));
    plan.required_by = needed_by
        .into_iter()
        .map(|(id, by)| (id, by.into_iter().collect()))
        .collect();
    plan
}

/// List installed mods that directly or transitively require `id`, from the indexed metadata
/// for matching installed artifacts. An optional or out-of-side dependency is not a blocker.
pub fn removal_dependents(
    index: &Index,
    installed: &[InstalledMod],
    id: &str,
    side: Side,
) -> Result<Vec<ModVersion>, String> {
    let mut reverse = BTreeMap::<&str, Vec<&InstalledMod>>::new();
    for m in installed {
        let Some(version) = index
            .project(&m.id)
            .filter(|p| p.kind.is_mod())
            .and_then(|p| p.versions.iter().find(|v| m.is_file(v)))
        else {
            continue;
        };
        for dep in version
            .dependencies
            .iter()
            .flatten()
            .filter(|d| !d.optional && d.applies(side))
        {
            dep.range(&m.id, &m.version)?;
            reverse.entry(&dep.id).or_default().push(m);
        }
    }
    let mut seen = BTreeSet::from([id]);
    let mut queue = vec![id];
    let mut dependents = Vec::new();
    while let Some(id) = queue.pop() {
        for m in reverse.get(id).into_iter().flatten() {
            if seen.insert(&m.id) {
                queue.push(&m.id);
                dependents.push(ModVersion {
                    id: m.id.clone(),
                    version: m.version.clone(),
                });
            }
        }
    }
    dependents.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(dependents)
}

#[derive(PartialEq, Eq)]
enum Fit {
    Yes,
    Revoked,
    WrongGame,
    Unsupported(String),
    WrongSunrise,
}

struct Range<'a> {
    row: &'a Dependency,
    parsed: Result<VersionReq, String>,
}

struct Candidate<'a> {
    version: &'a str,
    installed: bool,
    unindexed: bool,
    number: Option<semver::Version>,
    fit: Fit,
    required: Vec<Range<'a>>,
    optional: Vec<Range<'a>>,
}

struct Catalog<'a> {
    game: &'a str,
    mods: BTreeMap<&'a str, Vec<Candidate<'a>>>,
}

type Roots<'a> = BTreeMap<&'a str, Vec<&'a str>>;
type Selected<'a> = BTreeMap<&'a str, &'a Candidate<'a>>;

struct SolveFailure<'a> {
    reason: String,
    /// Installed artifacts whose replacement may make a failed choice viable.
    replacements: BTreeSet<&'a str>,
}

struct Need<'a> {
    pins: &'a [&'a str],
    ranges: Vec<(&'a str, &'a Range<'a>)>,
}

impl Need<'_> {
    fn accepts(&self, candidate: &Candidate<'_>) -> bool {
        self.pins.iter().all(|v| *v == candidate.version)
            && self.ranges.iter().all(|(_, r)| {
                r.parsed
                    .as_ref()
                    .is_ok_and(|req| candidate.number.as_ref().is_some_and(|n| req.matches(n)))
            })
    }

    fn describe(&self) -> String {
        self.pins
            .iter()
            .map(|v| format!("requested {v}"))
            .chain(
                self.ranges
                    .iter()
                    .map(|(owner, r)| format!("{owner} needs {}", r.row.version)),
            )
            .collect::<Vec<_>>()
            .join("; ")
    }
}

impl<'a> Catalog<'a> {
    fn new(
        index: &'a Index,
        game: &'a str,
        side: Side,
        native_fits: impl Fn(&Project, &Version) -> bool,
        present: &Present<'a>,
    ) -> Self {
        let mut mods: BTreeMap<_, Vec<_>> = index
            .projects
            .iter()
            .filter(|p| p.kind.is_mod())
            .map(|p| {
                let mut versions: Vec<_> = p
                    .versions
                    .iter()
                    .map(|v| {
                        let mut required = Vec::new();
                        let mut optional = Vec::new();
                        for dep in v.dependencies.iter().flatten().filter(|d| d.applies(side)) {
                            let parsed = dep.range(&p.id, &v.version);
                            if dep.optional {
                                optional.push(Range { row: dep, parsed });
                            } else {
                                required.push(Range { row: dep, parsed });
                            }
                        }
                        Candidate {
                            version: &v.version,
                            installed: present.get(p.id.as_str()).is_some_and(|m| m.is_file(v)),
                            unindexed: false,
                            number: parse_version(&v.version).ok(),
                            fit: if index.revoked(&p.id, &v.file.sha256).is_some() {
                                Fit::Revoked
                            } else if v.game != game {
                                Fit::WrongGame
                            } else if let Some(reason) = v.capabilities.unsupported() {
                                Fit::Unsupported(reason)
                            } else if p.kind == Kind::NativeMod && !native_fits(p, v) {
                                Fit::WrongSunrise
                            } else {
                                Fit::Yes
                            },
                            required,
                            optional,
                        }
                    })
                    .collect();
                // Equal SemVer precedence keeps the index's newest-first order.
                versions.sort_by(|a, b| match (&a.number, &b.number) {
                    (Some(a), Some(b)) => b.cmp_precedence(a),
                    _ => b.number.is_some().cmp(&a.number.is_some()),
                });
                (p.id.as_str(), versions)
            })
            .collect();
        for (&id, &row) in present {
            let version = &row.version;
            let candidates = mods.entry(id).or_default();
            if !candidates.iter().any(|c| c.installed) {
                candidates.push(Candidate {
                    version,
                    installed: true,
                    unindexed: true,
                    number: parse_version(version).ok(),
                    fit: if index.revoked(id, "").is_some() {
                        Fit::Revoked
                    } else {
                        Fit::Yes
                    },
                    required: Vec::new(),
                    optional: Vec::new(),
                });
            }
        }
        Self { game, mods }
    }

    fn choices(&self, id: &str, need: &Need<'_>) -> Result<Vec<&Candidate<'a>>, String> {
        let describe = || {
            let needs = need.describe();
            if needs.is_empty() {
                id.to_owned()
            } else {
                format!("{id} ({needs})")
            }
        };
        let Some(versions) = self.mods.get(id) else {
            return Err(format!(
                "{}: not listed as a mod in the registry",
                describe()
            ));
        };
        let mut matches = versions.iter().filter(|c| need.accepts(c)).peekable();
        if matches.peek().is_none() {
            if need.pins.is_empty()
                && !need.ranges.is_empty()
                && let Some(pre) = versions
                    .iter()
                    .filter(|c| !c.unindexed)
                    .filter_map(|c| c.number.as_ref())
                    .find(|n| {
                        !n.pre.is_empty()
                            && need.ranges.iter().all(|(_, r)| {
                                r.parsed.as_ref().is_ok_and(|range| {
                                    range.matches(n) || excluded_as_prerelease(range, n)
                                })
                            })
                    })
            {
                return Err(format!(
                    "{}: no listed version satisfies all requirements; use a range that names the prerelease, such as ={pre}",
                    describe()
                ));
            }
            return Err(format!(
                "{}: no listed version satisfies all requirements",
                describe()
            ));
        }
        let mut unrevoked = matches.filter(|c| c.fit != Fit::Revoked).peekable();
        if unrevoked.peek().is_none() {
            return Err(format!(
                "{}: every matching version was taken down",
                describe()
            ));
        }
        let mut game_fits = unrevoked.filter(|c| c.fit != Fit::WrongGame).peekable();
        let Some(first) = game_fits.peek().copied() else {
            return Err(format!(
                "{}: no matching version fits this instance (Minecraft {})",
                describe(),
                self.game
            ));
        };
        let mut supported = game_fits
            .filter(|c| !matches!(c.fit, Fit::Unsupported(_)))
            .peekable();
        if supported.peek().is_none()
            && let Fit::Unsupported(reason) = &first.fit
        {
            return Err(format!("{} {}: {reason}", describe(), first.version));
        }
        let fits: Vec<_> = supported.filter(|c| c.fit == Fit::Yes).collect();
        if fits.is_empty() {
            return Err(format!(
                "{}: no matching native version fits this Sunrise build",
                describe()
            ));
        }
        Ok(fits)
    }

    /// Depth-first search over version choices, not over mods on the call stack. New requirements
    /// can narrow an earlier choice; undo it and retry instead of keeping stale outgoing edges.
    fn solve<'s>(
        &'s self,
        roots: &Roots<'s>,
        present: &Present<'_>,
        installed: &Selected<'s>,
        budget: &mut usize,
    ) -> Result<Selected<'s>, SolveFailure<'s>>
    where
        'a: 's,
    {
        let mut selected = Selected::new();
        let mut stack = Vec::<(&str, std::vec::IntoIter<&Candidate<'_>>)>::new();
        let mut failure = None;
        let mut replacements = BTreeSet::new();
        while *budget > 0 {
            *budget -= 1;
            let step = (|| {
                let mut needs: BTreeMap<_, _> = roots
                    .iter()
                    .map(|(&id, pins)| {
                        (
                            id,
                            Need {
                                pins: pins.as_slice(),
                                ranges: Vec::new(),
                            },
                        )
                    })
                    .collect();
                for (&id, &c) in &selected {
                    for range in &c.required {
                        range.parsed.as_ref().map_err(Clone::clone)?;
                        needs
                            .entry(range.row.id.as_str())
                            .or_insert(Need {
                                pins: &[],
                                ranges: Vec::new(),
                            })
                            .ranges
                            .push((id, range));
                    }
                }
                // Optional ranges constrain a selected target, or a target already installed.
                // They never add a target to the closure. Check the fixed installed owners
                // only once no pending selection can replace them.
                if needs.keys().all(|id| selected.contains_key(id)) {
                    for (&owner, c) in &selected {
                        for range in &c.optional {
                            let target = range.row.id.as_str();
                            if let Some(need) = needs.get_mut(target) {
                                range.parsed.as_ref().map_err(Clone::clone)?;
                                need.ranges.push((owner, range));
                            } else if let Some(row) = present.get(target) {
                                let req = range.parsed.as_ref().map_err(Clone::clone)?;
                                if !parse_version(&row.version).is_ok_and(|n| req.matches(&n)) {
                                    replacements.insert(target);
                                    return Err(format!(
                                        "{target} {} is installed, but {owner} needs {}",
                                        row.version, range.row.version
                                    ));
                                }
                            }
                        }
                    }
                    for (&owner, &c) in installed {
                        if !selected.contains_key(owner) {
                            for range in c.required.iter().chain(&c.optional) {
                                let target = range.row.id.as_str();
                                if let Some(need) = needs.get_mut(target) {
                                    let req = range.parsed.as_ref().map_err(Clone::clone)?;
                                    if !selected[target]
                                        .number
                                        .as_ref()
                                        .is_some_and(|n| req.matches(n))
                                    {
                                        replacements.insert(owner);
                                    }
                                    need.ranges.push((owner, range));
                                }
                            }
                        }
                    }
                }
                for (&id, &chosen) in &selected {
                    if let Some(need) = needs.get(id)
                        && !need.accepts(chosen)
                    {
                        // Distinguish a true disjoint range from a choice another branch can undo.
                        self.choices(id, need)?;
                        return Err(format!(
                            "{id} {} does not fit all requirements ({})",
                            chosen.version,
                            need.describe()
                        ));
                    }
                }
                let next = roots
                    .keys()
                    .chain(needs.keys())
                    .find(|id| !selected.contains_key(**id));
                if let Some(&id) = next {
                    let mut choices = self.choices(id, &needs[id])?;
                    if let Some(at) = choices.iter().position(|c| c.installed) {
                        choices[..=at].rotate_right(1);
                    }
                    return Ok(Some((id, choices.into_iter())));
                }
                let nodes: Vec<_> = selected.keys().copied().collect();
                let edges: Vec<_> = selected
                    .values()
                    .map(|c| {
                        c.required
                            .iter()
                            .filter_map(|r| nodes.binary_search(&r.row.id.as_str()).ok())
                            .collect()
                    })
                    .collect();
                if let Some(cycle) = required_cycle(&edges) {
                    let names: Vec<_> = cycle
                        .into_iter()
                        .map(|i| format!("{} {}", nodes[i], selected[nodes[i]].version))
                        .collect();
                    return Err(format!("required dependency cycle: {}", names.join(" -> ")));
                }
                Ok(None)
            })();
            match step {
                Ok(None) => return Ok(selected),
                Ok(Some(frame)) => stack.push(frame),
                Err(reason) => {
                    failure.get_or_insert(reason);
                }
            }
            while let Some((id, choices)) = stack.last_mut() {
                if let Some(next) = choices.next() {
                    selected.insert(*id, next);
                    break;
                }
                selected.remove(id);
                stack.pop();
            }
            if stack.is_empty() {
                return Err(SolveFailure {
                    reason: failure
                        .unwrap_or_else(|| "no consistent versions for the requested mods".into()),
                    replacements,
                });
            }
        }
        Err(SolveFailure {
            reason: format!(
                "{}: dependency resolution limit reached; try fewer requests or pin a version",
                roots.keys().copied().collect::<Vec<_>>().join(", ")
            ),
            replacements,
        })
    }
}

/// Check selected versions as exact pins, reusing the install catalog per Minecraft release and side.
/// This has no installed files and assumes native SDK fit: a publisher can't know a player's build.
/// Package side is a join contract; only dependency side filters client and dedicated installations.
pub fn uninstallable_versions(index: &Index, include: impl Fn(&Version) -> bool) -> Vec<String> {
    let mut by_game = BTreeMap::<_, Vec<_>>::new();
    for p in index.projects.iter().filter(|p| p.kind.is_mod()) {
        for v in p.versions.iter().filter(|v| include(v)) {
            by_game
                .entry(v.game.as_str())
                .or_default()
                .push((p.id.as_str(), v.version.as_str()));
        }
    }
    let mut problems = Vec::new();
    for (game, versions) in by_game {
        let present = Present::new();
        let installed = Selected::new();
        for (side, label) in [(Side::Client, "client"), (Side::Server, "dedicated server")] {
            let catalog = Catalog::new(index, game, side, |_, _| true, &present);
            for &(id, version) in &versions {
                let roots = Roots::from([(id, vec![version])]);
                let mut budget = MAX_STATES;
                if let Err(failure) = catalog.solve(&roots, &present, &installed, &mut budget) {
                    problems.push(format!("{id} {version} ({label}): {}", failure.reason));
                }
            }
        }
    }
    problems
}

/// Test the same comparators with an extra exact comparator to opt in to the prerelease. If it
/// then matches, SemVer's prerelease opt-in (rather than the numeric bounds) was the only block.
fn excluded_as_prerelease(range: &VersionReq, version: &semver::Version) -> bool {
    if version.pre.is_empty() {
        return false;
    }
    let mut opted_in = range.clone();
    opted_in.comparators.push(Comparator {
        op: Op::Exact,
        major: version.major,
        minor: Some(version.minor),
        patch: Some(version.patch),
        pre: version.pre.clone(),
    });
    opted_in.matches(version)
}

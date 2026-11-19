use super::*;

#[test]
fn independent_role_files_extend_both_tables_and_renames_cannot_leave_stale_roles() {
    let previous = Declared {
        imports: imports().iter().map(|name| (*name).to_owned()).collect(),
        exports: exports().iter().map(|name| (*name).to_owned()).collect(),
        held: held().iter().map(|name| (*name).to_owned()).collect(),
        refusals: CURRENT.with(|current| {
            current
                .get()
                .map(|roles| roles.refusals.clone())
                .unwrap_or_default()
        }),
        paths: Vec::new(),
    };
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("crates/sunrise-mod-vocab/src/wire/traits");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("Context.import"),
        "Borrowed receive context.",
    )
    .unwrap();
    std::fs::write(
        directory.join("Handler.export"),
        "refusal: error Failure::Rejected\nOwned callback.",
    )
    .unwrap();
    std::fs::write(
        directory.join("Kept.held"),
        "refusal: unwind\nOwned callback the game hands back.",
    )
    .unwrap();
    let found = declared(root.path()).unwrap();
    assert!(found.paths.contains(&directory));
    select(&found);
    assert_eq!(exports(), ["Handler", "Kept"]);
    assert_eq!(held(), ["Kept"]);
    assert_eq!(refusal("Kept"), Some(&Refusal::Unwind));
    assert_eq!(
        refusal("Handler"),
        Some(&Refusal::Error("Failure::Rejected".into()))
    );
    assert!(imports().contains(&"Context"));
    assert!(exports().contains(&"Handler"));
    assert!(!exports().contains(&"Context"));
    std::fs::remove_file(directory.join("Context.import")).unwrap();
    std::fs::write(directory.join("Replacement.import"), "Replacement context.").unwrap();
    select(&declared(root.path()).unwrap());
    assert!(!imports().contains(&"Context"));
    assert!(imports().contains(&"Replacement"));
    // Concurrent source generation must not replace this thread's selected table.
    std::thread::spawn(|| {
        select(&Declared {
            imports: vec!["OtherContext".into()],
            exports: vec![],
            held: vec![],
            paths: vec![],
            refusals: BTreeMap::new(),
        })
    })
    .join()
    .unwrap();
    assert!(imports().contains(&"Replacement"));
    assert!(!imports().contains(&"OtherContext"));
    select(&previous);
}

#[test]
fn duplicate_or_ambiguous_roles_and_non_trait_names_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("crates/sunrise-mod-vocab/src/wire/traits");
    std::fs::create_dir_all(&directory).unwrap();
    for (filename, other) in [
        ("Doubled.export", Some("Doubled.held")),
        ("Duplicate.import", Some("Duplicate.export")),
        ("not-a-trait.import", None),
    ] {
        let path = directory.join(filename);
        std::fs::write(&path, "Role.").unwrap();
        if let Some(other) = other {
            std::fs::write(directory.join(other), "Conflicting role.").unwrap();
        }
        assert!(declared(root.path()).is_err(), "{filename}");
        std::fs::remove_file(path).unwrap();
        if let Some(other) = other {
            std::fs::remove_file(directory.join(other)).unwrap();
        }
    }
}

#[test]
fn invalid_or_duplicate_refusal_policies_are_rejected_at_the_role_boundary() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("crates/sunrise-mod-vocab/src/wire/traits");
    std::fs::create_dir_all(&directory).unwrap();
    for (file, note) in [
        ("Callback.held", "refusal: default"),
        ("Callback.held", "refusal: error not-a-path"),
        ("Callback.held", "refusal: unwind\nrefusal: unwind"),
        ("Context.import", "refusal: unwind"),
    ] {
        let path = directory.join(file);
        std::fs::write(&path, note).unwrap();
        assert!(declared(root.path()).is_err(), "{file}: {note}");
        std::fs::remove_file(path).unwrap();
    }
}

use super::*;

#[test]
fn ownership_lock_names_match_the_server_contract_and_deduplicate_aliases() {
    let mut identity = ResolvedInstancePaths {
        data: "/fixture/data".into(),
        config: "/fixture/config.toml".into(),
    };
    let registry = Path::new("registry");
    assert_eq!(
        identity.ownership_lock_paths(registry).unwrap(),
        vec![
            registry.join("a409bc6f490d0f93f26c081480a7f6f859fc9c28a485107ca15e43cce09bf777.lock"),
            registry.join("b2fa0b9a7e4507490dcf3dcdad6e6f2d5a2565d77e5e14d26d677a68d4171024.lock"),
        ]
    );
    identity.config = identity.data.clone();
    assert_eq!(identity.ownership_lock_paths(registry).unwrap().len(), 1);
}

#[test]
fn process_split_contract_missing_tail_parents_resume_existing_resolution() {
    let root = tempfile::tempdir().unwrap();
    let existing = root.path().join("existing");
    std::fs::create_dir(&existing).unwrap();
    let first = ResolvedInstancePaths::resolve(&existing, &existing.join("config.toml")).unwrap();
    let alias = root.path().join("missing/nested/../../existing");
    let second = ResolvedInstancePaths::resolve(&alias, &alias.join("config.toml")).unwrap();
    assert_eq!(first, second);
    assert!(!root.path().join("missing").exists());
    assert!(!existing.join("config.toml").exists());
}

#[cfg(windows)]
#[test]
fn process_split_contract_verbatim_missing_parent_keeps_fresh_identity() {
    let root = tempfile::tempdir().unwrap();
    let canonical = root.path().canonicalize().unwrap();
    let data = canonical.join("fresh");
    let first = ResolvedInstancePaths::resolve(&data, &data.join("config.toml")).unwrap();
    let alias = data.join("absent/..");
    let second = ResolvedInstancePaths::resolve(&alias, &data.join("config.toml")).unwrap();
    assert_eq!(first, second);
    assert!(!data.exists());
}

#[test]
fn process_split_contract_file_parent_never_becomes_directory_identity() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file");
    std::fs::write(&file, b"owned fixture").unwrap();
    for alias in [file.join("../fresh"), file.join("fresh")] {
        let error =
            ResolvedInstancePaths::resolve(&alias, &root.path().join("config")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotADirectory);
    }
    assert!(!root.path().join("fresh").exists());
}

#[cfg(unix)]
#[test]
fn process_split_contract_parent_symlink_resolves_before_missing_identity() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let first = ResolvedInstancePaths::resolve(&real.join("fresh"), &real.join("config")).unwrap();
    let second =
        ResolvedInstancePaths::resolve(&alias.join("fresh"), &alias.join("config")).unwrap();
    assert_eq!(first, second);
    assert!(!real.join("fresh").exists());
}

#[cfg(unix)]
#[test]
fn process_split_contract_parent_after_symlink_uses_target_parent() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    let nested = real.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&nested, &alias).unwrap();
    let normal = ResolvedInstancePaths::resolve(&real.join("fresh"), &real.join("config")).unwrap();
    let through_alias =
        ResolvedInstancePaths::resolve(&alias.join("../fresh"), &alias.join("../config")).unwrap();
    assert_eq!(normal, through_alias);
    assert!(!real.join("fresh").exists());
}

#[cfg(unix)]
#[test]
fn process_split_contract_dangling_alias_refuses_before_parent_reduction() {
    let root = tempfile::tempdir().unwrap();
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(root.path().join("missing"), &alias).unwrap();
    assert!(
        ResolvedInstancePaths::resolve(&alias.join("../fresh"), &root.path().join("config"),)
            .is_err()
    );
    assert!(!root.path().join("fresh").exists());
    assert!(!root.path().join("missing").exists());
}

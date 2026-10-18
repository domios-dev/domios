use super::*;

#[test]
fn process_split_contract_missing_identity_is_locked_before_initialization() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("fresh");
    let identity = ResolvedInstancePaths::resolve(&data, &data.join("config.toml")).unwrap();
    let registry = root.path().join("owners");
    let owner = identity.clone().try_acquire(&registry).unwrap();
    let mut writes = Vec::new();
    let loser = identity.clone().try_acquire(&registry).map(|_owner| {
        writes.extend(["mesh", "config", "credential", "hook", "crash"]);
    });
    assert_eq!(loser.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    assert!(writes.is_empty());
    assert!(!data.exists());
    drop(owner);
    assert!(identity.try_acquire(&registry).is_ok());
}

#[test]
fn process_split_contract_aliases_share_an_owner_without_creating_data() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("fresh");
    let first = ResolvedInstancePaths::resolve(&data, &data.join("config.toml")).unwrap();
    let alias = root.path().join("absent/../fresh/.");
    let second = ResolvedInstancePaths::resolve(&alias, &alias.join("config.toml")).unwrap();
    assert_eq!(first, second);
    let registry = root.path().join("owners");
    let _owner = first.try_acquire(&registry).unwrap();
    assert!(second.try_acquire(&registry).is_err());
    assert!(!data.exists());
}

#[test]
fn process_split_contract_shared_config_refuses_distinct_data_writers() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config.toml");
    let first = ResolvedInstancePaths::resolve(&root.path().join("one"), &config).unwrap();
    let second = ResolvedInstancePaths::resolve(&root.path().join("two"), &config).unwrap();
    let registry = root.path().join("owners");
    let _owner = first.try_acquire(&registry).unwrap();
    assert!(second.try_acquire(&registry).is_err());
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

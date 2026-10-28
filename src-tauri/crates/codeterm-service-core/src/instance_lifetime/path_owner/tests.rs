use super::*;
use std::io;

#[test]
fn process_split_contract_missing_identity_is_locked_before_initialization() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("fresh");
    let identity = ResolvedInstancePaths::resolve(&data, &data.join("config.toml")).unwrap();
    let registry = root.path().join("owners");
    let owner = PathOwnership::try_acquire(identity.clone(), &registry).unwrap();
    let mut writes = Vec::new();
    let loser = PathOwnership::try_acquire(identity.clone(), &registry).map(|_owner| {
        writes.extend(["mesh", "config", "credential", "hook", "crash"]);
    });
    assert_eq!(loser.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    assert!(writes.is_empty());
    assert!(!data.exists());
    drop(owner);
    assert!(PathOwnership::try_acquire(identity, &registry).is_ok());
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
    let _owner = PathOwnership::try_acquire(first, &registry).unwrap();
    assert!(PathOwnership::try_acquire(second, &registry).is_err());
    assert!(!data.exists());
}

#[test]
fn process_split_contract_shared_config_refuses_distinct_data_writers() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config.toml");
    let first = ResolvedInstancePaths::resolve(&root.path().join("one"), &config).unwrap();
    let second = ResolvedInstancePaths::resolve(&root.path().join("two"), &config).unwrap();
    let registry = root.path().join("owners");
    let _owner = PathOwnership::try_acquire(first, &registry).unwrap();
    assert!(PathOwnership::try_acquire(second, &registry).is_err());
}

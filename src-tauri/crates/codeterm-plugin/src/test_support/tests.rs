use super::*;

#[test]
fn runtime_repository_root_contains_plugin_channel() {
    assert!(fixture_repo_root().join("plugins/channel.json").is_file());
}

#[test]
fn repository_root_resolves_for_each_consuming_package() {
    let repository = tempfile::tempdir().unwrap();
    let root = repository.path();
    std::fs::create_dir(root.join("plugins")).unwrap();
    std::fs::write(root.join("plugins/channel.json"), "{}").unwrap();
    for package in ["cli", "src-tauri/crates/codeterm-plugin"] {
        let manifest_dir = root.join(package);
        std::fs::create_dir_all(&manifest_dir).unwrap();
        assert_eq!(find_repo_root(&manifest_dir).as_deref(), Some(root));
    }
    assert_eq!(find_repo_root(root).as_deref(), Some(root));
}

#[test]
fn repository_root_requires_channel_marker() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("plugins")).unwrap();
    assert_eq!(find_repo_root(directory.path()), None);
}

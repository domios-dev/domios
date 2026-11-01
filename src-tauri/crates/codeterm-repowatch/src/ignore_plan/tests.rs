use super::*;

#[test]
fn repository_global_and_unlisted_heavy_ignores_preserve_tracked_descendants() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("repo");
    let repo = Repository::init(&root).unwrap();
    let global = fixture.path().join("global-ignore");
    std::fs::write(&global, "global-output/\n").unwrap();
    repo.config()
        .unwrap()
        .set_str("core.excludesFile", global.to_str().unwrap())
        .unwrap();
    std::fs::write(repo.path().join("info/exclude"), "private-output/\n").unwrap();
    for directory in [
        "global-output",
        "private-output",
        "node_modules",
        "target/owned",
        "target/churn",
        "src",
    ] {
        std::fs::create_dir_all(root.join(directory)).unwrap();
    }
    std::fs::write(root.join("target/owned/input.rs"), "tracked").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("target/owned/input.rs")).unwrap();
    index.write().unwrap();
    let planned = plan(&root).unwrap();
    for ignored in [
        "global-output",
        "private-output",
        "node_modules",
        "target/churn",
    ] {
        assert!(
            planned
                .excluded
                .iter()
                .any(|entry| entry.path == root.join(ignored)),
            "{ignored}"
        );
        assert!(!planned
            .watches
            .iter()
            .any(|watch| watch.path.starts_with(root.join(ignored))));
    }
    for ancestor in ["", "target", "target/owned"] {
        assert!(planned
            .watches
            .iter()
            .any(|watch| watch.path == root.join(ancestor) && !watch.recursive));
    }
}

#[test]
fn repository_gitignore_overrides_global_exclusion() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("repo");
    let repo = Repository::init(&root).unwrap();
    let global = fixture.path().join("global-ignore");
    std::fs::write(&global, "source/\n").unwrap();
    repo.config()
        .unwrap()
        .set_str("core.excludesFile", global.to_str().unwrap())
        .unwrap();
    std::fs::write(root.join(".gitignore"), "!source/\n").unwrap();
    std::fs::create_dir(root.join("source")).unwrap();
    assert!(!plan(&root)
        .unwrap()
        .excluded
        .iter()
        .any(|entry| entry.path == root.join("source")));
}

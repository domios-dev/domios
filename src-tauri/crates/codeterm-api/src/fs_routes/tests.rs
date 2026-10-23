use super::*;

#[test]
fn file_suggestions_include_files_without_changing_folder_picker_results() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("cdp.mjs"), "opaque").unwrap();
    std::fs::write(root.path().join(".private"), "opaque").unwrap();
    std::fs::create_dir(root.path().join("folder")).unwrap();
    let names = |hidden, files| {
        directory_entries(root.path(), hidden, files)
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(false, true), ["cdp.mjs"]);
    assert_eq!(names(true, true), [".private", "cdp.mjs"]);
    assert_eq!(names(false, false), ["folder"]);
    let query: ListDirQuery = serde_json::from_value(serde_json::json!({"path":"opaque"})).unwrap();
    assert!(!query.files_only);
}

#[test]
fn picker_paths_simplify_windows_verbatim_paths_without_changing_normal_paths() {
    for (input, expected) in [
        (r"\\?\C:\Users\dev\ScoutSub", r"C:\Users\dev\ScoutSub"),
        (r"\\?\UNC\server\share\ScoutSub", r"\\server\share\ScoutSub"),
        (r"C:\Users\dev\ScoutSub", r"C:\Users\dev\ScoutSub"),
        ("/home/user/ScoutSub", "/home/user/ScoutSub"),
    ] {
        assert_eq!(display_path(Path::new(input)), expected);
    }
}

#[test]
fn picker_listing_returns_plain_paths_after_canonicalization() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("ScoutSub")).unwrap();
    let canonical = root.path().canonicalize().unwrap();
    let entries = directory_entries(&canonical, false, false).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].path, display_path(&canonical.join("ScoutSub")));
    assert!(!entries[0].path.starts_with(r"\\?\"));
    assert!(!display_path(&canonical).starts_with(r"\\?\"));
    assert!(!parent_path(&canonical).unwrap().starts_with(r"\\?\"));
}

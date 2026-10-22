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

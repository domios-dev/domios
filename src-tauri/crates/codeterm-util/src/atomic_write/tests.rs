use super::*;

#[test]
fn unique_tmp_paths_are_disjoint_and_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("plan.json");
    let a = unique_tmp_path(&base);
    let b = unique_tmp_path(&base);
    assert_ne!(a, b);
    assert_ne!(a, base);
    let name = a.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with(".plan.json."));
    assert!(name.ends_with(".tmp"));
}

#[test]
fn write_atomic_creates_parents_and_leaves_no_tmp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/deep/plan.json");
    write_atomic(&path, b"{\"k\":1}").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"k\":1}");
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_str().unwrap().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp file left behind: {leftovers:?}");
}

#[test]
fn write_atomic_overwrites_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plan.json");
    write_atomic(&path, b"old").unwrap();
    write_atomic(&path, b"new").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
}

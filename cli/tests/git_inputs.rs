#[path = "../build/git_inputs.rs"]
mod git_inputs;

#[test]
fn loose_refs_watch_the_file_and_packed_refs_watch_the_existing_parent() {
    let root = tempfile::tempdir().unwrap();
    let heads = root.path().join("refs/heads");
    std::fs::create_dir_all(&heads).unwrap();
    let reference = heads.join("develop");
    assert_eq!(git_inputs::existing_input(&reference), heads);
    std::fs::write(&reference, "commit").unwrap();
    assert_eq!(git_inputs::existing_input(&reference), reference);
}

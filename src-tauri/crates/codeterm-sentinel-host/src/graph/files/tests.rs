use super::*;

#[test]
fn sentinel_node_files_read_only_definition_paths_include_script_and_references() {
    let cwd = std::env::temp_dir().join("blueprint-files");
    let absolute = cwd.join("outside.txt");
    let source = format!(
        "python scripts/run.py --input=inputs/a.json '{}'",
        absolute.display()
    );
    let files = references(&source, &cwd, |_| FileKind::File);
    assert_eq!(files.len(), 3);
    assert_eq!(files[0].name, "run.py");
    assert_eq!(Path::new(&files[1].path), cwd.join("inputs/a.json"));
    assert_eq!(files[2].path, absolute.to_string_lossy());
    let missing = references(
        "python 'scripts/missing.py' $RUNTIME/file.txt https://host/file.txt",
        &cwd,
        |_| FileKind::Missing,
    );
    assert_eq!(missing.len(), 1);
    assert!(!missing[0].exists);
    let parent = references("linked/../data.json", &cwd, |_| FileKind::File);
    assert_eq!(Path::new(&parent[0].path), cwd.join("linked/../data.json"));
    let quoted = references("Get-Content 'a.json','b.json'", &cwd, |_| FileKind::File);
    assert_eq!(quoted.len(), 2);
    let duplicates = references("./inputs/a.json inputs/a.json", &cwd, |_| FileKind::File);
    assert_eq!(duplicates.len(), 1);
    assert!(references("assets/", &cwd, |_| FileKind::Other).is_empty());
}

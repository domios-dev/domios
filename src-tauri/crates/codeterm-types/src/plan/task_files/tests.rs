use super::*;
#[test]
fn ownership_uses_normalized_component_boundaries() {
    for path in [".GIT/config", "src/owned./file.rs", "src/owned /file.rs"] {
        assert!(normalize_edit_path(path).is_err());
    }
    let files = TaskFiles {
        edit: vec!["src/owned/".into()],
        ..Default::default()
    };
    assert!(files.owns_edit_path("src\\owned\\file.rs"));
    assert!(!files.owns_edit_path("src/owned-other/file.rs"));
    assert!(!files.owns_edit_path("src/owned/../foreign.rs"));
    assert_eq!(
        files.overlapping_edit_path(&TaskFiles {
            edit: vec!["src".into()],
            ..Default::default()
        }),
        Some("src/owned/")
    );
}
#[test]
fn task_files_validate_deduplicate_and_preserve_new_edit_paths() {
    let files = TaskFiles {
        edit: vec!["new.rs".into(), "new.rs".into()],
        read: vec!["context.md".into()],
    }
    .validated()
    .unwrap();
    assert_eq!(files.edit, ["new.rs"]);
    assert_eq!(files.read, ["context.md"]);
}
#[test]
fn task_files_refuse_invalid_and_conflicting_paths() {
    for path in ["", " ", "bad\npath"] {
        assert!(TaskFiles {
            edit: vec![path.into()],
            ..Default::default()
        }
        .validated()
        .is_err());
    }
    assert!(TaskFiles {
        edit: vec!["x".into()],
        read: vec!["x".into()]
    }
    .validated()
    .is_err());
    assert!(serde_json::from_str::<TaskFiles>(r#"{"write":["x"]}"#).is_err());
}

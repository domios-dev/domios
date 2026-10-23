use super::*;

#[test]
fn cleanup_decision_preserves_the_only_previous_version() {
    let backup = Path::new("plugins/fixture.rollback.tmp-install");
    assert_eq!(
        restore_destination(backup, false),
        Some(PathBuf::from("plugins/fixture"))
    );
    assert_eq!(restore_destination(backup, true), None);
    assert_eq!(
        restore_destination(Path::new("plugins/fixture.tmp-install"), false),
        None
    );
    assert_eq!(
        restore_destination(Path::new("plugins/...rollback.tmp-install"), false),
        None
    );
}

#[test]
fn failed_staging_is_removed_and_the_original_error_survives() {
    let root = tempfile::tempdir().unwrap();
    let staging = root.path().join("fixture.tmp-install");
    let result = with_cleanup(&staging, || {
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(staging.join("plugin.json"), "incomplete").unwrap();
        Err::<(), _>("invalid manifest".to_string())
    });
    assert_eq!(result, Err("invalid manifest".into()));
    assert!(!staging.exists());
}

#[test]
fn startup_removes_staging_and_recovers_interrupted_backup() {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "fixture.tmp-install",
        "fixture.rollback.tmp-install",
        "installed",
        "installed.rollback.tmp-install",
    ] {
        std::fs::create_dir(root.path().join(name)).unwrap();
        std::fs::write(root.path().join(name).join("plugin.json"), name).unwrap();
    }
    cleanup_stale(root.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.path().join("fixture/plugin.json")).unwrap(),
        "fixture.rollback.tmp-install"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("installed/plugin.json")).unwrap(),
        "installed"
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
}

#[cfg(windows)]
#[test]
fn open_child_refuses_directory_swap_but_leaves_no_staging() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let _context = crate::enter_ctx(crate::PluginCtx {
        data_root: root.path().into(),
        version: None,
    });
    let source = root.path().join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(
        source.join("plugin.json"),
        r#"{"id":"fixture","version":"2.0.0"}"#,
    )
    .unwrap();
    let destination = crate::plugins_root().join("fixture");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(
        destination.join("plugin.json"),
        r#"{"id":"fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(7)
        .open(destination.join("plugin.json"))
        .unwrap();
    let error = crate::install_from_dir(&source, "fixture", None).unwrap_err();
    assert!(error.contains("move previous plugin aside"), "{error}");
    assert!(!destination.with_extension("tmp-install").exists());
    assert!(!destination.with_extension("rollback.tmp-install").exists());
    assert_eq!(
        crate::manifest_version(&destination).unwrap().as_deref(),
        Some("1.0.0")
    );
    drop(held);
    crate::install_from_dir(&source, "fixture", None).unwrap();
    assert_eq!(
        crate::manifest_version(&destination).unwrap().as_deref(),
        Some("2.0.0")
    );
    assert_eq!(std::fs::read_dir(crate::plugins_root()).unwrap().count(), 1);
}

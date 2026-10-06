use super::*;

fn copied_root(root: &Path) {
    fs::create_dir_all(root.join("sessions")).expect("sessions directory");
    fs::create_dir_all(root.join("secrets")).expect("secrets directory");
    fs::create_dir_all(root.join("mesh")).expect("mesh directory");
    fs::write(
        root.join("sessions/last.json"),
        br#"{"version":"1","timestamp":1,"active_workspace_id":null,"workspaces":[],"teams":[]}"#,
    )
    .expect("desk snapshot");
    fs::write(
        root.join("sessions/agent-session-identities.json"),
        br#"{"sessions":{}}"#,
    )
    .expect("desk identities");
    fs::write(
        root.join("crons.json"),
        br#"{"version":"1","jobs":[],"history":{}}"#,
    )
    .expect("cron state");
    fs::write(root.join("sentinels.json"), b"[]").expect("sentinel state");
    fs::write(
        root.join("secrets/credential.json"),
        br#"{"secret":"private"}"#,
    )
    .expect("secret fixture");
    fs::write(root.join("mesh/private-key"), b"private key bytes").expect("mesh key fixture");
    fs::write(root.join("config.json"), br#"{"token":"private"}"#).expect("config token fixture");
}

fn make_tree_writable(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.file_type().is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                make_tree_writable(&entry.path());
            }
        }
    }
    let mut permissions = metadata.permissions();
    if permissions.readonly() {
        enable_writes(&mut permissions);
        let _ = fs::set_permissions(path, permissions);
    }
}

#[test]
fn storage_verify_receipt_proves_adapter_roundtrip_and_reopen() {
    let temp = tempfile::tempdir().expect("test root");
    let copy_root = temp.path().join("copy");
    let work_root = temp.path().join("work");
    fs::create_dir_all(&copy_root).expect("copy root");
    copied_root(&copy_root);

    let report = verify_copied_state(&copy_root, &work_root).expect("verification report");

    assert!(report.passed);
    assert!(report.originals_unchanged);
    assert_eq!(report.hashes_before, report.hashes_after);
    assert!(report.import.comparison_clean);
    assert!(report.import.capture_matches_plan);
    assert!(report.import.marker_matches_receipt);
    assert!(report.import.archive_sealed);
    assert_eq!(report.import.outcome, "Committed");
    assert_eq!(report.reopen.outcome, "AlreadyCommitted");
    assert!(report.reopen.marker_identical);
    assert!(report.reopen.projection_identical);
    assert!(report.tamper.archive_tamper_rejected);
    assert!(report.tamper.marker_absent);
    assert!(report.tamper.originals_unchanged);
    assert!(report
        .import
        .domains
        .iter()
        .chain(&report.reopen.domains)
        .all(|domain| domain.importer_diffs.is_empty()
            && domain.importer_diffs_elided == 0
            && domain.adapter_diffs.is_empty()
            && domain.adapter_diffs_elided == 0
            && domain.expected_entities == domain.committed_entities
            && domain.expected_entities == domain.adapter_entities
            && domain.expected_items == domain.committed_items
            && domain.expected_items == domain.adapter_items));
    make_tree_writable(&work_root);
}

#[test]
fn storage_verify_hash_manifest_contains_only_importer_inventory() {
    let temp = tempfile::tempdir().expect("test root");
    let copy_root = temp.path().join("copy");
    let work_root = temp.path().join("work");
    fs::create_dir_all(&copy_root).expect("copy root");
    copied_root(&copy_root);

    let report = verify_copied_state(&copy_root, &work_root).expect("verification report");
    let paths: Vec<_> = report
        .hashes_before
        .files
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();

    assert!(!paths.iter().any(|path| path.starts_with("secrets/")));
    assert!(!paths.iter().any(|path| path.starts_with("mesh/")));
    assert!(!paths.iter().any(|path| *path == "config.json"));
    make_tree_writable(&work_root);
}

#[test]
fn storage_verify_rejects_overlapping_copy_and_work_roots() {
    let temp = tempfile::tempdir().expect("test root");
    let copy_root = temp.path().join("copy");
    fs::create_dir_all(&copy_root).expect("copy root");

    let error = verify_copied_state(&copy_root, &copy_root.join("work"))
        .expect_err("overlapping roots must be refused");

    assert!(error.contains("must be disjoint"));
    assert!(!copy_root.join("work").exists());
}

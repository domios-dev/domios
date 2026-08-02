use super::*;

#[test]
fn role_seeds_are_created_once_and_nested_utf8_is_exact() {
    let temp = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(temp.path(), "tab-full-123", AgentPadRole::Worker).unwrap();
    assert!(pad.path().join("message.md").is_file());
    assert!(pad.path().join("notes.md").is_file());
    assert!(pad.path().join("report.md").is_file());
    fs::remove_file(pad.path().join("notes.md")).unwrap();
    fs::create_dir(pad.path().join("nested")).unwrap();
    fs::write(pad.path().join("nested/report.md"), "Привіт\n`$`\n").unwrap();
    assert_eq!(pad.read_utf8(Path::new("nested/report.md")).unwrap(), "Привіт\n`$`\n");
    assert!(!pad.path().join("notes.md").exists());
}

#[test]
fn unsafe_paths_and_sibling_access_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(temp.path(), "tab-a", AgentPadRole::General).unwrap();
    let sibling = AgentPad::create(temp.path(), "tab-b", AgentPadRole::General).unwrap();
    fs::write(sibling.path().join("secret.md"), "secret").unwrap();
    assert!(pad.read_utf8(Path::new("../tab-b/secret.md")).is_err());
    assert!(pad.read_utf8(pad.path()).is_err());
    assert!(pad.read_utf8(Path::new("notes.md/more")).is_err());
}

#[test]
fn revoke_is_exact_and_reconciliation_preserves_only_live_tabs() {
    let temp = tempfile::tempdir().unwrap();
    let live = AgentPad::create(temp.path(), "tab-live", AgentPadRole::Planner).unwrap();
    let gone = AgentPad::create(temp.path(), "tab-gone", AgentPadRole::Orchestrator).unwrap();
    gone.revoke().unwrap();
    assert!(!temp.path().join(PAD_SUBDIR).join("tab-gone").exists());
    let orphan = AgentPad::create(temp.path(), "tab-orphan", AgentPadRole::General).unwrap();
    drop(orphan);
    reconcile(temp.path(), &HashSet::from(["tab-live".to_string()])).unwrap();
    assert!(live.path().is_dir());
    assert!(!temp.path().join(PAD_SUBDIR).join("tab-orphan").exists());
    revoke(&temp.path().join(PAD_SUBDIR), "tab-live").unwrap();
    revoke(&temp.path().join(PAD_SUBDIR), "tab-live").unwrap();
}

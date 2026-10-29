use super::*;

#[test]
fn container_marker_refuses_tag_managed_updates() {
    let error = marker_refusal(true).unwrap();
    assert_eq!(error.code, ErrorCode::ContainerManagedUpdate);
    assert!(error.message.contains("Pull a newer image tag"));
    assert_eq!(
        serde_json::to_value(error).unwrap()["code"],
        "container_managed_update"
    );
}

#[test]
fn container_marker_absent_preserves_host_update_policy() {
    assert_eq!(marker_refusal(false), None);
}

#[test]
fn container_marker_is_resolved_from_the_release_bundle_for_both_binaries() {
    let root = tempfile::tempdir().unwrap();
    let server = root.path().join("domios-server/domios-server");
    let cli = root.path().join("cli/codeterm");
    assert!(!codeterm_paths::container_install::marked_at(&server));
    std::fs::write(
        root.path().join(codeterm_paths::container_install::MARKER),
        b"",
    )
    .unwrap();
    assert!(codeterm_paths::container_install::marked_at(&server));
    assert!(codeterm_paths::container_install::marked_at(&cli));
    assert!(!codeterm_paths::container_install::marked_at(
        &root.path().join("unrelated")
    ));
}

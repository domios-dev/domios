use super::*;

#[test]
fn legacy_peer_http_200_updating_triggers_status_observation_and_catches_attachment_error() {
    let legacy_install = serde_json::json!({
        "status": "updating",
        "message": "Checking for the latest version. App will restart if an update is available."
    });
    assert_eq!(
        classify_mesh_install_response(&legacy_install, Some("1.23.3")),
        MeshInstallOutcome::ObserveStatus {
            after_phase: "checking".into()
        }
    );

    let failed_status = serde_json::json!({
        "phase": "error",
        "last_error": "the server attachment is unavailable",
        "current_version": "1.23.0"
    });
    let outcome = classify_mesh_status_response(&failed_status, Some("1.23.3"));
    match outcome {
        MeshInstallOutcome::Failed(failure) => {
            assert_eq!(
                failure.code,
                InstallFailureCode::ServerAttachmentUnavailable
            );
            assert!(failure.recovery.contains("Relaunch"));
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn confirmed_relaunching_handoff_is_never_reported_as_verified_success() {
    let handoff = serde_json::json!({
        "status": "updating",
        "handoff": "relaunching",
        "message": "Installer handoff started; replacement version is not yet verified."
    });
    assert!(matches!(
        classify_mesh_install_response(&handoff, Some("1.23.3")),
        MeshInstallOutcome::RestartingUnverified { .. }
    ));
}

#[test]
fn verified_peer_status_and_up_to_date_report_clean_outcomes() {
    assert_eq!(
        classify_mesh_install_response(&serde_json::json!({"status": "up_to_date"}), None),
        MeshInstallOutcome::UpToDate
    );
    let verified = serde_json::json!({
        "phase": "success",
        "current_version": "1.23.3",
        "native_update": {
            "phase": {
                "phase": "verified",
                "payload": { "version": "1.23.3" }
            }
        }
    });
    assert_eq!(
        classify_mesh_status_response(&verified, Some("1.23.3")),
        MeshInstallOutcome::Verified {
            version: "1.23.3".into()
        }
    );
    let unverified_success = serde_json::json!({
        "phase": "success",
        "current_version": "1.23.0"
    });
    assert!(matches!(
        classify_mesh_status_response(&unverified_success, Some("1.23.3")),
        MeshInstallOutcome::RestartingUnverified { .. }
    ));
}

#[test]
fn peer_idle_without_no_update_receipt_is_a_failure() {
    let idle = serde_json::json!({ "phase": "idle", "current_version": "1.23.0" });
    assert!(matches!(
        classify_mesh_status_response(&idle, Some("1.23.3")),
        MeshInstallOutcome::Failed(_)
    ));
    let no_update = serde_json::json!({
        "phase": "idle",
        "current_version": "1.23.3",
        "native_update": { "phase": { "phase": "no_update" } }
    });
    assert_eq!(
        classify_mesh_status_response(&no_update, Some("1.23.3")),
        MeshInstallOutcome::UpToDate
    );
}

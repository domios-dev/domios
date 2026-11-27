use super::*;

#[test]
fn server_attachment_loss_maps_to_typed_relaunch_failure_with_conflict_status() {
    for raw in [
        "the server attachment is unavailable",
        "the native host is unavailable",
        "the native updater attachment is unavailable",
        "the attached server process identity changed",
        "native update refused: NotAttached",
    ] {
        let failure = InstallHandoffFailure::classify(raw);
        assert_eq!(
            failure.code,
            InstallFailureCode::ServerAttachmentUnavailable
        );
        assert_eq!(failure.code.as_str(), "server_attachment_unavailable");
        assert_eq!(failure.code.http_status(), 409);
        assert!(failure.recovery.contains("Relaunch"));
        assert_eq!(failure.detail.as_deref(), Some(raw));
        let wire = failure.to_wire_string();
        assert_eq!(
            InstallHandoffFailure::from_wire(&wire),
            Some(failure.clone())
        );
        assert_eq!(InstallHandoffFailure::classify(&wire), failure);
    }
}

#[test]
fn admission_and_holder_and_network_failures_keep_distinct_codes_and_statuses() {
    let admission =
        InstallHandoffFailure::classify(&NativeUpdateAdmissionRefusal::IncarnationLimit.error());
    assert_eq!(admission.code, InstallFailureCode::UpdateAdmissionRefused);
    assert_eq!(admission.code.http_status(), 409);
    assert!(admission.recovery.contains("Relaunch"));
    assert_eq!(
        InstallHandoffFailure::from_wire(&admission.to_wire_string()),
        Some(admission)
    );

    let uncertain = InstallHandoffFailure::classify(
        "native update outcome uncertain; execution will not be replayed: DisconnectedAfterDispatch",
    );
    assert_eq!(uncertain.code, InstallFailureCode::UpdateOutcomeUncertain);
    assert_eq!(
        InstallHandoffFailure::from_wire(&uncertain.to_wire_string()),
        Some(uncertain)
    );

    let holders = InstallHandoffFailure::classify(
        "Update blocked by busy tabs: Build\n[update-blocking-tabs][]",
    );
    assert_eq!(holders.code, InstallFailureCode::InstallFileHolders);
    assert_eq!(holders.code.http_status(), 409);
    assert_eq!(holders.message, "Update blocked by busy tabs: Build");

    let download =
        InstallHandoffFailure::classify("GET https://example.invalid download/verify failed: 502");
    assert_eq!(download.code, InstallFailureCode::DownloadOrSignature);
    assert_eq!(download.code.http_status(), 502);

    let fallback = InstallHandoffFailure::classify("opaque handoff failure");
    assert_eq!(fallback.code, InstallFailureCode::InstallHandoffFailed);
    assert_eq!(fallback.code.http_status(), 500);
    assert_eq!(fallback.message, "opaque handoff failure");
    assert_eq!(fallback.recovery, RELAUNCH_RECOVERY);
}

#[test]
fn api_json_is_explicitly_non_ok_and_carries_code_message_and_recovery() {
    let json =
        InstallHandoffFailure::classify("the server attachment is unavailable").to_api_json();
    assert_eq!(json["ok"], false);
    assert_eq!(json["kind"], INSTALL_HANDOFF_FAILED_KIND);
    assert_eq!(json["code"], "server_attachment_unavailable");
    assert_eq!(json["recovery"], RELAUNCH_RECOVERY);
    assert_eq!(json["hint"], RELAUNCH_RECOVERY);
}

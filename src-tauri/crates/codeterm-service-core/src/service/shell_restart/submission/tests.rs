use super::*;

#[test]
fn submission_returns_with_queue_evidence_without_command_completion() {
    let receipt = ShellCommandSubmission::attempt("new-pty", |queued| {
        queued.set(37);
        Ok(())
    })
    .unwrap();
    assert!(receipt.submitted());
    assert_eq!(
        serde_json::to_value(receipt).unwrap(),
        serde_json::json!({
            "status": "submitted", "pty_id": "new-pty", "bytes_written": true,
            "bytes_queued": 37, "evidence": "shell_command_start", "detail": null
        })
    );
}

#[test]
fn postwrite_errors_are_uncertainty_and_never_replay_the_payload() {
    for error in [
        "submit key refused",
        "PTY write acknowledgement timed out",
        "PTY closed",
    ] {
        let calls = Cell::new(0);
        let receipt = ShellCommandSubmission::attempt("exact-pty", |queued| {
            calls.set(calls.get() + 1);
            queued.set(29);
            Err(error.into())
        })
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert!(!receipt.submitted());
        let value = serde_json::to_value(receipt).unwrap();
        assert_eq!(value["status"], "submit_unconfirmed");
        assert_eq!(value["bytes_written"], true);
        assert_eq!(value["detail"], error);
    }
}

#[test]
fn ease_created_shell_retains_tab_and_queue_receipt_after_unconfirmed_submit() {
    let receipt = ShellCommandSubmission::attempt("new-pty", |queued| {
        queued.set(71);
        Err("submit_unconfirmed: no early command-start mark".into())
    }).unwrap();
    let handle = crate::service::spawn::TabHandle {
        tab_id: "created-tab".into(), pty_id: Some("new-pty".into()), workspace_id: "workspace".into(),
        description: None, worker_id: None, warning: None, model_fallback: None, budget_notice: None,
        command_submission: Some(receipt),
    };
    let response = serde_json::to_value(handle).unwrap();
    assert_eq!(response["tab_id"], "created-tab");
    assert_eq!(response["command_submission"]["status"], "submit_unconfirmed");
    assert_eq!(response["command_submission"]["bytes_written"], true);
    assert_eq!(response["command_submission"]["evidence"], "pty_write_queue");
}

#[test]
fn proven_non_submission_is_a_typed_refusal() {
    let result = ShellCommandSubmission::attempt("absent", |_| Err("PTY not found".into()));
    assert_eq!(
        result.unwrap_err(),
        "shell_command_not_submitted: PTY not found"
    );
}

#[test]
fn success_without_queue_evidence_is_refused() {
    assert!(ShellCommandSubmission::attempt("absent", |_| Ok(()))
        .unwrap_err()
        .starts_with("shell_command_not_submitted:"));
}

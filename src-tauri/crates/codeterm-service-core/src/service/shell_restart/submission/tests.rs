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
            "bytes_queued": 37, "evidence": "pty_write_queue", "detail": null
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

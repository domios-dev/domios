use super::cron_completion_params;

#[test]
fn cron_complete_carries_the_current_tab_binding() {
    assert_eq!(
        cron_completion_params(
            "run-token",
            Some("failed"),
            Some("summary-token"),
            Some("tab-token"),
            Some("nonce-token"),
            false,
        ),
        serde_json::json!({
            "run_id": "run-token",
            "status": "failed",
            "summary": "summary-token",
            "caller_tab_id": "tab-token",
            "session_binding_nonce": "nonce-token",
            "human_confirmed": false,
        })
    );
}

#[test]
fn cron_complete_without_tab_identity_is_unconfirmed_by_default() {
    assert_eq!(
        cron_completion_params("run-token", None, None, None, Some("unused"), false),
        serde_json::json!({ "run_id": "run-token", "human_confirmed": false })
    );
}

#[test]
fn cron_complete_carries_explicit_human_confirmation() {
    assert_eq!(
        cron_completion_params("run-token", None, None, None, None, true),
        serde_json::json!({ "run_id": "run-token", "human_confirmed": true })
    );
}

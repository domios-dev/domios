use super::{
    check_item_line, classify_continuation, continuation_withdraw_action,
    resolve_local_install_continuation, session_save_action, ContinuationRegistration,
    DEFAULT_LOCAL_INSTALL_CONTINUE_PROMPT,
};

#[test]
fn continuation_registration_reads_the_daemon_answer() {
    let registered = serde_json::json!({"tab_id": "tab-1", "registered": true});
    let refused = serde_json::json!({"tab_id": "tab-1", "registered": false, "reason": "opaque"});

    assert_eq!(
        classify_continuation(Some("tab-1"), &registered),
        ContinuationRegistration::Registered
    );
    assert_eq!(
        classify_continuation(Some("tab-1"), &refused),
        ContinuationRegistration::Refused("opaque".into())
    );
    assert_eq!(
        classify_continuation(Some("tab-1"), &serde_json::Value::Null),
        ContinuationRegistration::Unsupported
    );
    assert_eq!(
        classify_continuation(None, &serde_json::Value::Null),
        ContinuationRegistration::NotRequested
    );
}

#[test]
fn local_install_defaults_to_resuming_the_calling_tab() {
    let (prompt, tab) =
        resolve_local_install_continuation(false, None, None, Some("tab-1".to_string()));

    assert_eq!(tab.as_deref(), Some("tab-1"));
    assert_eq!(
        prompt.as_deref(),
        Some(DEFAULT_LOCAL_INSTALL_CONTINUE_PROMPT)
    );
}

#[test]
fn local_install_continuation_can_be_disabled_explicitly() {
    let (prompt, tab) = resolve_local_install_continuation(
        true,
        None,
        Some("tab-1".to_string()),
        Some("tab-2".to_string()),
    );

    assert!(prompt.is_none());
    assert!(tab.is_none());
}

#[test]
fn auto_continue_tab_must_match_the_caller() {
    assert_eq!(
        super::validate_auto_continue_tab(true, Some("tab-b"), Some("tab-a")),
        Err("--continue-tab must match the authenticated caller tab")
    );
    assert!(super::validate_auto_continue_tab(true, Some("tab-a"), Some("tab-a")).is_ok());
    assert!(super::validate_auto_continue_tab(true, None, Some("tab-a")).is_ok());
}

#[test]
fn no_continue_saves_a_caller_suppression_action() {
    let action = session_save_action(None, Some("tab-1"), true).unwrap();

    assert_eq!(
        action,
        serde_json::json!({ "suppress_continue_tab": "tab-1" })
    );
    assert_eq!(
        session_save_action(None, None, true),
        None,
        "no caller has no mark to suppress"
    );
}

#[test]
fn no_continue_requires_a_daemon_suppression_result() {
    assert!(super::suppression_result(&serde_json::json!({
        "suppression": {"tab_id": "tab-1", "suppressed": true}
    }))
    .is_ok());
    assert!(super::suppression_result(&serde_json::json!({
        "suppression": {"tab_id": "tab-1", "suppressed": false}
    }))
    .is_ok());
    assert_eq!(
        super::suppression_result(&serde_json::json!({
            "suppression": {"tab_id": "tab-1", "suppressed": false, "reason": "disabled"}
        })),
        Err("caller continuation was not suppressed: disabled".into())
    );
    assert!(super::suppression_result(&serde_json::json!({ "ok": true })).is_err());
}

#[test]
fn failed_install_can_withdraw_only_its_registered_prompt() {
    let action = continuation_withdraw_action(Some("opaque-prompt"), Some("tab-1")).unwrap();

    assert_eq!(
        action,
        serde_json::json!({
            "withdraw_continue_tab": "tab-1",
            "withdraw_continue_prompt": "opaque-prompt",
        })
    );
    assert_eq!(continuation_withdraw_action(None, None), None);
}

#[test]
fn install_helper_failure_withdraws_the_continuation() {
    use std::cell::Cell;

    let withdrawals = Cell::new(0);
    let result = super::install_with_withdrawal(
        || Err("helper failed".into()),
        || withdrawals.set(withdrawals.get() + 1),
    );

    assert_eq!(result, Err("helper failed".into()));
    assert_eq!(withdrawals.get(), 1);
}

#[test]
fn successful_install_does_not_withdraw_the_continuation() {
    use std::cell::Cell;

    let withdrawals = Cell::new(0);
    let result =
        super::install_with_withdrawal(|| Ok(()), || withdrawals.set(withdrawals.get() + 1));

    assert_eq!(result, Ok(()));
    assert_eq!(withdrawals.get(), 0);
}

#[test]
fn install_refusal_decoding_preserves_code_message_and_status() {
    let error = crate::cmd::print::CliError::http(
        503,
        r#"{"code":"native_capability_unavailable","message":"Attach the desktop host or install from file."}"#,
    );
    assert_eq!(
        error.message,
        "HTTP 503: native_capability_unavailable: Attach the desktop host or install from file."
    );
    let data = error.data.unwrap();
    assert_eq!(data["code"], "native_capability_unavailable");
    assert_eq!(data["http_status"], 503);
    assert_eq!(
        crate::cmd::print::CliError::http(503, "").message,
        "HTTP 503"
    );
    assert_eq!(
        crate::cmd::print::CliError::http(502, "upstream offline").message,
        "HTTP 502: upstream offline"
    );
}

#[test]
fn install_refusal_is_printed_in_human_and_json_modes() {
    let error = crate::cmd::print::CliError::http(
        503,
        r#"{"code":"native_capability_unavailable","message":"Install the official file."}"#,
    );
    for json in [false, true] {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        crate::cmd::print::write_cli_error(json, &error, &mut stdout, &mut stderr);
        if json {
            assert!(stderr.is_empty());
            let envelope: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
            assert_eq!(envelope["ok"], false);
            assert_eq!(
                envelope["error"]["message"],
                "HTTP 503: native_capability_unavailable: Install the official file."
            );
        } else {
            assert!(stdout.is_empty());
            let text = String::from_utf8(stderr).unwrap();
            assert!(text.contains("native_capability_unavailable: Install the official file."));
        }
    }
}

#[test]
fn failed_checks_never_print_unknown_up_to_date() {
    let item = serde_json::json!({"id":"app", "kind":"app", "installed":"1.17.0", "latest":null, "state":"up_to_date"});
    let errors = serde_json::json!({"app":"GitHub rate limit, retry at 14:30"});
    assert_eq!(
        check_item_line(&item, &errors),
        "app: check failed: GitHub rate limit, retry at 14:30"
    );
    let cached = serde_json::json!({"id":"app", "kind":"app", "installed":"1.17.0", "latest":"1.17.1", "state":"update_available", "reason":"cached_manifest"});
    assert_eq!(
        check_item_line(&cached, &errors),
        "app: check failed: GitHub rate limit, retry at 14:30 (cached update usable)"
    );
    assert_eq!(
        check_item_line(&cached, &serde_json::json!({})),
        "app: 1.17.0 → 1.17.1 (update_available)"
    );
}

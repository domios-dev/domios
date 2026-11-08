use super::{
    check_item_line, check_lines, classify_continuation, continuation_withdraw_action,
    resolve_local_install_continuation, session_save_action, ContinuationRegistration,
    DEFAULT_LOCAL_INSTALL_CONTINUE_PROMPT,
};

#[test]
fn container_update_refusal_preserves_typed_data_and_tag_recovery() {
    assert_eq!(super::container_update_error(false), None);
    let error = super::container_update_error(true).unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    crate::cmd::print::write_cli_error(true, &error, &mut stdout, &mut stderr);
    let body: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(body["ok"], false);
    assert!(String::from_utf8(stdout)
        .unwrap()
        .contains("container_managed_update"));
    assert!(body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Pull a newer image tag"));
    assert!(stderr.is_empty());
}

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
    let message = "Use the desktop app, or domios update install --from-file <official installer>.";
    let body = serde_json::json!({"code": "native_capability_unavailable", "message": message});
    let error = crate::cmd::print::CliError::http(409, &body.to_string());
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
                format!("HTTP 409: native_capability_unavailable: {message}")
            );
            assert_eq!(envelope["result"]["code"], "native_capability_unavailable");
            assert_eq!(envelope["result"]["message"], message);
        } else {
            assert!(stdout.is_empty());
            let text = String::from_utf8(stderr).unwrap();
            assert!(text.contains("native_capability_unavailable"));
            assert!(text.contains(message));
        }
    }
}

fn item(state: &str, reason: &str, installed: &str, latest: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "app", "kind": "app", "installed": installed, "latest": latest,
        "state": state, "reason": reason,
    })
}

#[test]
fn an_arrow_appears_only_for_a_strictly_newer_target() {
    for (state, words) in [
        ("update_available", "update available"),
        ("ready_to_apply", "ready to apply"),
    ] {
        assert_eq!(
            check_item_line(&item(state, "newer_version", "1.19.0", "1.19.1")),
            format!("app: 1.19.0 → 1.19.1 ({words})")
        );
    }
    assert_eq!(
        check_item_line(&item("up_to_date", "same_version", "1.19.1", "1.19.1")),
        "app: 1.19.1 (up to date)"
    );
    assert_eq!(
        check_item_line(&item("up_to_date", "local_ahead", "1.19.1", "1.19.0")),
        "app: 1.19.1 (up to date)"
    );
    for latest in ["1.19.1", "1.19.0", "not-a-version"] {
        let line = check_item_line(&item("update_available", "newer_version", "1.19.1", latest));
        assert_eq!(line, "app: 1.19.1", "a same, older or unreadable target");
    }
}

#[test]
fn internal_codes_are_never_printed() {
    assert_eq!(
        check_item_line(&item("core_silent", "same_version", "1.15.3", "1.15.3")),
        "app: 1.15.3 (updates automatically)"
    );
    assert_eq!(
        check_item_line(&item("core_silent", "newer_version", "1.15.3", "1.16.0")),
        "app: 1.15.3 (updates automatically)",
        "a core plugin never offers a user update"
    );
    assert_eq!(
        check_item_line(&item("incompatible", "incompatible", "1.0.0", "2.0.0")),
        "app: 1.0.0 (no compatible update)"
    );
    assert_eq!(
        check_item_line(&item("up_to_date", "local_install", "1.0.0", "1.0.0")),
        "app: 1.0.0 (installed locally)"
    );
    for (state, reason) in [
        ("up_to_date", "channel_unavailable"),
        ("a_future_state", "a_future_reason"),
    ] {
        assert_eq!(
            check_item_line(&item(state, reason, "1.0.0", "1.0.0")),
            "app: 1.0.0",
            "{state}/{reason} vouches for nothing"
        );
    }
}

#[test]
fn failed_checks_never_print_unknown_up_to_date() {
    let never_checked = serde_json::json!({"id": "app", "kind": "app", "installed": "1.17.0", "latest": null, "state": "up_to_date", "reason": "not_checked"});
    assert_eq!(check_item_line(&never_checked), "app: 1.17.0");
    assert_eq!(
        check_item_line(&item("up_to_date", "cached_check", "1.17.0", "1.17.0")),
        "app: 1.17.0"
    );
    assert_eq!(
        check_item_line(&item(
            "update_available",
            "cached_manifest",
            "1.17.0",
            "1.17.1"
        )),
        "app: 1.17.0 → 1.17.1 (update available)",
        "a cached update stays usable"
    );
}

#[test]
fn an_item_without_an_installed_version_prints_no_placeholder() {
    let unknown = serde_json::json!({"id": "lark", "installed": null, "latest": null, "state": "up_to_date", "reason": "missing_version"});
    assert_eq!(check_item_line(&unknown), "lark");
}

#[test]
fn an_absent_frontend_release_prints_as_up_to_date_with_no_diagnostic() {
    let snapshot = serde_json::json!({
        "items": [{"id": "frontend", "kind": "frontend", "installed": "1.19.1", "latest": "1.19.1", "state": "up_to_date", "reason": "same_version"}],
        "errors": {},
        "served_by": {},
    });
    for verbose in [false, true] {
        assert_eq!(
            check_lines(&snapshot, verbose),
            ["frontend: 1.19.1 (up to date)"]
        );
    }
}

#[test]
fn a_failed_check_stays_quiet_until_verbose() {
    let failure = "direct download: HTTP 504; GitHub API fallback: GitHub API returned HTTP 403";
    let snapshot = serde_json::json!({
        "items": [
            {"id": "app", "kind": "app", "installed": "1.17.0", "latest": null, "state": "up_to_date", "reason": "not_checked"},
            {"id": "lark", "kind": "plugin", "installed": "0.1.0", "latest": "0.2.0", "state": "update_available", "reason": "cached_manifest"},
        ],
        "errors": {"app": failure},
        "served_by": {"frontend": "github_api"},
    });
    let quiet = ["app: 1.17.0", "lark: 0.1.0 → 0.2.0 (update available)"];
    assert_eq!(check_lines(&snapshot, false), quiet);
    let failed = format!("app: check failed: {failure}");
    assert_eq!(
        check_lines(&snapshot, true),
        [
            quiet[0],
            quiet[1],
            "frontend: served by the GitHub Releases API",
            failed.as_str(),
        ]
    );
}

#[test]
fn verbose_names_the_path_in_plain_words() {
    let snapshot = serde_json::json!({
        "items": [],
        "served_by": {"app": "direct", "frontend": "a_future_path"},
    });
    assert_eq!(
        check_lines(&snapshot, true),
        [
            "app: served by direct download",
            "frontend: served by another path"
        ]
    );
}

#[test]
fn updater_admission_errors_render_recovery_copy_without_protocol_noise() {
    use codeterm_types::native_update::apply::NativeUpdateAdmissionRefusal as Refusal;
    for reason in [
        Refusal::OperationConflict,
        Refusal::RetiredOperation,
        Refusal::IncarnationLimit,
        Refusal::StaleServerIncarnation,
    ] {
        assert_eq!(
            super::update_failure_message(&reason.error()),
            reason.message()
        );
    }
    assert_eq!(
        super::update_failure_message("download failure"),
        "download failure"
    );
}

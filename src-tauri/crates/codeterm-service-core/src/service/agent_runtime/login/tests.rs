use super::*;
#[test]
fn completion_requires_exact_pending_login_and_canceled_cannot_resume() {
    let pending = RuntimeLoginState {
        instructions: None,
        code_input: None,
        status: RuntimeLoginStatus::Pending,
        login_id: Some("own".into()),
        method: Some(RuntimeLoginMethod::Browser),
        url: None,
        user_code: None,
        expires_at_ms: None,
        error: None,
    };
    let event = RuntimeLoginState {
        status: RuntimeLoginStatus::Completed,
        ..pending.clone()
    };
    assert!(matching_completion(Some(&pending), &event));
    assert!(!matching_completion(
        Some(&pending),
        &RuntimeLoginState {
            login_id: Some("foreign".into()),
            ..event.clone()
        }
    ));
    assert!(!matching_completion(
        Some(&RuntimeLoginState {
            status: RuntimeLoginStatus::Canceled,
            ..pending
        }),
        &event
    ));
    assert!(!matching_completion(None, &event));
}

#[test]
fn login_projection_removes_expired_and_finished_values() {
    let pending: RuntimeLoginState = serde_json::from_value(json!({"status":"pending","loginId":"opaque-id","url":"https://example.invalid/opaque-path","userCode":"opaque-token","expiresAtMs":5000,"instructions":"Opaque challenge instructions","codeInput":true})).unwrap();
    let current = serde_json::to_value(project(Some(&pending), 4999)).unwrap();
    assert_eq!(current["url"], "https://example.invalid/opaque-path");
    assert_eq!(current["userCode"], "opaque-token");
    assert_eq!(current["expiresAtMs"], 5000);
    assert_eq!(current["instructions"], "Opaque challenge instructions");
    assert_eq!(current["codeInput"], true);
    for state in [
        pending.clone(),
        RuntimeLoginState {
            status: RuntimeLoginStatus::Completed,
            ..pending.clone()
        },
        RuntimeLoginState {
            status: RuntimeLoginStatus::Canceled,
            ..pending
        },
    ] {
        let hidden = serde_json::to_value(project(Some(&state), 5000)).unwrap();
        assert!(hidden.get("url").is_none());
        assert!(hidden.get("userCode").is_none());
        assert!(hidden.get("instructions").is_none());
        assert!(hidden.get("codeInput").is_none());
    }
    assert!(project(None, 0).is_none());
}

#[test]
fn only_a_live_pending_sign_in_blocks_another_and_names_its_tab() {
    let pending: RuntimeLoginState =
        serde_json::from_value(json!({"status":"pending","loginId":"opaque","expiresAtMs":5000}))
            .unwrap();
    let refusal = claim_refusal(Some("other"), Some(&pending), "own", 4999).unwrap();
    assert!(refusal.contains("tab other"));
    assert!(claim_refusal(Some("own"), Some(&pending), "own", 4999)
        .unwrap()
        .contains("this tab"));
    let starting = RuntimeLoginState {
        expires_at_ms: None,
        ..pending.clone()
    };
    assert!(claim_refusal(Some("other"), Some(&starting), "own", u64::MAX).is_some());
    assert!(claim_refusal(Some("other"), Some(&pending), "own", 5000).is_none());
    for status in [
        RuntimeLoginStatus::Completed,
        RuntimeLoginStatus::Failed,
        RuntimeLoginStatus::Canceled,
    ] {
        let finished = RuntimeLoginState {
            status,
            ..pending.clone()
        };
        assert!(claim_refusal(Some("other"), Some(&finished), "own", 0).is_none());
    }
    assert!(claim_refusal(Some("other"), None, "own", 0).is_none());
    assert!(claim_refusal(None, Some(&pending), "own", 0).is_none());
}

#[test]
fn sign_in_failures_keep_the_real_reason_without_the_submitted_key() {
    let provider = codeterm_plugin::agent_runtime::RuntimeFailure::new(
        RuntimeFailureCode::ProviderRefused,
        "Sign-in failed: failed to start login server",
    );
    assert_eq!(
        run_failure_reason(&RuntimeFailure::Refused(provider.to_string())),
        "Sign-in failed: failed to start login server"
    );
    assert_eq!(
        run_failure_reason(&RuntimeFailure::Refused(
            "RPC method outside declared contract".into()
        )),
        "Sign-in failed: RPC method outside declared contract"
    );
    assert_eq!(
        run_failure_reason(&RuntimeFailure::Deadline),
        "Sign-in failed: Runtime did not respond in time"
    );
    assert_eq!(
        safe_reason(
            "Invalid key\n opaque-key-value  rejected",
            Some(" opaque-key-value ")
        ),
        "Invalid key <redacted> rejected"
    );
    assert_eq!(safe_reason("  ", None), "Sign-in failed. Try again.");
    let bounded = safe_reason(&"x".repeat(400), None);
    assert_eq!(bounded.chars().count(), 300);
    assert!(bounded.ends_with('…'));
}

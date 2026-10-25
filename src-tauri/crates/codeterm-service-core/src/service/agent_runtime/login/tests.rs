use super::*;
#[test]
fn completion_requires_exact_pending_login_and_canceled_cannot_resume() {
    let pending = RuntimeLoginState {
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
    let pending: RuntimeLoginState = serde_json::from_value(json!({"status":"pending","loginId":"opaque-id","url":"https://example.invalid/opaque-path","userCode":"opaque-token","expiresAtMs":5000})).unwrap();
    let current = serde_json::to_value(project(Some(&pending), 4999)).unwrap();
    assert_eq!(current["url"], "https://example.invalid/opaque-path");
    assert_eq!(current["userCode"], "opaque-token");
    assert_eq!(current["expiresAtMs"], 5000);
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
    }
    assert!(project(None, 0).is_none());
}

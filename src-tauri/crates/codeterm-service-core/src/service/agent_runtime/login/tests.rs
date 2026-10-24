use super::*;
#[test]
fn completion_requires_exact_pending_login_and_canceled_cannot_resume() {
    let pending = RuntimeLoginState {
        status: RuntimeLoginStatus::Pending,
        login_id: Some("own".into()),
        method: Some(RuntimeLoginMethod::Browser),
        url: None,
        user_code: None,
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

use super::{authenticated_plan_caller, caller_tab_header};
use axum::http::{HeaderMap, HeaderValue};

#[test]
fn tab_identity_readers_accept_domios_and_codeterm_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("X-Domios-Tab-Id", HeaderValue::from_static("new-tab"));
    headers.insert(
        "X-Domios-Session-Id",
        HeaderValue::from_static("new-session"),
    );
    headers.insert(
        "X-Domios-Binding-Nonce",
        HeaderValue::from_static("new-nonce"),
    );
    assert_eq!(caller_tab_header(&headers).as_deref(), Some("new-tab"));
    let caller = authenticated_plan_caller(&headers);
    assert_eq!(caller.session_id.as_deref(), Some("new-session"));
    assert_eq!(caller.binding_nonce.as_deref(), Some("new-nonce"));

    headers.clear();
    headers.insert("X-CodeTerm-Tab-Id", HeaderValue::from_static("old-tab"));
    headers.insert(
        "X-CodeTerm-Session-Id",
        HeaderValue::from_static("old-session"),
    );
    headers.insert(
        "X-CodeTerm-Binding-Nonce",
        HeaderValue::from_static("old-nonce"),
    );
    assert_eq!(caller_tab_header(&headers).as_deref(), Some("old-tab"));
    let caller = authenticated_plan_caller(&headers);
    assert_eq!(caller.session_id.as_deref(), Some("old-session"));
    assert_eq!(caller.binding_nonce.as_deref(), Some("old-nonce"));
}

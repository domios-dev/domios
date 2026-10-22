use super::*;
#[test]
fn human_confirmation_is_target_specific_and_cannot_override_a_tab() {
    let mut headers = HeaderMap::new();
    assert!(confirmed(&headers, "job").is_err());
    headers.insert(
        "x-codeterm-blueprint-confirmation",
        "other".parse().unwrap(),
    );
    assert!(confirmed(&headers, "job").is_err());
    headers.insert("x-codeterm-blueprint-confirmation", "job".parse().unwrap());
    assert!(confirmed(&headers, "job").is_ok());
    headers.insert("x-codeterm-tab-id", "worker".parse().unwrap());
    assert!(confirmed(&headers, "job").is_err());
}

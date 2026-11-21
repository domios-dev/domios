use super::*;

#[test]
fn dictation_requires_explicit_normalized_session_and_frontend_ids() {
    let value = "a2a9f17e-cb7c-4ba1-9a1b-7099ae215153";
    let mut headers = HeaderMap::new();
    assert!(client(&headers, value).is_err());
    headers.insert("x-codeterm-client-id", value.parse().unwrap());
    assert_eq!(client(&headers, value).unwrap(), value);
    assert!(client(&headers, "some-other-session").is_err());
    assert!(client(&headers, &value.to_ascii_uppercase()).is_err());
}

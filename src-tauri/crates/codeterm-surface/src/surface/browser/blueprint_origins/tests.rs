use super::*;

#[test]
fn origin_matching_checks_scheme_host_port_and_url_credentials() {
    let origins = vec!["https://example.com".into(), "http://localhost:7799".into()];
    for target in ["https://example.com/a?post=5", "http://localhost:7799/page"] {
        assert!(allows_page_origin(&origins, target), "{target}");
    }
    for target in [
        "http://example.com",
        "https://sub.example.com",
        "https://example.com:8443",
        "https://example.com.evil.invalid",
        "https://user@example.com",
        "file:///local",
    ] {
        assert!(!allows_page_origin(&origins, target), "{target}");
    }
}

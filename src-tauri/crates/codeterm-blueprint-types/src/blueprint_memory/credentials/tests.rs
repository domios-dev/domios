use super::contains_credentials;

#[test]
fn ordinary_words_and_unlabelled_identifiers_are_not_credentials() {
    for text in [
        "task-worker disk-space desk-layout",
        "Basic authentication and bearer authentication are supported.",
        "The akia substring occurs in words like takian.",
        "sk-short AKIAshort ghp_short",
        "xsk-0123456789abcdefghijklmnop",
        "xAKIA0123456789ABCDEF",
        "https://example.test/health?page=1",
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "VGhpcyBpcyBhIHB1YmxpYyBub24tc2VjcmV0IGV4YW1wbGU=",
    ] {
        assert!(
            !contains_credentials(text),
            "rejected ordinary text: {text}"
        );
    }
}

#[test]
fn credential_shapes_are_rejected() {
    for text in [
        "sk-0123456789abcdefghijklmnop",
        "sk-proj-0123456789abcdefghijklmnop",
        "AKIA0123456789ABCDEF",
        "ghp_0123456789abcdefghijklmnopqrstuvwxyz",
        "github_pat_0123456789abcdefghijklmnop",
        "Authorization: Bearer abcDEF0123456789",
        "Bearer abcDEF0123456789",
        "Basic dXNlcjpwYXNz",
        "https://user:pass@example.test/health",
        "https://user@example.test/health",
        "token=private-token",
        "api_key: 0123456789abcdef0123456789abcdef",
        "signing_key=0123456789abcdef0123456789abcdef",
        "secret=VGhpcyBpcyBhIHB1YmxpYyBub24tc2VjcmV0IGV4YW1wbGU=",
    ] {
        assert!(contains_credentials(text), "accepted credential: {text}");
    }
}

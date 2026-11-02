use codeterm_service_core::service::extract_revision;
#[test]
fn test_extract_revision_valid() {
    assert_eq!(extract_revision("some content\n<!-- revision: 10 -->"), 10);
}

#[test]
fn test_extract_revision_missing() {
    assert_eq!(extract_revision("no revision here"), 0);
}

#[test]
fn test_extract_revision_whitespace() {
    assert_eq!(extract_revision("<!-- revision:   42   -->"), 42);
}

#[test]
fn test_extract_revision_invalid_number() {
    assert_eq!(extract_revision("<!-- revision: abc -->"), 0);
}

use super::*;

#[test]
fn foreign_caller_reads_through_the_parent_authorization() {
    assert_eq!(reader_for("worker", None), TodoReader::Owner(None));
    assert_eq!(
        reader_for("worker", Some("worker".into())),
        TodoReader::Owner(Some("worker".into()))
    );
    assert_eq!(
        reader_for("worker", Some("orchestrator".into())),
        TodoReader::Parent("orchestrator".into())
    );
}

#[test]
fn service_refusals_keep_their_meaning_over_http() {
    assert_eq!(read_error_status("Tab not found: x"), StatusCode::NOT_FOUND);
    assert_eq!(
        read_error_status("only the owning Tab or user may read this checklist"),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        read_error_status("shared Storage is unavailable"),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

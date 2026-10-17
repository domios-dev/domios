use super::*;

#[test]
fn taskless_evidence_report_carries_launch_binding_without_inventing_session() {
    for session in [None, Some("own-session")] {
        let context = CallerContext {
            tab_id: Some("own-tab".into()),
            session_id: session.map(str::to_string),
            session_binding_nonce: Some("own-launch-nonce".into()),
            tab_title: None,
        };
        let body = context.report_body("own-tab", "evidence".into());
        assert_eq!(body["tab_id"], "own-tab");
        assert_eq!(body["caller_tab_id"], "own-tab");
        assert_eq!(body["session_binding_nonce"], "own-launch-nonce");
        assert_eq!(
            body.get("caller_session_id"),
            session.map(|value| json!(value)).as_ref()
        );
        assert!(body.get("session_id").is_none());
        assert_eq!(body["message"], "evidence");
    }
}

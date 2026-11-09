use super::*;

#[test]
fn exec_envelopes_omit_unreported_fields_and_keep_error_only_refusals() {
    assert_eq!(
        serde_json::json!(ExecResult {
            code: Some(0),
            pid: Some(42),
            ..Default::default()
        }),
        serde_json::json!({"code":0,"pid":42})
    );
    assert_eq!(
        serde_json::json!(ExecPollResult {
            done: None,
            result: ExecResult {
                error: Some("denied".into()),
                ..Default::default()
            }
        }),
        serde_json::json!({"error":"denied"})
    );
    assert_eq!(
        serde_json::json!(ExecPollResult {
            done: Some(false),
            result: ExecResult::default()
        }),
        serde_json::json!({"done":false})
    );
}

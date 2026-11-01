use super::review_verdict;
use serde_json::json;

#[test]
fn plugin_steps_agent_review_routes_only_closed_bounded_verdicts() {
    for verdict in ["pass", "retry"] {
        assert_eq!(
            review_verdict(&json!({"verdict":verdict,"notes":"review notes"})).unwrap(),
            verdict
        );
    }
    assert_eq!(
        review_verdict(&json!({"verdict":"choose","choice":0})).unwrap(),
        "choose"
    );
    for invalid in [
        json!("pass"),
        json!({"verdict":"execute"}),
        json!({"verdict":"choose"}),
        json!({"verdict":"choose","choice":256}),
        json!({"verdict":"choose","choice":-1}),
        json!({"verdict":"pass","choice":0}),
        json!({"verdict":"retry","command":"untrusted"}),
        json!({"verdict":"pass","notes":"x".repeat(4097)}),
    ] {
        assert!(review_verdict(&invalid).is_err());
    }
}

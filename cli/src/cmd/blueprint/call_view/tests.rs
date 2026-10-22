use super::*;
use serde_json::json;
#[test]
fn blueprint_v2_call_show_text_preserves_expanded_identity_body_result_and_nested_stop() {
    let value = json!({"expanded_calls":[{"node":"invoke","pin":{"id":"mem:fixed-helper","hash":"abc123"},"graph":{"steps":[{"id":"increment","run":{"lit":"fixed-author-command"}}],"result":{"from":"increment.out"}},"children":[]}],"nested_run":{"nodes":[{"step_name":"increment","state":"failed","result_summary":"bounded typed result","reason":"budget_exhausted","call_stack":[{"node":"invoke","id":"mem:fixed-helper","hash":"abc123"}]}]}});
    let text = text(&value);
    for expected in ["Call invoke -> mem:fixed-helper@abc123","fixed-author-command","increment.out","Nested increment [failed] result=bounded typed result stop=budget_exhausted stack=invoke:mem:fixed-helper@abc123"]{assert!(text.contains(expected),"{text}");}
}
#[test]
fn blueprint_v2_call_show_plain_job_has_no_invented_call_projection() {
    assert_eq!(text(&json!({"id":"plain"})), "");
}

#[test]
fn blueprint_v2_call_check_drift_names_pin_head_and_semantic_fields() {
    let rendered = drift(
        &json!({"call_drift":[{"pin":{"id":"mem:helper","hash":"pinned"},"head_hash":"edited","diff":{"changed":{"value":["code"]},"document_fields":["inputs"]}}]}),
    );
    for required in ["WARNING:", "mem:helper@pinned", "edited", "code", "inputs"] {
        assert!(rendered.contains(required), "{rendered}");
    }
}

#[test]
fn blueprint_v2_call_retained_show_names_missing_history_without_inventing_a_graph() {
    let output = text(
        &json!({"historical_graph_available":false,"graph_definition_version":null,"expanded_calls":[],"nested_run":{"nodes":[{"step_name":"invoke","state":"done"}]}}),
    );
    assert!(output.contains("Historical call graph unavailable"));
    assert!(output.contains("unknown"));
    assert!(output.contains("Nested invoke [done]"));
    assert!(!output.contains("Call invoke ->"));
}

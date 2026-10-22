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

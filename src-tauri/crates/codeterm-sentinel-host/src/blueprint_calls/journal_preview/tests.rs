use super::*;
use serde_json::json;

#[test]
fn typed_preview_preserves_the_existing_bounded_journal_wire() {
    let mut entry:BlueprintJournalEntry=serde_json::from_value(json!({"entry_id":1,"run_id":"run","step_index":0,"step_name":"step","state":"done","outcome":"done","started_at_ms":2,"ended_at_ms":3,"result_summary":null,"reason":null,"body_ref":null,"parent_step_index":0,"definition_version":1,"call_stack":[{"node":"call","id":"callee","hash":"hash"}]})).unwrap();
    for payload_size in [0, 140] {
        entry.evidence = (0..payload_size)
            .map(|index| {
                (
                    format!("a-{index:03}"),
                    json!({"text":"x".repeat(5000),"nested":[1,2]}),
                )
            })
            .collect();
        assert_eq!(json!(project(&entry)), bounded(&json!(entry), 0));
    }
}

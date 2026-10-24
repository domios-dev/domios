use super::*;

fn caller(index: usize) -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":"bounded","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"call","call":{"ref":format!("mem:{index}"),"pin":{"id":format!("mem:{index}"),"hash":"hash"}},"out":{"type":"integer"}}],"result":{"to":"notify"}})).unwrap()
}

#[test]
fn saved_solution_show_depth_budget_is_typed_and_stops_resolution() {
    let mut reads = 0;
    let graph = graph_resolved(&caller(0), |_| {
        reads += 1;
        Ok(caller(reads))
    })
    .unwrap();
    assert_eq!(reads, MAX_CALL_DEPTH);
    let mut entry = &graph.calls[0];
    for _ in 0..MAX_CALL_DEPTH {
        entry = &entry.children[0];
    }
    assert!(matches!(
        entry.unavailable,
        Some(UnavailableCall::BudgetExhausted)
    ));
    assert_eq!(entry.graph, Value::Null);
}

#[test]
fn saved_solution_show_byte_budget_is_typed_before_projection() {
    let mut oversized = caller(1);
    oversized.note = Some("x".repeat(MAX_CLOSURE_BYTES));
    let graph = graph_resolved(&caller(0), |_| Ok(oversized.clone())).unwrap();
    assert!(matches!(
        graph.calls[0].unavailable,
        Some(UnavailableCall::BudgetExhausted)
    ));
    assert!(graph.calls[0].children.is_empty());
}

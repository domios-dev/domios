use super::*;
use serde_json::json;

#[test]
fn rpc_rejects_unbounded_item_arrays_with_split_guidance() {
    let params = Some(json!({ "items": vec![json!({}); MAX_TAB_TODO_ITEMS + 1] }));
    let error = bounded_array::<Vec<TabTodoDraft>>(&params, "items").unwrap_err();
    assert_eq!(error.code, -32602);
    assert!(error.message.contains("propose a task split"));
}

#[test]
fn rpc_accepts_at_most_twelve_bounded_drafts() {
    let draft = json!({ "title": "A bounded todo" });
    let params = Some(json!({ "items": vec![draft; MAX_TAB_TODO_ITEMS] }));
    let items = bounded_array::<Vec<TabTodoDraft>>(&params, "items").expect("bounded list");
    assert_eq!(items.len(), MAX_TAB_TODO_ITEMS);
    assert_eq!(MAX_TAB_TODO_ITEMS, 12);
}

#[test]
fn rpc_state_accepts_blocked_and_skipped() {
    for (value, state) in [
        ("blocked", TabTodoState::Blocked),
        ("skipped", TabTodoState::Skipped),
    ] {
        assert_eq!(
            parse_state(&Some(json!({ "state": value }))).unwrap(),
            state
        );
    }
}

#[test]
fn rpc_write_accepts_an_empty_declaration_and_refuses_an_unbounded_one() {
    let empty = write_entries(&Some(json!({ "items": [] }))).expect("empty declaration");
    assert!(empty.is_empty());
    let entry = json!({ "content": "Step", "state": "in_progress" });
    let error = write_entries(&Some(
        json!({ "items": vec![entry; MAX_TAB_TODO_ITEMS + 1] }),
    ))
    .unwrap_err();
    assert!(error.message.contains("propose a task split"));
    assert!(write_entries(&Some(
        json!({ "items": [{ "content": "Step", "state": "open", "extra": 1 }] })
    ))
    .is_err());
}

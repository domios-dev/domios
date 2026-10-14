use super::*;

fn item(id: &str, running: bool, content: &str) -> ChatMsg {
    let mut row = ChatMsg::default();
    row.id = id.into();
    row.kind = "tool".into();
    row.content = content.into();
    row.tool_status = Some(if running { "running" } else { "completed" }.into());
    row
}

fn put(maps: &mut RuntimeMaps, epoch: u64, row: ChatMsg) {
    fold(
        maps,
        epoch,
        &RuntimeEvent::ItemUpdated {
            thread_id: "t".into(),
            item: row,
        },
    );
}

#[test]
fn an_item_update_replaces_the_row_with_the_same_id() {
    let mut maps = RuntimeMaps::default();
    put(&mut maps, 1, item("a", true, "one"));
    put(&mut maps, 1, item("a", false, "one two"));
    assert_eq!(maps.item_ids(), vec!["a".to_string()]);
    assert_eq!(maps.text_bytes(), "one two".len());
}

#[test]
fn the_item_cap_retires_closed_rows_oldest_first_and_keeps_running_ones() {
    let mut maps = RuntimeMaps::default();
    put(&mut maps, 1, item("running", true, ""));
    for index in 0..LIVE_ITEM_CAP + 10 {
        put(&mut maps, 1, item(&format!("done-{index}"), false, ""));
    }
    let ids = maps.item_ids();
    assert_eq!(ids.len(), LIVE_ITEM_CAP);
    assert!(ids.contains(&"running".to_string()));
    assert!(!ids.contains(&"done-0".to_string()));
    assert!(ids.contains(&format!("done-{}", LIVE_ITEM_CAP + 9)));
}

#[test]
fn the_byte_budget_shrinks_running_text_instead_of_failing() {
    let mut maps = RuntimeMaps::default();
    let big = "x".repeat(LIVE_ITEM_BYTES);
    put(&mut maps, 1, item("a", true, &big));
    put(&mut maps, 1, item("b", true, &big));
    assert!(maps.text_bytes() <= LIVE_ITEM_BYTES);
    assert_eq!(maps.item_ids().len(), 2);
}

#[test]
fn a_new_epoch_resets_the_maps() {
    let mut maps = RuntimeMaps::default();
    put(&mut maps, 1, item("a", false, "x"));
    put(&mut maps, 2, item("b", false, "y"));
    assert_eq!(maps.item_ids(), vec!["b".to_string()]);
}

#[test]
fn warnings_with_distinct_ids_never_merge() {
    let mut maps = RuntimeMaps::default();
    for id in ["w-1", "w-2"] {
        fold(
            &mut maps,
            1,
            &RuntimeEvent::Warning {
                id: id.into(),
                message: "m".into(),
            },
        );
    }
    assert_eq!(maps.item_ids().len(), 2);
}

#[test]
fn a_working_turn_becomes_active_and_idle_clears_it() {
    let mut context = RuntimeContext::default();
    let turn = |state| RuntimeEvent::TurnUpdated {
        thread_id: "t".into(),
        turn_id: Some("turn".into()),
        state,
        detail: None,
        status_line: None,
        started_at: None,
        completed_at: None,
        parent_thread_id: None,
    };
    observe(&mut context, &turn(RuntimeTurnState::Working));
    assert_eq!(context.active_turns.len(), 1);
    observe(&mut context, &turn(RuntimeTurnState::Idle));
    assert!(context.active_turns.is_empty());
}

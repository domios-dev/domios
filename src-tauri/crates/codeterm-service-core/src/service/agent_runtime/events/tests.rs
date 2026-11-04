use super::*;

fn item(id: &str, running: bool, content: &str) -> ChatMsg {
    let mut row = ChatMsg::default();
    row.id = id.into();
    row.kind = "tool".into();
    row.content = content.into();
    row.tool_status = Some(if running { "running" } else { "completed" }.into());
    row
}

fn item_ids(maps: &RuntimeMaps) -> Vec<String> {
    maps.items.iter().map(|item| item.item.id.clone()).collect()
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
    assert_eq!(item_ids(&maps), vec!["a".to_string()]);
    assert_eq!(text_total(&maps.items), "one two".len());
}

#[test]
fn the_item_cap_retires_closed_rows_oldest_first_and_keeps_running_ones() {
    let mut maps = RuntimeMaps::default();
    put(&mut maps, 1, item("running", true, ""));
    for index in 0..LIVE_ITEM_CAP + 10 {
        put(&mut maps, 1, item(&format!("done-{index}"), false, ""));
    }
    let ids = item_ids(&maps);
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
    assert!(text_total(&maps.items) <= LIVE_ITEM_BYTES);
    assert_eq!(item_ids(&maps).len(), 2);
}

#[test]
fn a_new_epoch_resets_the_maps() {
    let mut maps = RuntimeMaps::default();
    put(&mut maps, 1, item("a", false, "x"));
    put(&mut maps, 2, item("b", false, "y"));
    assert_eq!(item_ids(&maps), vec!["b".to_string()]);
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
    assert_eq!(item_ids(&maps).len(), 2);
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

#[test]
fn idle_thread_prunes_its_background_roster() {
    let mut maps = RuntimeMaps::default();
    fold(
        &mut maps,
        1,
        &RuntimeEvent::BackgroundProcessesUpdated {
            thread_id: "child".into(),
            processes: Vec::new(),
        },
    );
    assert!(maps.background_processes.contains_key("child"));
    fold(
        &mut maps,
        1,
        &RuntimeEvent::TurnUpdated {
            thread_id: "child".into(),
            turn_id: Some("turn".into()),
            state: RuntimeTurnState::Idle,
            detail: None,
            status_line: None,
            started_at: None,
            completed_at: None,
            parent_thread_id: Some("root".into()),
        },
    );
    assert!(!maps.background_processes.contains_key("child"));
}

#[test]
fn runtime_evidence_preserves_the_protocol_item_through_translation_and_fold() {
    let source = AgentRuntimeEvent::SendEvidence {
        thread_id: "own-thread".into(),
        send_id: "send".into(),
        turn_id: "shared-turn".into(),
        item_id: Some("user-item".into()),
    };
    let event = translate(&source);
    assert!(
        matches!(&event, RuntimeEvent::EvidenceUpdated { item_id, .. } if item_id.as_deref() == Some("user-item"))
    );
    let mut maps = RuntimeMaps::default();
    fold(&mut maps, 1, &event);
    assert_eq!(
        maps.take_evidence("send"),
        Some((
            "own-thread".into(),
            "shared-turn".into(),
            Some("user-item".into())
        ))
    );
    assert_eq!(maps.take_evidence("send"), None);
}

#[test]
fn runtime_evidence_without_item_identity_survives_translation_and_fold() {
    let source = AgentRuntimeEvent::SendEvidence {
        thread_id: "own-thread".into(),
        send_id: "send".into(),
        turn_id: "turn".into(),
        item_id: None,
    };
    let mut maps = RuntimeMaps::default();
    fold(&mut maps, 1, &translate(&source));
    assert_eq!(
        maps.take_evidence("send"),
        Some(("own-thread".into(), "turn".into(), None))
    );
}

#[test]
fn declared_notices_never_enter_the_runtime_item_cache() {
    let mut maps = RuntimeMaps::default();
    for message in ["first", "replacement"] {
        let event = translate(&AgentRuntimeEvent::Notice {
            thread_id: "own".into(),
            message: message.into(),
        });
        fold(&mut maps, 1, &event);
    }
    assert!(maps.items.is_empty());
}

#[test]
fn authentication_declarations_are_cleared_with_the_runtime_epoch() {
    let mut maps = RuntimeMaps::default();
    let authentication = codeterm_plugin::agent_runtime::RuntimeAuthentication {
        state: codeterm_plugin::agent_runtime::RuntimeAuthenticationState::Unknown,
        methods: vec![
            codeterm_plugin::agent_runtime::RuntimeAuthenticationMethod {
                id: "own-method".into(),
                name: "Sign in".into(),
                available: true,
            },
        ],
    };
    fold(
        &mut maps,
        1,
        &translate(&AgentRuntimeEvent::Authentication {
            authentication: authentication.clone(),
        }),
    );
    assert_eq!(maps.authentication, Some(authentication));
    maps.reset(2);
    assert!(maps.authentication.is_none());
}

#[test]
fn shared_long_stream_fixtures_fold_append_content_and_survive_replay() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../plugins/provider-acp/tests/fixtures/runtime");
    for name in [
        "long-stream-complete",
        "long-stream-cancel",
        "oversize-single-frame",
        "oversize-astral-cut",
    ] {
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(format!("{name}.json"))).unwrap(),
        )
        .unwrap();
        let events: Vec<AgentRuntimeEvent> =
            serde_json::from_value(fixture["expected"]["events"].clone()).unwrap();
        let mut maps = RuntimeMaps::default();
        for _ in 0..2 {
            for source in &events {
                let event = translate_with_items(source, &mut maps, false).unwrap();
                fold(&mut maps, 1, &event);
            }
            assert_eq!(maps.items.len(), 1);
            let content = &maps.items[0].item.content;
            if name.starts_with("oversize-") {
                assert!(content.contains("[ACP row truncated: oversized text update]"));
                assert!(content.ends_with("healthy"));
            } else {
                assert_eq!(*content, format!("{}🌍", "x".repeat(4092)).repeat(128));
                assert_eq!(content.len(), 512 * 1024);
            }
        }
        if name == "long-stream-cancel" {
            assert!(events.iter().any(|event| matches!(
                event,
                AgentRuntimeEvent::TurnState {
                    state: RuntimeTurnState::Idle,
                    detail: Some(RuntimeTurnDetail::Interrupted),
                    ..
                }
            )));
        }
    }
}

#[test]
fn append_requires_the_exact_thread_row_and_byte_offset() {
    let mut maps = RuntimeMaps::default();
    let mut row = item("row", false, "🌍");
    row.kind = "assistant".into();
    put(&mut maps, 1, row);
    for (thread, id, offset) in [("foreign", "row", 4), ("t", "missing", 4), ("t", "row", 1)] {
        let source = AgentRuntimeEvent::ChatItemAppend {
            thread_id: thread.into(),
            item_id: id.into(),
            content: "suffix".into(),
            offset,
        };
        let event = translate_with_items(&source, &mut maps, true).unwrap();
        assert!(matches!(event, RuntimeEvent::Notice { .. }));
        fold(&mut maps, 1, &event);
        assert_eq!(maps.items[0].item.content, "🌍");
    }
}

#[test]
fn settled_turn_appends_are_refused_once_per_row() {
    let mut maps = RuntimeMaps::default();
    let turn = |id: &str, state| {
        translate(&AgentRuntimeEvent::TurnState {
            thread_id: "t".into(),
            turn_id: Some(id.into()),
            state,
            detail: None,
            status_line: None,
            started_at: None,
            completed_at: None,
            parent_thread_id: None,
        })
    };
    fold(&mut maps, 1, &turn("first", RuntimeTurnState::Working));
    let mut row = item("row", false, "🌍");
    row.kind = "assistant".into();
    put(&mut maps, 1, row);
    fold(&mut maps, 1, &turn("first", RuntimeTurnState::Idle));
    fold(&mut maps, 1, &turn("next", RuntimeTurnState::Working));
    let append = AgentRuntimeEvent::ChatItemAppend {
        thread_id: "t".into(),
        item_id: "row".into(),
        content: "late".into(),
        offset: 4,
    };
    assert!(matches!(
        translate_with_items(&append, &mut maps, false),
        Some(RuntimeEvent::Notice { .. })
    ));
    for _ in 0..100 {
        assert!(translate_with_items(&append, &mut maps, false).is_none());
    }
    assert_eq!(maps.items[0].item.content, "🌍");
    assert!(matches!(
        translate_with_items(&append, &mut maps, true),
        Some(RuntimeEvent::ItemUpdated { .. })
    ));
}

#[test]
fn tool_patch_preserves_full_retained_card_and_sequence() {
    let mut maps = RuntimeMaps::default();
    let mut original = item("tool", true, "");
    original.tool_title = Some("Opaque tool".into());
    original.tool_input = Some(serde_json::json!({"command":"opaque"}));
    original.tool_result = Some("x".repeat(15900));
    put(&mut maps, 1, original);
    let patch = AgentRuntimeEvent::ChatItemPatch {
        thread_id: "t".into(),
        item: ChatMsg {
            id: "tool".into(),
            kind: "tool".into(),
            seq: 99,
            tool_status: Some("completed".into()),
            ..Default::default()
        },
    };
    let event = translate_with_items(&patch, &mut maps, false).unwrap();
    fold(&mut maps, 1, &event);
    let row = &maps.items[0].item;
    assert_eq!(row.seq, 0);
    assert_eq!(row.tool_title.as_deref(), Some("Opaque tool"));
    assert_eq!(
        row.tool_input,
        Some(serde_json::json!({"command":"opaque"}))
    );
    assert_eq!(row.tool_result.as_ref().unwrap().len(), 15900);
    assert_eq!(row.tool_status.as_deref(), Some("completed"));
}

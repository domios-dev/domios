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
    let root = crate::test_support::fixture_manifest_dir()
        .join("../../../plugins/provider-acp/tests/fixtures/runtime");
    for name in [
        "long-stream-complete",
        "long-stream-cancel",
        "oversize-single-frame",
        "oversize-astral-cut",
        "large-single-update",
        "large-multi-update",
        "batch-byte-limit",
        "batch-event-limit",
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
            if name.starts_with("batch-") {
                assert!(content.ends_with("healthy"));
                if name == "batch-byte-limit" {
                    assert!(content.contains("[ACP row truncated: runtime reduction byte limit]"));
                    assert!(content.len() > 3 * 1024 * 1024);
                } else {
                    assert_eq!(*content, format!("{}healthy", "z".repeat(1022)));
                }
                continue;
            }
            let delivered: String = fixture["rpc"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|record| {
                    record
                        .pointer("/frame/params/update/content/text")
                        .and_then(serde_json::Value::as_str)
                })
                .collect();
            assert_eq!(
                *content, delivered,
                "{name}: every delivered character is retained"
            );
            assert!(!content.contains("[ACP row truncated:"));
            if name.starts_with("long-stream-") {
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
    let mut original = item("tool", true, "Original tool caption");
    original.tool_title = Some("Opaque tool".into());
    original.tool_input = Some(serde_json::json!({"command":"opaque"}));
    original.tool_result = Some("x".repeat(15900));
    put(&mut maps, 1, original);
    let patch = AgentRuntimeEvent::ChatItemPatch {
        thread_id: "t".into(),
        item: ChatMsg {
            id: "tool".into(),
            kind: "assistant".into(),
            seq: 99,
            tool_status: Some("completed".into()),
            ..Default::default()
        },
    };
    let event = translate_with_items(&patch, &mut maps, false).unwrap();
    fold(&mut maps, 1, &event);
    let row = &maps.items[0].item;
    assert_eq!(row.seq, 0);
    assert_eq!(row.kind, "tool");
    assert_eq!(row.content, "Original tool caption");
    assert_eq!(row.tool_title.as_deref(), Some("Opaque tool"));
    assert_eq!(
        row.tool_input,
        Some(serde_json::json!({"command":"opaque"}))
    );
    assert_eq!(row.tool_result.as_ref().unwrap().len(), 15900);
    assert_eq!(row.tool_status.as_deref(), Some("completed"));
}

#[test]
fn tool_patches_cannot_blank_or_retype_conversation_rows() {
    for kind in ["user", "assistant", "thinking"] {
        let mut maps = RuntimeMaps::default();
        let original = ChatMsg {
            id: "row".into(),
            seq: 7,
            kind: kind.into(),
            content: "Original body".into(),
            ..Default::default()
        };
        put(&mut maps, 1, original.clone());
        let patch = AgentRuntimeEvent::ChatItemPatch {
            thread_id: "t".into(),
            item: ChatMsg {
                id: "row".into(),
                kind: "tool_call".into(),
                tool_status: Some("completed".into()),
                ..Default::default()
            },
        };
        let event = translate_with_items(&patch, &mut maps, false).unwrap();
        assert!(matches!(event, RuntimeEvent::Notice { .. }));
        fold(&mut maps, 1, &event);
        assert_eq!(maps.items[0].item, original);
    }
}

#[test]
fn binding_change_between_restore_steps_preserves_new_cache_and_maps() {
    use super::super::model::{ItemCache, RuntimeFailure};
    use super::super::transcript::apply_restored_rows;
    use parking_lot::Mutex;
    use std::cell::{Cell, RefCell};

    let original = RuntimeContext {
        epoch: 1,
        thread_id: Some("old-thread".into()),
        binding_nonce: "old-nonce".into(),
        ..Default::default()
    };
    for changed in [
        RuntimeContext {
            epoch: 2,
            ..original.clone()
        },
        RuntimeContext {
            thread_id: Some("new-thread".into()),
            ..original.clone()
        },
        RuntimeContext {
            binding_nonce: "new-nonce".into(),
            ..original.clone()
        },
    ] {
        for before_cache in [true, false] {
            let current = RefCell::new(original.clone());
            let cache = Mutex::new(ItemCache::default());
            let maps = Mutex::new(RuntimeMaps::default());
            maps.lock().prepare_restore(&original);
            let mut fresh = item("new-row", false, "New binding body");
            fresh.kind = "assistant".into();
            let replace_binding = || {
                *current.borrow_mut() = changed.clone();
                let mut target = cache
                    .try_lock()
                    .expect("binding changes occur between cache steps");
                *target = ItemCache::default();
                target.reconcile(fresh.clone());
                drop(target);
                let mut target = maps
                    .try_lock()
                    .expect("binding changes occur between map steps");
                target.reset(changed.epoch);
                target.sync_active_turn(&changed);
                reconcile_history(
                    &mut target,
                    changed.epoch,
                    changed.thread_id.as_deref().unwrap(),
                    fresh.clone(),
                );
            };
            if before_cache {
                replace_binding();
            }
            let checks = Cell::new(0);
            let result = apply_restored_rows(
                &cache,
                &maps,
                &original,
                vec![item("old-row", false, "Stale body")],
                || {
                    let index = checks.get();
                    checks.set(index + 1);
                    if !before_cache && index == 1 {
                        assert!(cache.try_lock().unwrap().get("old-row").is_some());
                        replace_binding();
                    }
                    let current = current.borrow();
                    current.epoch == original.epoch
                        && current.thread_id == original.thread_id
                        && current.binding_nonce == original.binding_nonce
                },
            );
            assert!(matches!(result, Err(RuntimeFailure::Superseded)));
            assert_eq!(cache.lock().snapshot(), vec![fresh.clone()]);
            let mut target = maps.lock();
            assert_eq!(target.epoch, changed.epoch);
            assert_eq!(target.items.len(), 1);
            assert_eq!(target.items[0].item, fresh);
            assert!(!reconcile_history_current(
                &mut target,
                &original,
                "old-thread",
                item("late", false, "stale")
            ));
            assert!(!target.prepare_restore(&original));
            assert_eq!(target.epoch, changed.epoch);
            assert_eq!(target.items.len(), 1);
        }
    }
}

#[test]
fn runtime_routed_inbound_reuses_chatbackend_metadata_without_trusting_human_frames() {
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/routed-inbound.json")).unwrap();
    for fixture in fixtures {
        let wire = fixture["wire"].as_str().unwrap();
        let frame = crate::service::messenger::Messenger::restore_framed_delivery(wire).unwrap();
        let mut maps = RuntimeMaps::default();
        maps.ensure_epoch(1);
        maps.remember_routed_input("send", "thread", &frame)
            .unwrap();
        let source = AgentRuntimeEvent::ChatItem {
            thread_id: "thread".into(),
            item: ChatMsg {
                id: "user".into(),
                kind: "user".into(),
                content: wire.into(),
                ..Default::default()
            },
        };
        let evidence = HashSet::from([("send".into(), Some("user".into()))]);
        let mut projected = translate_with_items(&source, &mut maps, false).unwrap();
        maps.project_routed_item(&mut projected, &evidence);
        let RuntimeEvent::ItemUpdated { item, .. } = projected else {
            panic!()
        };
        let actual = serde_json::to_value(&item).unwrap();
        for key in [
            "content",
            "inboundKind",
            "fromLabel",
            "fromTab",
            "fromModel",
            "fromPurpose",
            "fromContext",
            "fromMesh",
        ] {
            assert_eq!(actual.get(key), fixture.get(key), "{key}");
        }
        assert_eq!(item.id, "user");
        let mut cache = super::super::model::ItemCache::default();
        let retained = cache.reconcile(item.clone());
        assert_eq!(retained.content, fixture["content"].as_str().unwrap());
        assert_eq!(retained.segments, item.segments);
        let mut unsupported = translate_with_items(&source, &mut maps, false).unwrap();
        maps.project_routed_item(&mut unsupported, &HashSet::new());
        let RuntimeEvent::ItemUpdated { item, .. } = unsupported else {
            panic!()
        };
        assert_eq!(item.content, wire);
        assert!(item.inbound_kind.is_none());
        let mut untrusted = RuntimeMaps::default();
        let mut projected = translate_with_items(&source, &mut untrusted, false).unwrap();
        untrusted.project_routed_item(&mut projected, &evidence);
        let RuntimeEvent::ItemUpdated { item, .. } = projected else {
            panic!()
        };
        assert_eq!(item.content, wire);
        assert!(item.inbound_kind.is_none());
        let foreign = AgentRuntimeEvent::ChatItem {
            thread_id: "foreign".into(),
            item: ChatMsg {
                id: "user".into(),
                kind: "user".into(),
                content: wire.into(),
                ..Default::default()
            },
        };
        let mut projected = translate_with_items(&foreign, &mut maps, false).unwrap();
        maps.project_routed_item(&mut projected, &evidence);
        let RuntimeEvent::ItemUpdated { item, .. } = projected else {
            panic!()
        };
        assert_eq!(item.content, wire);
        assert!(item.inbound_kind.is_none());
        maps.reset(2);
        let mut projected = translate_with_items(&source, &mut maps, false).unwrap();
        maps.project_routed_item(&mut projected, &evidence);
        let RuntimeEvent::ItemUpdated { item, .. } = projected else {
            panic!()
        };
        assert_eq!(item.content, wire);
        assert!(item.inbound_kind.is_none());
    }
}

#[test]
fn restart_replay_uses_source_identity_for_two_identical_written_prompts() {
    use crate::service::agent_runtime::model::ItemCache;
    let mut maps = RuntimeMaps::default();
    let mut rows = Vec::new();
    for (ordinal, (send, source)) in [
        ("first-send", "first-source"),
        ("second-send", "second-source"),
    ]
    .into_iter()
    .enumerate()
    {
        let row = ChatMsg {
            id: RuntimeMaps::written_input_id("t", send),
            seq: ordinal as i64,
            kind: "user".into(),
            content: "identical opaque prompt".into(),
            attachments: Some(vec![codeterm_types::agent::chat_types::ChatAttachment {
                id: format!("image-{ordinal}"),
                mime_type: "image/png".into(),
                path: Some(format!("/opaque/image-{ordinal}.png")),
                ..Default::default()
            }]),
            ..Default::default()
        };
        put(&mut maps, 1, row);
        let incoming = RuntimeEvent::ItemUpdated {
            thread_id: "t".into(),
            item: ChatMsg {
                id: source.into(),
                kind: "user".into(),
                content: "identical opaque prompt".into(),
                ..Default::default()
            },
        };
        let evidence = HashSet::from([(send.to_owned(), Some(source.to_owned()))]);
        let projected = maps.project_written_item(&incoming, &evidence).unwrap();
        fold(&mut maps, 1, &incoming);
        fold(&mut maps, 1, &projected);
        let RuntimeEvent::ItemUpdated { item, .. } = projected else {
            panic!("Expected user row")
        };
        assert_eq!(item.runtime_source_item_id.as_deref(), Some(source));
        assert_eq!(
            item.attachments.as_ref().unwrap()[0].id,
            format!("image-{ordinal}")
        );
        rows.push(item);
    }
    let stored = serde_json::to_vec(&rows).unwrap();
    let restored: Vec<ChatMsg> = serde_json::from_slice(&stored).unwrap();
    let mut cache = ItemCache::default();
    cache.restore_transcript(restored.clone());
    maps = RuntimeMaps::default();
    for row in restored {
        put(&mut maps, 2, row);
    }
    for source in [
        "first-source",
        "second-source",
        "first-source",
        "second-source",
    ] {
        let incoming = RuntimeEvent::ItemUpdated {
            thread_id: "t".into(),
            item: ChatMsg {
                id: source.into(),
                kind: "user".into(),
                content: "identical opaque prompt".into(),
                ..Default::default()
            },
        };
        let projected = maps
            .project_written_item(&incoming, &HashSet::new())
            .unwrap();
        fold(&mut maps, 2, &incoming);
        fold(&mut maps, 2, &projected);
        let RuntimeEvent::ItemUpdated { item, .. } = projected else {
            panic!("Expected user row")
        };
        cache.reconcile(item);
    }
    let users = cache.snapshot();
    assert_eq!(users.len(), 2);
    for (ordinal, row) in users.iter().enumerate() {
        assert_eq!(
            row.attachments.as_ref().unwrap()[0].id,
            format!("image-{ordinal}")
        );
    }
    assert_eq!(
        users[0].id,
        RuntimeMaps::written_input_id("t", "first-send")
    );
    assert_eq!(
        users[1].id,
        RuntimeMaps::written_input_id("t", "second-send")
    );
    let foreign = RuntimeEvent::ItemUpdated {
        thread_id: "foreign-thread".into(),
        item: ChatMsg {
            id: "first-source".into(),
            kind: "user".into(),
            content: "identical opaque prompt".into(),
            ..Default::default()
        },
    };
    assert!(maps
        .project_written_item(&foreign, &HashSet::new())
        .is_none());
    let unknown = RuntimeEvent::ItemUpdated {
        thread_id: "t".into(),
        item: ChatMsg {
            id: "third-source".into(),
            kind: "user".into(),
            content: "identical opaque prompt".into(),
            ..Default::default()
        },
    };
    assert!(maps
        .project_written_item(&unknown, &HashSet::new())
        .is_none());
}

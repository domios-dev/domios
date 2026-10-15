use super::*;
use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::TabBacking;
use crate::plan::{OrchestratorRef, Plan};

#[test]
fn runtime_heartbeat_health_tracks_loaded_and_failed_epochs_honestly() {
    use crate::service::agent_runtime::model::RuntimePhase;
    assert_eq!(snapshot::runtime_process(&RuntimePhase::Ready), "present");
    assert_eq!(snapshot::runtime_process(&RuntimePhase::Loading), "unknown");
    for phase in [
        RuntimePhase::Unloaded { reason: None },
        RuntimePhase::Degraded {
            reason: "opaque-fault".into(),
        },
    ] {
        assert_eq!(snapshot::runtime_process(&phase), "absent");
    }
}

#[test]
fn runtime_heartbeat_cache_observes_classified_turns_once_without_history_replay() {
    let owner = ChatMsg {
        id: "owner-turn".into(),
        kind: "user".into(),
        content: "opaque-owner".into(),
        ..Default::default()
    };
    let reminder = ChatMsg {
        id: "reminder-turn".into(),
        kind: "user".into(),
        content: "opaque-reminder".into(),
        inbound_kind: Some("reminder".into()),
        ..Default::default()
    };
    let messages = vec![owner, reminder];
    let (first, cursor, unavailable) = snapshot::appended_messages(messages.clone(), None);
    assert!(!unavailable);
    assert_eq!(
        first
            .iter()
            .filter(|message| snapshot::is_owner_turn(message))
            .count(),
        1
    );
    assert_eq!(first.len(), 2);
    assert_eq!(cursor.as_deref(), Some("reminder-turn"));
    let (again, next_cursor, unavailable) =
        snapshot::appended_messages(messages, cursor.as_deref());
    assert!(!unavailable);
    assert!(again.is_empty());
    assert_eq!(next_cursor, cursor);
}

#[test]
fn heartbeat_gate_turn_hashes_detect_loops_across_ticks() {
    let message = ChatMsg {
        id: "turn-a".into(),
        kind: "assistant".into(),
        content: "opaque repeated fixture".into(),
        ..Default::default()
    };
    let mut worker = Worker::default();
    for _ in 0..3 {
        worker.recent_turn_hashes = snapshot::recent_turn_hashes(Some(&worker), &[message.clone()]);
    }
    assert_eq!(worker.recent_turn_hashes.len(), 3);
    assert!(worker
        .recent_turn_hashes
        .iter()
        .all(|h| h == &worker.recent_turn_hashes[0]));
    let changed = ChatMsg {
        content: "opaque progress fixture".into(),
        ..message
    };
    let hashes = snapshot::recent_turn_hashes(Some(&worker), &[changed]);
    assert_ne!(hashes[0], hashes[2]);
    assert_eq!(hashes.len(), 3);
}

#[test]
fn heartbeat_gate_owner_signals_use_structural_provenance() {
    let mut message = ChatMsg {
        kind: "user".into(),
        content: "opaque owner input".into(),
        ..Default::default()
    };
    assert!(snapshot::is_owner_turn(&message));
    message.from_label = Some("opaque bridge owner".into());
    message.inbound_kind = Some("telegram".into());
    assert!(snapshot::is_owner_turn(&message));
    message.from_tab = Some("opaque bridge sender".into());
    assert!(snapshot::is_owner_turn(&message));
    message.inbound_kind = Some("tab".into());
    assert!(!snapshot::is_owner_turn(&message));
    message.from_tab = None;
    message.inbound_kind = None;
    for kind in ["system", "reminder", "report", "tab"] {
        message.content = codeterm_wire::domios::DomiosFrame {
            opening_tag: codeterm_wire::domios::build_opening_tag(&[("from", kind)]),
            body: "opaque daemon input".into(),
            suffix: String::new(),
        }
        .to_message();
        assert!(!snapshot::is_owner_turn(&message));
    }
}

#[test]
fn heartbeat_gate_binding_requires_the_tabs_own_confirmed_nonce() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "fixture".into(),
        session_id: "own-session".into(),
    };
    tab.chat_view = true;
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("foreign-nonce".into());
    assert!(snapshot::session_key(tab).is_none());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    assert_eq!(
        snapshot::session_key(tab).as_deref(),
        Some("own-session:own-nonce")
    );
    let mut attempt = crate::plan::TaskExecutionAttempt {
        session_id: Some("foreign-session".into()),
        ..Default::default()
    };
    assert!(snapshot::bound_attempt_session_key(tab, &attempt).is_none());
    attempt.session_id = Some("own-session".into());
    attempt.session_binding_nonce = Some("foreign-nonce".into());
    assert!(snapshot::bound_attempt_session_key(tab, &attempt).is_none());
    attempt.session_binding_nonce = Some("own-nonce".into());
    assert!(snapshot::bound_attempt_session_key(tab, &attempt).is_some());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "fixture".into(),
        session_id: "foreign-session".into(),
    };
    assert!(snapshot::session_key(tab).is_none());
    tab.agent_session_id = None;
    assert!(snapshot::session_key(tab).is_none());
}

#[test]
fn heartbeat_gate_plan_delta_projection_is_ordered_and_content_addressed() {
    let mut plan = Plan::new(
        "fixture".into(),
        "/fixture".into(),
        OrchestratorRef::from_tab("owner"),
    );
    let before = snapshot::plan_projection(&plan);
    assert_eq!(before, snapshot::plan_projection(&plan.clone()));
    plan.mode = if plan.mode == crate::plan::PlanMode::Execution {
        crate::plan::PlanMode::Planning
    } else {
        crate::plan::PlanMode::Execution
    };
    assert_ne!(before["mode"], snapshot::plan_projection(&plan)["mode"]);
}

#[test]
fn heartbeat_gate_owner_rules_do_not_consume_situational_selection_slots() {
    let mut plan = Plan::new(String::new(), String::new(), Default::default());
    for index in 0..13 {
        plan.heartbeat
            .learnings
            .push(crate::plan::HeartbeatLearning {
                id: format!("opaque-situation-{index}"),
                text: format!("opaque-fragment-{index}"),
                ts: index,
                tag: None,
            });
    }
    for index in 0..6 {
        plan.heartbeat
            .learnings
            .push(crate::plan::HeartbeatLearning {
                id: format!("opaque-owner-{index}"),
                text: format!("opaque-rule-{index}"),
                ts: index + 20,
                tag: Some("owner".into()),
            });
    }
    let catalog = snapshot::situational_learning_catalog(&plan);
    assert_eq!(catalog.len(), 12);
    for learning in &plan.heartbeat.learnings {
        assert_eq!(
            catalog.contains_key(&learning.id),
            learning.tag.is_none() && learning.ts > 0
        );
    }
}

use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn focus_wakes_failed_sleeping_chat_on_its_own_session() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.protected = false;
    tab.frozen = true;
    tab.restored_from_snapshot = true;
    tab.agent_type = Some("fixture-runtime".into());
    tab.agent_session_id = Some("own-thread".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "fixture-runtime".into(),
        session_id: "own-thread".into(),
    };
    tab.boot_stage = Some(BootStage::SpawnFailed);
    let fixture = ServiceFixture::new(mux);
    let service = fixture.service.clone();
    let target = id.clone();
    let (completed, completion) = std::sync::mpsc::channel();
    let _port = test_boundary::Boundary::install(
        &id,
        std::sync::Arc::new(move || {
            let mut mux = service.mux.write();
            let tab = mux.get_tab_mut(&target).unwrap();
            assert!(!tab.frozen);
            assert_eq!(tab.agent_session_id.as_deref(), Some("own-thread"));
            assert_eq!(tab.session_binding_nonce.as_deref(), Some("own-nonce"));
            tab.reported_session_binding_nonce = Some("own-nonce".into());
            tab.restored_from_snapshot = false;
            super::super::boot_stage::apply_boot_stage(tab, BootStage::AgentReady, None);
            completed.send(()).unwrap();
            Ok(())
        }),
    );
    fixture.service.focus_tab(&id).unwrap();
    completion
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert_eq!(mux.get_focused_tab_id(), Some(id.as_str()));
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert!(tab.pty_id.is_none());
}

#[test]
fn restart_thaws_before_runtime_ensure_without_opening_a_second_backend() {
    for backing in [
        TabBacking::Pty,
        TabBacking::ChatBackend {
            plugin_id: "fixture".into(),
            session_id: "own-session".into(),
        },
    ] {
        let (mut mux, id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.protected = false;
        tab.backing = backing;
        tab.frozen = true;
        tab.agent_session_id = Some("own-session".into());
        tab.session_binding_nonce = Some("own-nonce".into());
        let fixture = ServiceFixture::new(mux);
        let _port = test_boundary::Boundary::install(
            &id,
            std::sync::Arc::new(|| panic!("restart must own materialization")),
        );
        assert!(fixture.service.mux.read().get_tab(&id).unwrap().frozen);
        fixture.service.thaw_tab_for_restart(&id).unwrap();
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some("own-session"));
        assert_eq!(tab.session_binding_nonce.as_deref(), Some("own-nonce"));
        assert!(!tab.frozen);
    }
}

#[test]
fn addressed_sleeping_chat_wakes_before_opening_its_backend() {
    use crate::service::messenger::{MessageEnvelope, Messenger};
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.protected = false;
    tab.frozen = true;
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "fixture".into(),
        session_id: "own-session".into(),
    };
    let fixture = ServiceFixture::new(mux);
    let _port = test_boundary::Boundary::install(
        &id,
        std::sync::Arc::new(|| Err("injected wake reached before input".into())),
    );
    assert_eq!(
        Messenger::deliver(
            &fixture.service,
            &id,
            MessageEnvelope::codeterm_reminder("fixture"),
            "opaque-body"
        )
        .unwrap_err(),
        "injected wake reached before input"
    );
}

#[test]
fn startup_materializes_interrupted_tabs_without_plan_or_live_state() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("saved-session".into());
    crate::multiplexer::types::observe_turn(
        &mut tab.turn_resume,
        Some("saved-session"),
        "previous-start",
    );
    let restored = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
    let fixture = ServiceFixture::new(restored);
    initialize(&fixture.service, 48);
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert!(tab.agent_state.is_none());
    assert!(!tab.frozen);
    assert_eq!(tab.agent_session_id.as_deref(), Some("saved-session"));
}

#[test]
fn explicit_chat_wake_bypasses_pending_boot_restore() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.protected = false;
    tab.frozen = false;
    tab.restored_from_snapshot = true;
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "fixture".into(),
        session_id: "own-session".into(),
    };
    assert!(!wake_eligible(tab, false));
    assert!(wake_eligible(tab, true));
    tab.protected = true;
    assert!(!wake_eligible(tab, true));
    tab.protected = false;
    tab.restored_from_snapshot = false;
    assert!(!wake_eligible(tab, true));
}

#[test]
fn startup_keeps_general_awake_and_freezes_unknown_project_activity() {
    let (mut mux, general) = MultiplexerState::new();
    mux.register_alias(super::super::general_agent::GENERAL_ALIAS, &general);
    let general_ws = mux.list_workspaces()[0].id.clone();
    let sibling = mux.create_tab(&general_ws).unwrap();
    for id in [&general, &sibling] {
        mux.get_tab_mut(id).unwrap().restored_from_snapshot = true;
    }
    let fixture = ServiceFixture::new(mux);
    initialize(&fixture.service, 48);
    for id in [&general, &sibling] {
        assert!(!fixture.service.mux.read().get_tab(id).unwrap().frozen);
    }
}

#[test]
fn startup_freezes_recent_idle_agents_but_preserves_busy_and_protected_tabs() {
    let (mut mux, idle) = MultiplexerState::new();
    mux.register_alias(super::super::general_agent::GENERAL_ALIAS, &idle);
    let workspace = mux.list_workspaces()[0].id.clone();
    let busy = mux.create_tab(&workspace).unwrap();
    let protected = mux.create_tab(&workspace).unwrap();
    for id in [&idle, &busy, &protected] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.restored_from_snapshot = true;
        tab.agent_type = Some("fixture".into());
        tab.last_active_at_ms = Some(u64::MAX);
    }
    mux.get_tab_mut(&protected).unwrap().protected = true;
    mux.get_tab_mut(&busy).unwrap().agent_runtime_state =
        Some(serde_json::json!({"active_turn_started_at_ms": 42}));
    let fixture = ServiceFixture::new(mux);
    initialize(&fixture.service, 48);
    let mux = fixture.service.mux.read();
    assert!(mux.get_tab(&idle).unwrap().frozen);
    assert!(!mux.get_tab(&busy).unwrap().frozen);
    assert!(!mux.get_tab(&protected).unwrap().frozen);
}

#[test]
fn materializer_failure_is_visible_and_explicit_wake_retries_once() {
    let (mut mux, id) = MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().frozen = true;
    mux.get_tab_mut(&id).unwrap().agent_type = Some("fixture".into());
    let fixture = ServiceFixture::new(mux);
    let reason = "fixture materializer error";
    assert_eq!(
        wake_with(&fixture.service, &id, false, |_, _| Err(reason.into())),
        Err(reason.into())
    );
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
        assert_eq!(tab.boot_failure_reason.as_deref(), Some(reason));
    }
    wake_with(&fixture.service, &id, false, |_, _| {
        panic!("address retried failed wake")
    })
    .unwrap();
    let calls = std::cell::Cell::new(0);
    wake_with(
        &fixture.service,
        &id,
        true,
        |resolved, retry_started_agent| {
            assert!(retry_started_agent);
            calls.set(calls.get() + 1);
            let tab = fixture
                .service
                .mux
                .read()
                .get_tab(resolved)
                .cloned()
                .unwrap();
            assert_eq!(tab.boot_stage, Some(BootStage::PtyStarting));
            assert!(tab.boot_failure_reason.is_none());
            wake_with(&fixture.service, resolved, true, |_, _| {
                panic!("wake duplicated while materializing")
            })?;
            fixture
                .service
                .mux
                .write()
                .get_tab_mut(resolved)
                .unwrap()
                .pty_id = Some("materialized".into());
            Ok(())
        },
    )
    .unwrap();
    wake_with(&fixture.service, &id, true, |_, _| {
        panic!("completed wake duplicated")
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
}

#[test]
fn repeated_wakes_materialize_once() {
    let (mut mux, id) = MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().frozen = true;
    let fixture = ServiceFixture::new(mux);
    let calls = std::cell::Cell::new(0);
    for _ in 0..2 {
        wake_with(&fixture.service, &id, false, |_, _| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .unwrap();
    }
    assert_eq!(calls.get(), 1);
}

#[test]
fn waking_keeps_runtime_sleep_marker_and_exact_session() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.frozen = true;
    tab.slept_at_ms = Some(42);
    tab.agent_session_id = Some("own-session".into());
    let fixture = ServiceFixture::new(mux);
    wake_with(&fixture.service, &id, false, |resolved, _| {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(resolved).unwrap();
        assert!(!tab.frozen);
        assert_eq!(tab.slept_at_ms, Some(42));
        assert_eq!(tab.agent_session_id.as_deref(), Some("own-session"));
        Ok(())
    })
    .unwrap();
}

#[test]
fn missing_backend_wake_eligibility_excludes_owned_stopped_and_protected_tabs() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.protected = false;
    tab.frozen = false;
    tab.pty_id = None;
    assert!(!wake_eligible(tab, false));
    assert!(wake_eligible(tab, true));
    tab.protected = true;
    assert!(!wake_eligible(tab, true));
    tab.protected = false;
    tab.pty_id = Some("owned".into());
    assert!(!wake_eligible(tab, true));
    tab.pty_id = None;
    tab.close_restore = Some(crate::multiplexer::types::CloseRestoreTabState {
        stopped: true,
        ..Default::default()
    });
    assert!(!wake_eligible(tab, true));
}

#[test]
fn awake_missing_backend_materializes_without_restart_and_publishes_failure() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.protected = false;
    tab.frozen = false;
    tab.pty_id = None;
    tab.agent_session_id = Some("exact-session".into());
    let fixture = ServiceFixture::new(mux);
    let reason = r#"{"code":"workspace_cwd_missing","path":"/deleted"}"#;
    assert_eq!(
        wake_with(&fixture.service, &id, true, |resolved, restart| {
            assert_eq!(resolved, id);
            assert!(!restart);
            wake_with(&fixture.service, resolved, true, |_, _| {
                panic!("duplicate wake")
            })?;
            Err(reason.into())
        }),
        Err(reason.into())
    );
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("exact-session"));
    assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
    assert_eq!(tab.boot_failure_reason.as_deref(), Some(reason));
}

#[test]
fn frozen_address_materializes_before_delivery() {
    let (mut mux, id) = MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().frozen = true;
    let fixture = ServiceFixture::new(mux);
    let mut events = Vec::new();
    wake_with(&fixture.service, &id, false, |resolved, _| {
        assert_eq!(resolved, id);
        assert!(!fixture.service.mux.read().get_tab(resolved).unwrap().frozen);
        events.push("materialize");
        Ok(())
    })
    .unwrap();
    events.push("deliver");
    assert_eq!(events, ["materialize", "deliver"]);
}

#[test]
fn blueprint_v2_call_durable_review_lazy_failure_wakes_and_preserves_materializer_cause() {
    use crate::service::messenger::{Delivery, MessageEnvelope, Messenger};
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("saved-session".into());
    let fixture = ServiceFixture::new(MultiplexerState::from_session_snapshot(
        &mux.to_session_snapshot(),
    ));
    let writes = fixture.record_agent_deliveries(&id);
    let mut envelope = MessageEnvelope::codeterm_reminder("blueprint:result");
    envelope.durable_delivery = Some(fixture.service.durable_delivery("run", &id));
    assert!(matches!(
        Messenger::deliver(&fixture.service, &id, envelope, "result"),
        Ok(Delivery::Pending { .. })
    ));
    writes.wait_for_flush();
    let reason = "fixture materializer refused missing workspace";
    assert_eq!(
        wake_with(&fixture.service, &id, true, |_, _| Err(reason.into())),
        Err(reason.into())
    );
    writes.wait_for_flush();
    let events = fixture.ui.0.lock();
    let failure = events
        .iter()
        .find(|(kind, value)| kind == "agent-delivery-status" && value["status"] == "failed")
        .expect("lazy restore failure settles pending result");
    assert_eq!(failure.1["code"], "recipient_launch_failed");
    assert_eq!(failure.1["detail"], reason);
    assert!(writes.texts.lock().is_empty());
}

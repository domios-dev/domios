use super::*;

#[test]
fn handoff_routes_both_transport_directions_by_capability() {
    let pty = (ProviderTransport::Pty, false);
    let runtime = (ProviderTransport::ChatBackend, true);
    assert_eq!(
        route(pty, runtime, false, &TabBacking::Pty).unwrap(),
        HandoffRoute {
            source: TargetRoute::Pty,
            target: TargetRoute::Runtime
        }
    );
    assert_eq!(
        route(
            runtime,
            pty,
            false,
            &TabBacking::ChatBackend {
                plugin_id: "opaque".into(),
                session_id: "thread".into()
            }
        )
        .unwrap(),
        HandoffRoute {
            source: TargetRoute::Runtime,
            target: TargetRoute::Pty
        }
    );
    assert!(route(
        pty,
        (ProviderTransport::ChatBackend, false),
        false,
        &TabBacking::Pty
    )
    .is_err());
    assert!(route(pty, runtime, true, &TabBacking::Pty).is_err());
    assert!(route(runtime, pty, false, &TabBacking::Pty).is_err());
    let dual = (ProviderTransport::Pty, true);
    assert_eq!(
        route(dual, pty, false, &TabBacking::Pty).unwrap().source,
        TargetRoute::Pty
    );
    assert_eq!(
        route(pty, dual, false, &TabBacking::Pty).unwrap().target,
        TargetRoute::Runtime
    );
    assert_eq!(
        route(
            dual,
            pty,
            false,
            &TabBacking::ChatBackend {
                plugin_id: "opaque".into(),
                session_id: "thread".into()
            }
        )
        .unwrap()
        .source,
        TargetRoute::Runtime
    );
}

#[test]
fn busy_switch_requires_explicit_stop_and_refuses_an_inflight_launch() {
    assert!(require_idle(true, false, false).is_ok());
    assert!(require_idle(false, false, false).is_err());
    assert!(require_idle(false, true, false).is_ok());
    assert!(require_idle(true, true, true).is_err());
}

#[test]
fn runtime_idle_requires_live_binding_and_no_active_turn_or_question() {
    assert!(runtime_idle(true, false, false));
    assert!(!runtime_idle(false, false, false));
    assert!(!runtime_idle(true, true, false));
    assert!(!runtime_idle(true, false, true));
}

#[test]
fn backing_transition_preserves_tab_and_task_identity_in_both_directions() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.title = "Baton".into();
    tab.description = Some("opaque-description".into());
    tab.team_id = Some("team".into());
    tab.parent_tab_id = Some("parent".into());
    tab.cwd = Some("lane".into());
    tab.role = Some(crate::multiplexer::types::AgentRole::Worker);
    tab.role_profile = Some("opaque-profile".into());
    tab.worker_id = Some("worker-token".into());
    tab.report_target_tab_id = Some("report-target".into());
    tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
        plan_id: Some("plan".into()),
        task_id: Some("task".into()),
        attempt_id: Some("attempt".into()),
        spawn_handle: None,
        bound_at: 1,
    });
    let identity = |tab: &Tab| {
        serde_json::json!([
            tab.id,
            tab.title,
            tab.description,
            tab.role,
            tab.role_profile,
            tab.team_id,
            tab.parent_tab_id,
            tab.cwd,
            tab.task_binding,
            tab.worker_id,
            tab.report_target_tab_id
        ])
    };
    let before = identity(tab);
    for route in [TargetRoute::Runtime, TargetRoute::Pty] {
        reset_for_target(
            tab,
            "target",
            route,
            &RestorableLaunchParameters {
                captured: true,
                model: Some("opaque-model".into()),
                ..Default::default()
            },
        );
        assert_eq!(identity(tab), before);
        assert_eq!(tab.agent_type.as_deref(), Some("target"));
        assert!(tab.agent_session_id.is_none());
        assert!(tab.pty_id.is_none());
        assert_eq!(
            tab.agent_provider_resolved_model.as_deref(),
            Some("opaque-model")
        );
        assert_eq!(
            tab.backing.kind(),
            if route == TargetRoute::Runtime {
                "chat_backend"
            } else {
                "pty"
            }
        );
    }
}

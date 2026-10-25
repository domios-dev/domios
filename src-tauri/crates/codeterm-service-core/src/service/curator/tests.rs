use super::*;

#[test]
fn curator_mirrors_only_the_orchestrators_confirmed_session() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
        plugin_id: "fixture-provider".into(),
        session_id: "session".into(),
    };
    tab.chat_view = true;
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("session".into());
    tab.agent_session_model = Some("actual-model".into());
    tab.agent_requested_model = Some("stale-request".into());
    tab.session_binding_nonce = Some("nonce".into());
    tab.reported_session_binding_nonce = Some("foreign".into());
    assert!(scheduling::resolve_mirror(tab).is_err());
    tab.reported_session_binding_nonce = Some("nonce".into());
    tab.reasoning_effort_identity.actual = Some("actual-effort".into());
    let mirror = scheduling::resolve_mirror(tab).unwrap();
    assert_eq!(mirror.provider, "fixture-provider");
    assert_eq!(mirror.model, "actual-model");
    assert_eq!(mirror.effort.as_deref(), Some("actual-effort"));
    tab.agent_session_model = None;
    assert!(scheduling::resolve_mirror(tab).is_err());
}

#[test]
fn curator_grants_expire_at_round_session_and_binding_boundaries() {
    let identity = CuratorIdentity {
        plan_id: "plan".into(),
        number: 3,
        tab_id: "tab".into(),
        session_id: "session".into(),
        binding_nonce: "nonce".into(),
    };
    let grant = CuratorGrant {
        tab_id: "tab".into(),
        session_id: "session".into(),
        binding_nonce: "nonce".into(),
        round: 3,
        ..Default::default()
    };
    assert!(authority::grant_matches(&grant, &identity));
    for changed in [
        CuratorIdentity {
            number: 4,
            ..identity.clone()
        },
        CuratorIdentity {
            session_id: "restarted".into(),
            ..identity.clone()
        },
        CuratorIdentity {
            binding_nonce: "rebound".into(),
            ..identity.clone()
        },
        CuratorIdentity {
            tab_id: "other".into(),
            ..identity.clone()
        },
    ] {
        assert!(!authority::grant_matches(&grant, &changed));
    }
}

#[test]
fn curator_kind_has_no_plan_spawn_or_team_authority() {
    let kind = codeterm_types::agent::role_kind::builtins::lookup("curator").unwrap();
    assert!(!kind.can_spawn());
    assert!(!kind.plan_edit());
    assert!(!kind.plan_approval());
    assert!(!kind.has_team_authority());
    assert!(!kind.anchors_team());
    assert!(codeterm_prompts::role_preset::builtin_guidance("curator").is_some());
}

#[test]
fn curator_runtime_registration_binds_the_launched_round() {
    use crate::service::test_fixture::ServiceFixture;
    let (mux, tab) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    assert!(service.try_ext::<CuratorTabs>().is_none());
    service.start_curator_events().unwrap();
    assert!(service.try_ext::<CuratorTabs>().is_some());
    let mut plan = Plan::new(
        "Round fixture".into(),
        fixture.temp.path().display().to_string(),
        Default::default(),
    );
    plan.team_id = Some("fixture-team".into());
    plan.control_loop.running = Some(CuratorRun {
        number: 2,
        started_at: 100,
        ..Default::default()
    });
    service.plan_store.save(&mut plan, 100).unwrap();
    service.curator_spawned(&plan.id, &tab, "run", 101).unwrap();
    assert_eq!(
        service
            .mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .description
            .as_deref(),
        Some("Round 2")
    );
    let persisted = service.plan_store.get(&plan.id).unwrap();
    let running = persisted.control_loop.running.unwrap();
    assert_eq!(running.tab_id.as_deref(), Some(tab.as_str()));
    assert_eq!(running.run_id.as_deref(), Some("run"));
    assert_eq!(
        service.ext::<CuratorTabs>().0.read().get(&tab),
        Some(&plan.id)
    );
}

#[test]
fn curator_manual_run_refuses_without_its_runtime() {
    let (fixture, plan, _, owner) = crate::service::plan_authority::test_fixture::fixture();
    let before = fixture.service.plan_store.get(&plan).unwrap().control_loop;
    let error = fixture
        .service
        .control_loop_run(&plan, &owner, 100)
        .unwrap_err();
    assert_eq!(error, "Curator runtime is unavailable");
    assert_eq!(
        fixture.service.plan_store.get(&plan).unwrap().control_loop,
        before
    );
}

#[test]
fn curator_launch_failure_clears_the_unbound_round() {
    let (fixture, id, _, _) = crate::service::plan_authority::test_fixture::fixture();
    fixture
        .service
        .plan_store
        .with_plan(&id, 100, |plan| {
            plan.control_loop.running = Some(CuratorRun {
                number: 2,
                started_at: 100,
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    fixture
        .service
        .curator_fire_failed(&id, CuratorFailure::LaunchFailed, 101)
        .unwrap();
    let plan = fixture.service.plan_store.get(&id).unwrap();
    assert!(plan.control_loop.running.is_none());
    let round = plan.control_loop.last_round.unwrap();
    assert_eq!(round.outcome, codeterm_types::plan::CuratorOutcome::Failed);
    assert_eq!(round.reason, CuratorFailure::LaunchFailed.as_str());
}

#[test]
fn curator_notify_refuses_before_creating_an_owner_question() {
    use crate::notification::push::NotifyLevel;
    use crate::service::notification::{NotificationOrigin, PushIntent};
    use crate::service::test_fixture::ServiceFixture;
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.role = Some(crate::multiplexer::types::AgentRole::Manager);
    tab.role_profile = Some("curator".into());
    tab.agent_type = Some("fixture-provider".into());
    let fixture = ServiceFixture::new(mux);
    assert!(fixture.service.set_ext(CuratorTabs::default()).is_ok());
    let mut plan = Plan::new(
        "Question routing".into(),
        fixture.temp.path().display().to_string(),
        crate::plan::OrchestratorRef::from_tab("opaque-owner"),
    );
    plan.team_id = Some("fixture-team".into());
    plan.control_loop.running = Some(CuratorRun::default());
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    fixture
        .service
        .ext::<CuratorTabs>()
        .0
        .write()
        .insert(tab_id.clone(), plan.id.clone());
    let before = fixture.service.notif.lock().list(false).len();
    let error = fixture
        .service
        .create_agent_notification_reported(
            "opaque-question".into(),
            None,
            None,
            None,
            None,
            NotificationOrigin::local_tab(tab_id),
            PushIntent {
                level: Some(NotifyLevel::ActionNeeded),
                event_key: None,
            },
            None,
        )
        .unwrap_err();
    assert!(error.contains(ask::command("opaque-owner").as_str()));
    assert_eq!(fixture.service.notif.lock().list(false).len(), before);
    let run = fixture
        .service
        .plan_store
        .get(&plan.id)
        .unwrap()
        .control_loop
        .running
        .unwrap();
    assert!(!run.asked);
    assert!(!run.question_pending);
}

#[test]
fn curator_round_notices_reach_agent_payload_once_with_system_frames() {
    use crate::service::test_fixture::ServiceFixture;
    use parking_lot::Mutex;
    use std::sync::Arc;

    for (envelope, context) in [
        (curator_started_envelope(7), "curator:round_started:7"),
        (curator_envelope(), "curator"),
    ] {
        let (mux, target) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        drop(fixture.bind_recorded_agent_recipient(&target, "own-session", "own-nonce"));
        let writes = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&writes);
        let _boundary = crate::service::agent_delivery_outbox::test_boundary::Boundary::install(
            &target,
            Arc::new(move |payload| {
                captured.lock().push(payload.transport.clone());
                Ok(())
            }),
            Arc::new(|| {}),
        );
        let body = "opaque round payload";
        fixture
            .service
            .deliver_cron_report(&target, envelope, body)
            .unwrap();
        let writes = writes.lock();
        assert_eq!(writes.len(), 1);
        let text = writes[0].to_message();
        let parsed = codeterm_wire::domios::parse_message(&text).unwrap();
        assert_eq!(parsed.attr("from"), Some("system"));
        assert_eq!(parsed.attr("context"), Some(context));
        assert_eq!(parsed.body, body);
    }
}

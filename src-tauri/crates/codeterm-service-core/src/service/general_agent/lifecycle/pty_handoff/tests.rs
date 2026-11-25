use super::*;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::{
    general_agent::{ensure_general_workspace, GENERAL_ALIAS},
    message_gateway::DeliverySeam,
    messenger::MessageEnvelope,
    test_fixture::ServiceFixture,
};

fn main_with_chat_route(agent: &str, session: Option<&str>) -> (ServiceFixture, Tab) {
    let mut mux = MultiplexerState::empty();
    ensure_general_workspace(&mut mux, true);
    let id = mux.resolve_tab_id(GENERAL_ALIAS).unwrap();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "structured".into(),
        session_id: "old-thread".into(),
    };
    tab.agent_type = Some(agent.into());
    tab.agent_session_id = session.map(str::to_owned);
    tab.agent_runtime_state = Some(serde_json::json!({"state": "ready"}));
    tab.agent_runtime_config = Some(serde_json::json!({"model": "old-model"}));
    let source = tab.clone();
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(MockProvider::with_session(
            "terminal",
            "owned-session",
        )));
    (fixture, source)
}

#[test]
fn changed_general_provider_starts_the_pty_with_no_foreign_session() {
    let (fixture, source) = main_with_chat_route("structured", Some("old-thread"));
    let svc = &fixture.service;
    let target = prepare(svc, &source, "terminal", true, "cwd").unwrap();
    assert_eq!(target, None);
    let mux = svc.mux.read();
    let tab = mux.get_tab(&source.id).unwrap();
    assert_eq!(tab.backing, TabBacking::Pty);
    assert!(tab.agent_type.is_none() && tab.agent_session_id.is_none());
    assert!(tab.agent_runtime_state.is_none() && tab.agent_runtime_config.is_none());
    assert!(tab.agent_booting);
}

#[test]
fn mixed_main_resumes_its_verified_pty_session_and_routes_a_mesh_message_once() {
    let (fixture, source) = main_with_chat_route("terminal", Some("owned-session"));
    let svc = &fixture.service;
    let source = {
        let mut mux = svc.mux.write();
        mux.link_pty(&source.id, "retired-pty".into());
        mux.get_tab(&source.id).unwrap().clone()
    };
    let target = prepare(svc, &source, "terminal", true, "cwd").unwrap();
    assert_eq!(target.as_deref(), Some("owned-session"));
    {
        let mut mux = svc.mux.write();
        assert!(mux.tab_id_for_pty("retired-pty").is_none());
        mux.link_pty(&source.id, format!("recorded-{}", source.id));
        let tab = mux.get_tab_mut(&source.id).unwrap();
        assert_eq!(tab.agent_session_id, target);
        tab.session_binding_nonce = Some("new-launch".into());
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
        tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
        tab.agent_booting = false;
    }
    let writes = fixture.record_agent_deliveries(&source.id);
    svc.deliver_enveloped(
        &source.id,
        MessageEnvelope::tab("sender", "sender-tab").mesh(Some("peer".into())),
        "handoff payload",
    )
    .unwrap();
    assert_eq!(writes.texts.lock().as_slice(), ["handoff payload"]);
}

#[test]
fn unverified_mixed_session_refuses_before_retiring_its_source() {
    let (fixture, source) = main_with_chat_route("terminal", Some("unowned-session"));
    let result = prepare(&fixture.service, &source, "terminal", true, "cwd");
    assert!(result.unwrap_err().contains("could not verify"));
    assert!(same_binding(
        &source,
        fixture.service.mux.read().get_tab(&source.id).unwrap()
    ));
}

#[test]
fn mixed_main_without_a_recorded_session_prepares_a_fresh_pty() {
    let (fixture, source) = main_with_chat_route("terminal", None);
    let svc = &fixture.service;
    assert_eq!(
        prepare(svc, &source, "terminal", true, "cwd").unwrap(),
        None
    );
    let mux = svc.mux.read();
    let tab = mux.get_tab(&source.id).unwrap();
    assert_eq!(tab.backing, TabBacking::Pty);
    assert!(tab.agent_session_id.is_none() && tab.agent_booting);
}

#[test]
fn fresh_request_survives_a_failed_handoff_and_skips_the_resumable_session() {
    let (fixture, source) = main_with_chat_route("terminal", Some("owned-session"));
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("force-fresh-session");
    std::fs::write(&marker, b"fresh").unwrap();
    let attempt = |snapshot: &Tab| {
        state::prepare_with_fresh_session_requirement(&marker, |force_fresh| {
            prepare(&fixture.service, snapshot, "terminal", !force_fresh, "cwd")
        })
    };

    let mut stale = source.clone();
    stale.session_binding_nonce = Some("superseded".into());
    assert!(attempt(&stale).is_err());
    assert!(marker.is_file());
    assert!(same_binding(
        &source,
        fixture.service.mux.read().get_tab(&source.id).unwrap()
    ));

    assert_eq!(attempt(&source).unwrap(), None);
    assert!(!marker.try_exists().unwrap());
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&source.id).unwrap();
    assert_eq!(tab.backing, TabBacking::Pty);
    assert!(tab.agent_session_id.is_none());
}

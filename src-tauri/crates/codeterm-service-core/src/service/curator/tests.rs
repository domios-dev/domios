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

use super::*;
use crate::service::general_agent::{ensure_general_workspace, GENERAL_ALIAS, MAIN_ALIAS};

fn fixture() -> (
    MultiplexerState,
    crate::config::schema::GeneralAgentConfig,
    FreshGeneralSessionRequest,
) {
    let mut mux = MultiplexerState::empty();
    ensure_general_workspace(&mut mux, false);
    let config = crate::config::schema::GeneralAgentConfig {
        agent_provider: "community-provider".into(),
        first_run_complete: true,
        ..Default::default()
    };
    let request = FreshGeneralSessionRequest {
        workspace_id: crate::service::general_agent::general_workspace_id(&mux).unwrap(),
        provider_id: config.agent_provider.clone(),
        cwd: crate::service::general_agent::general_dir()
            .to_string_lossy()
            .to_string(),
        role: crate::service::general_agent::GENERAL_AGENT_KIND.into(),
        intent: "fresh_general".into(),
        model: None,
        reasoning_effort: None,
        preset: None,
    };
    (mux, config, request)
}

#[test]
fn fresh_general_selection_preserves_main_aliases_binding_and_focus() {
    let (mut mux, config, request) = fixture();
    let main_id = mux.resolve_tab_id(GENERAL_ALIAS).unwrap();
    let main = mux.get_tab_mut(&main_id).unwrap();
    main.agent_type = Some("community-provider".into());
    main.agent_session_id = Some("main-owned-session".into());
    main.session_binding_nonce = Some("main-binding".into());
    main.pty_id = Some("main-pty".into());
    let before = serde_json::to_value(&*main).unwrap();
    let selection = mux.snapshot();

    validate_request(&mux, &config, &request).unwrap();
    let fresh = create_fresh_tab(&mut mux, &request.workspace_id, true).unwrap();
    assert_ne!(fresh, main_id);
    let tab = mux.get_tab(&fresh).unwrap();
    assert!(crate::service::general_agent::is_general_agent(tab));
    assert_eq!(tab.cwd.as_deref(), Some(request.cwd.as_str()));
    assert_eq!(tab.provisioned_cwd, tab.cwd);
    assert_eq!(tab.agent_session_id, None);
    assert_eq!(tab.pty_id, None);
    assert_eq!(tab.session_binding_nonce, None);
    assert_eq!(tab.team_id, None);
    assert_eq!(tab.parent_tab_id, None);
    assert!(!tab.protected);
    assert!(tab.chat_view);
    assert!(crate::service::general_agent::confine_general_agent_kind(&mut mux).is_empty());
    assert_eq!(mux.resolve_tab_id(GENERAL_ALIAS), Some(main_id.clone()));
    assert_eq!(mux.resolve_tab_id(MAIN_ALIAS), Some(main_id.clone()));
    assert_eq!(
        serde_json::to_value(mux.get_tab(&main_id).unwrap()).unwrap(),
        before
    );
    assert_eq!(mux.snapshot().focused_tab_id, selection.focused_tab_id);
    assert_eq!(
        mux.get_workspace(&request.workspace_id)
            .unwrap()
            .active_tab_id,
        Some(main_id)
    );
}

#[test]
fn sanctioned_general_session_survives_durable_snapshot_restore() {
    let (mut mux, _, request) = fixture();
    let fresh = create_fresh_tab(&mut mux, &request.workspace_id, false).unwrap();
    mux.get_tab_mut(&fresh).unwrap().cwd = Some("changed-working-directory".into());
    let mut restored = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
    ensure_general_workspace(&mut restored, false);
    assert!(crate::service::general_agent::confine_general_agent_kind(&mut restored).is_empty());
    let tab = restored.get_tab(&fresh).unwrap();
    assert!(crate::service::general_agent::is_general_agent(tab));
    assert_eq!(tab.provisioned_cwd.as_deref(), Some(request.cwd.as_str()));
    assert_eq!(tab.cwd.as_deref(), Some("changed-working-directory"));
}

#[test]
fn fresh_general_launch_accepts_provider_selection_but_rejects_project_and_cwd_overrides() {
    let (mut mux, config, mut request) = fixture();
    let original = request.workspace_id.clone();
    request.workspace_id = mux.create_workspace("Project".into(), None).id;
    assert!(validate_request(&mux, &config, &request).is_err());
    request.workspace_id = original;
    request.provider_id = "foreign-provider".into();
    assert!(validate_request(&mux, &config, &request).is_ok());
    request.provider_id = String::new();
    assert!(validate_request(&mux, &config, &request).is_err());
    request.provider_id = config.agent_provider.clone();
    request.cwd = "foreign-home".into();
    assert!(validate_request(&mux, &config, &request).is_err());
}

#[test]
fn fresh_general_launch_requires_completed_setup_and_explicit_intent() {
    let (mux, mut config, mut request) = fixture();
    config.first_run_complete = false;
    assert!(validate_request(&mux, &config, &request).is_err());
    config.first_run_complete = true;
    config.enabled = false;
    assert!(validate_request(&mux, &config, &request).is_err());
    config.enabled = true;
    request.intent = "resume".into();
    assert!(validate_request(&mux, &config, &request).is_err());
    request.intent = "fresh_general".into();
    request.role = "orchestrator".into();
    assert!(validate_request(&mux, &config, &request).is_err());
}

#[test]
fn fresh_general_request_deserializes_chosen_launch_settings() {
    let request: FreshGeneralSessionRequest = serde_json::from_value(serde_json::json!({
        "workspaceId": "general", "providerId": "selected-provider", "cwd": "general-home",
        "role": "general_agent", "intent": "fresh_general", "model": "chosen-model",
        "reasoningEffort": "high", "preset": "owned-preset"
    }))
    .unwrap();
    assert_eq!(request.provider_id, "selected-provider");
    assert_eq!(request.model.as_deref(), Some("chosen-model"));
    assert_eq!(request.reasoning_effort.as_deref(), Some("high"));
    assert_eq!(request.preset.as_deref(), Some("owned-preset"));
}

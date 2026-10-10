use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use std::sync::Arc;

fn fixture() -> (ServiceFixture, String, String) {
    let mut mux = MultiplexerState::empty();
    super::super::workspace::ensure_general_workspace(&mut mux, true);
    let tab = mux.resolve_tab_id(super::super::GENERAL_ALIAS).unwrap();
    let fixture = ServiceFixture::new(mux);
    let provider = format!("structured-ga-{}", uuid::Uuid::new_v4());
    let mut manifest: PluginManifest = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plugins/provider-codex/plugin.json"
    )))
    .unwrap();
    manifest.id = provider.clone();
    let vm = PluginVm::from_source(
        &provider,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../plugins/provider-codex/plugin.js"
        ))
        .into(),
    )
    .unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(manifest, Arc::new(vm))));
    (fixture, tab, provider)
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn general_agent_fresh_and_resumed_launch_plans_select_structured_control() {
    let (fixture, tab, provider) = fixture();
    let svc = &fixture.service;
    let params = AgentLaunchParams {
        task: Some("opaque-general-instructions-481".into()),
        skip_permissions: true,
        starter_prompt: StarterPrompt::NoStarter,
        ..Default::default()
    };
    for resume in [None, Some("owned-session-39")] {
        let selected = plan(svc, &tab, &provider, "owned-cwd", &params, resume).unwrap();
        assert_eq!(selected.provider, provider);
        assert_eq!(selected.cwd, "owned-cwd");
        assert_eq!(selected.instructions, params.task);
        assert_eq!(selected.resume.as_deref(), resume);
        assert!(selected.full_access);
    }
    assert!(starter(svc, &provider, StarterPrompt::NoStarter).is_none());
    assert!(starter(svc, &provider, StarterPrompt::AssignedTask).is_some());
    let plain = "plain-ga-fixture";
    svc.agent_registry
        .register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
            plain,
        )));
    assert!(plan(svc, &tab, plain, "owned-cwd", &params, None).is_none());
    svc.mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .structured_control_active = true;
    assert!(command(svc, &tab, plain, "owned-cwd", &mut params.clone(), None).is_some());
    assert!(
        !svc.mux
            .read()
            .get_tab(&tab)
            .unwrap()
            .structured_control_active
    );
    assert_eq!(
        codex_rpc::lifecycle::delivery_state(&tab),
        codex_rpc::lifecycle::DeliveryState::Plain
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn fresh_general_model_policy_preserves_structured_launch_selection() {
    let (fixture, tab, provider) = fixture();
    let params = AgentLaunchParams {
        task: Some("opaque-general-contract".into()),
        args: Some(vec!["--model".into(), "configured-model".into()]),
        ..Default::default()
    };
    let selection = codex_rpc::launch::Selection {
        model: Some("configured-model".into()),
        ..Default::default()
    };
    let selected = codex_rpc::launch::selection_plan(
        &fixture.service,
        &tab,
        &provider,
        "general-home",
        &params,
        selection,
    )
    .unwrap();
    assert_eq!(selected.model.as_deref(), Some("configured-model"));
    assert_eq!(selected.cwd, "general-home");
    assert_eq!(selected.instructions, params.task);
    assert_eq!(selected.resume, None);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn general_agent_structured_credentials_precede_provider_identity_and_preserve_nonce() {
    let (fixture, tab, provider) = fixture();
    let svc = &fixture.service;
    let nonce = svc.rotate_session_binding_nonce(&tab);
    assert!(svc.mux.read().get_tab(&tab).unwrap().agent_type.is_none());
    let mut before = std::collections::HashMap::new();
    codex_rpc::lifecycle::launch_env_for_provider(svc, &tab, &provider, &mut before);
    let token = before.get("CODETERM_CODEX_RPC_TOKEN").unwrap();
    assert!(!token.is_empty());
    svc.mux.write().get_tab_mut(&tab).unwrap().agent_type = Some(provider);
    let mut after = std::collections::HashMap::new();
    codex_rpc::lifecycle::launch_env(svc, &tab, &mut after);
    assert_eq!(before, after);
    assert_eq!(svc.ensure_session_binding_nonce(&tab), nonce);
    codex_rpc::lifecycle::stop_tab(&tab);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn general_agent_comparison_ordinary_restore_defers_fresh_and_resumed_structured_launch() {
    let (fixture, tab, provider) = fixture();
    let svc = &fixture.service;
    for resume in [None, Some("restored-owned-session")] {
        let input = crate::service::restored_launch::restored_fresh_launch_input(
            &svc.agent_registry,
            crate::service::SpawnAgentParams {
                agent: Some(provider.clone()),
                cwd: Some("restored-cwd".into()),
                resume_session_id: resume.map(str::to_owned),
                ..Default::default()
            },
            Default::default(),
        );
        assert!(input.command(svc, &tab).is_some());
        let deferred = codex_rpc::launch::deferred_params(&tab).unwrap();
        assert_eq!(deferred.agent.as_deref(), Some(provider.as_str()));
        assert_eq!(deferred.cwd.as_deref(), Some("restored-cwd"));
        assert_eq!(deferred.resume_session_id.as_deref(), resume);
        assert_eq!(deferred.starter_prompt, StarterPrompt::NoStarter);
        codex_rpc::launch::clear_tab(&tab);
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn restored_general_sibling_and_main_share_typed_model_rpc_admission() {
    let (fixture, main, provider) = fixture();
    let svc = &fixture.service;
    let model_args = svc
        .agent_registry
        .with_provider(&provider, |provider| {
            provider.model_launch_args("gpt-6.1-sol")
        })
        .unwrap();
    let sibling = {
        let mut mux = svc.mux.write();
        let workspace = mux.find_workspace_for_tab(&main).unwrap();
        let sibling = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&sibling).unwrap();
        tab.role = Some(crate::multiplexer::types::AgentRole::Manager);
        tab.role_profile = Some("general_agent".into());
        sibling
    };
    for tab in [&main, &sibling] {
        let input = crate::service::restored_launch::restored_fresh_launch_input(
            &svc.agent_registry,
            crate::service::SpawnAgentParams {
                agent: Some(provider.clone()),
                cwd: Some("general-home".into()),
                resume_session_id: Some("owned-general-session".into()),
                role: Some("Manager".into()),
                role_profile: Some("general_agent".into()),
                ..Default::default()
            },
            crate::multiplexer::types::RestorableLaunchParameters {
                captured: true,
                model: Some("gpt-6.1-sol".into()),
                args: Some(model_args.clone()),
                identity_instructions: Some("opaque-general-contract".into()),
                ..Default::default()
            },
        );
        assert!(input.command(svc, tab).is_some());
        let deferred = codex_rpc::launch::deferred_params(tab)
            .expect("restored General model arguments must not disable RPC");
        assert!(deferred.args.is_none());
        assert_eq!(deferred.model.as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(
            deferred.resume_session_id.as_deref(),
            Some("owned-general-session")
        );
        assert_eq!(deferred.role_profile.as_deref(), Some("general_agent"));
        codex_rpc::launch::clear_tab(tab);
    }
}

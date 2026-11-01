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
        "/../../../plugins/provider-codex/plugin.json"
    )))
    .unwrap();
    manifest.id = provider.clone();
    let vm = PluginVm::from_source(
        &provider,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../plugins/provider-codex/plugin.js"
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
fn restored_main_plain_launch_registers_exact_resume_authority_before_binding() {
    let (fixture, main, _) = fixture();
    let svc = &fixture.service;
    let provider = "plain-restored-main";
    let session = "main-owned-session";
    svc.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session(provider, session),
    ));
    let nonce = svc.rotate_session_binding_nonce(&main);
    {
        let mut mux = svc.mux.write();
        let tab = mux.get_tab_mut(&main).unwrap();
        tab.agent_type = Some(provider.into());
        tab.agent_session_id = Some(session.into());
    }
    let mut params = AgentLaunchParams::default();
    assert!(command(
        svc,
        &main,
        provider,
        "general-home",
        &mut params,
        Some(session)
    )
    .is_some());
    assert_eq!(
        crate::service::launch_session_detection::registered_launch_marker(&main).as_deref(),
        Some(session),
    );
    assert!(svc.launch_has_resume_target(&main));
    assert!(!svc
        .mux
        .read()
        .get_tab(&main)
        .unwrap()
        .launch_session_authenticated());
    let claim = crate::service::hook_session_binding::bind_detected_session(
        &svc.mux,
        &svc.agent_registry,
        &main,
        provider,
        session,
        &nonce,
    )
    .unwrap();
    assert!(claim.nonce_verified);
    assert!(svc
        .mux
        .read()
        .get_tab(&main)
        .unwrap()
        .launch_session_authenticated());

    assert!(command(svc, &main, provider, "general-home", &mut params, None).is_some());
    assert!(!svc.launch_has_resume_target(&main));
    assert_ne!(
        crate::service::launch_session_detection::registered_launch_marker(&main).as_deref(),
        Some(session),
    );
    crate::service::launch_session_detection::clear_launch_marker(&main);
}

#[test]
fn isolated_general_launch_uses_daemon_socket_and_custom_api_without_rebinding_tab() {
    let mut app = crate::config::schema::AppConfig::default();
    app.api.port = 7798;
    let config = Arc::new(parking_lot::Mutex::new(app));
    let env = general_agent_env_vars(
        "verification-general",
        "owned-pad".into(),
        "opaque-binding".into(),
        &config,
        Some("opaque-tab-credential".into()),
    );
    assert_eq!(
        env["CODETERM_SOCKET_PATH"],
        crate::paths::socket_display_path()
    );
    assert_eq!(env["CODETERM_API_PORT"], "7798");
    assert_eq!(env["CODETERM_TAB_ID"], "verification-general");
    assert_eq!(env["CODETERM_SESSION_BINDING_NONCE"], "opaque-binding");
    assert_eq!(env["CODETERM_API_TOKEN"], "opaque-tab-credential");
    assert_eq!(env["CODETERM_MACHINE_AGENT_IDENTITY"], "general_agent");
}

use super::test_support::register_neighbor_runtime;
use super::*;

#[test]
fn permission_mode_never_broadens_an_explicit_false_or_an_unapproved_request() {
    assert_eq!(
        authorized_mode(Some(false), true).unwrap(),
        "auto-accept-edits"
    );
    assert_eq!(authorized_mode(Some(true), true).unwrap(), "full-access");
    assert!(authorized_mode(Some(true), false).is_err());
    assert!(authorized_mode(None, false).is_err());
}

#[test]
fn writable_roots_require_existing_directories_and_deduplicate_canonical_authority() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
    let validated = validate_host_directories(vec![root.clone(), root.join(".")]).unwrap();
    assert_eq!(validated.len(), 1);
    assert_eq!(
        validated[0],
        codeterm_paths::strip_verbatim_prefix(root.canonicalize().unwrap().to_str().unwrap())
    );
    assert!(validate_host_directories(vec![root.join("missing")]).is_err());
    let file = root.join("file");
    std::fs::write(&file, "opaque").unwrap();
    assert!(validate_host_directories(vec![file]).is_err());
    assert!(validate_host_directories(vec![PathBuf::from("relative")]).is_err());
}

#[test]
fn saved_options_round_trip_preserves_directories_and_explicit_network_denial() {
    let options = RestorableRuntimeLaunchOptions {
        provider_id: "fixture-provider".into(),
        binding_nonce: "fixture-nonce".into(),
        mode: "auto-accept-edits".into(),
        additional_directories: vec![std::env::current_dir().unwrap().to_str().unwrap().into()],
        network_access: Some(false),
        full_access_mode: Some("opaque-mode".into()),
    };
    let saved = serde_json::to_vec(&options).unwrap();
    let restored: RestorableRuntimeLaunchOptions = serde_json::from_slice(&saved).unwrap();
    assert_eq!(restored, options);
    assert_eq!(runtime_settings(&restored).network_access, Some(false));
    assert_eq!(
        runtime_settings(&restored).additional_directories,
        options.additional_directories
    );
}

#[test]
fn fresh_full_access_spawn_carries_its_profile_mode_before_the_tab_config_names_the_profile() {
    use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("pad-runtime".into());
    tab.session_binding_nonce = Some("nonce".into());
    let directory = tempfile::tempdir().unwrap();
    tab.cwd = Some(directory.path().to_string_lossy().into_owned());
    assert!(tab.agent_runtime_config.is_none());
    let fixture = ServiceFixture::new(mux);
    fixture.install_seeded_storage_desk();
    register_neighbor_runtime(&fixture.service);
    let svc = &fixture.service;
    for (full_access, expected) in [(true, Some("opaque-approve-all")), (false, None)] {
        let params = SpawnAgentParams {
            agent: Some("pad-runtime".into()),
            launch_profile: Some("approve-profile".into()),
            skip_permissions: Some(full_access),
            permission_bypass_authorization: permission_policy::authorize_permission_bypass(
                &svc.agent_registry,
                "pad-runtime",
                full_access,
            ),
            ..Default::default()
        };
        svc.prepare_agent_runtime_launch_options(&id, &params)
            .unwrap();
        let mux = svc.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        let options = tab.restorable_launch.runtime_launch_options.as_ref();
        assert_eq!(
            options.unwrap().full_access_mode.as_deref(),
            expected,
            "{full_access}"
        );
        assert_eq!(
            tab.agent_runtime_config.as_ref().unwrap()["fullAccessMode"].as_str(),
            expected,
            "{full_access}"
        );
    }
}

fn host_approval_fixture(profile: &str) -> (crate::service::test_fixture::ServiceFixture, String) {
    use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("host-runtime".into());
    tab.session_binding_nonce = Some("nonce".into());
    tab.cwd = Some(
        crate::test_support::fixture_manifest_dir()
            .to_string_lossy()
            .into_owned(),
    );
    tab.agent_runtime_config = Some(serde_json::json!({"launchProfile":profile}));
    tab.restorable_launch.captured = true;
    tab.restorable_launch.permission_bypass_requested = false;
    let fixture = ServiceFixture::new(mux);
    fixture.install_seeded_storage_desk();
    let manifest = PluginManifest::from_json_str(&serde_json::json!({
        "id":"host-runtime","version":"1.0.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"hostApprovals":true,"runtime":"stdio-jsonrpc","command":{"binary":"fixture-cli","args":[]},
                "launchProfiles":{"plain":{"binary":"fixture-cli","args":[]},
                    "native":{"binary":"fixture-cli","args":[],"fullAccessMode":"opaque-approve-all"}},
                "requestMethods":{"initialize":["handshake"]},"notificationMethods":[]}},
        "permissions":{"subprocess":{"allow":["fixture-cli"]}}
    }).to_string()).unwrap();
    let vm = std::sync::Arc::new(
        PluginVm::from_source("host-runtime", "module.exports={}".into()).unwrap(),
    );
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(manifest, vm)));
    (fixture, id)
}

fn restored_state(
    svc: &ServiceContext,
    id: &str,
) -> (bool, bool, serde_json::Value, serde_json::Value) {
    restore(svc, id).unwrap();
    let mux = svc.mux.read();
    let tab = mux.get_tab(id).unwrap();
    let config = tab.agent_runtime_config.as_ref().unwrap();
    (
        tab.restorable_launch.permission_bypass_requested,
        tab.auto_approve,
        config["hostApprovals"].clone(),
        config.get("fullAccessMode").cloned().unwrap_or_default(),
    )
}

#[test]
fn a_legacy_launch_record_restores_without_questions_and_the_owner_choice_survives_restore() {
    let (fixture, id) = host_approval_fixture("plain");
    let svc = &fixture.service;
    assert_eq!(
        restored_state(svc, &id),
        (
            true,
            true,
            serde_json::json!("allow_all"),
            serde_json::Value::Null
        ),
        "a false record from an older launch is not an owner choice"
    );
    assert!(svc.effective_full_access(&id));

    assert!(!svc.set_tab_host_approvals(&id, false).unwrap());
    assert!(!svc.effective_full_access(&id));
    let saved =
        serde_json::to_value(&svc.mux.read().get_tab(&id).unwrap().restorable_launch).unwrap();
    assert_eq!(saved["host_approval_choice"], "ask");
    let reloaded: crate::multiplexer::types::RestorableLaunchParameters =
        serde_json::from_value(saved).unwrap();
    svc.mux.write().get_tab_mut(&id).unwrap().restorable_launch = reloaded;
    assert_eq!(
        restored_state(svc, &id),
        (
            false,
            false,
            serde_json::json!("ask"),
            serde_json::Value::Null
        ),
        "the owner's ask survives a restore"
    );
    assert!(!svc.get_tab_status(&id).unwrap().full_access);

    assert!(svc.set_tab_host_approvals(&id, true).unwrap());
    assert_eq!(
        restored_state(svc, &id),
        (
            true,
            true,
            serde_json::json!("allow_all"),
            serde_json::Value::Null
        ),
    );
    assert!(svc.get_tab_status(&id).unwrap().full_access);
}

#[test]
fn a_native_mode_stays_the_single_switch_and_its_choice_is_reapplied_on_restore() {
    let (fixture, id) = host_approval_fixture("native");
    let svc = &fixture.service;
    let mode_key = || {
        svc.mux
            .read()
            .get_tab(&id)
            .unwrap()
            .agent_runtime_config
            .as_ref()
            .unwrap()
            .get("sessionMode")
            .cloned()
    };
    assert!(svc.set_tab_host_approvals(&id, false).is_err());
    let unchosen = restored_state(svc, &id);
    assert_eq!(
        unchosen.3,
        serde_json::json!("opaque-approve-all"),
        "a new or legacy tab enters the approve-all mode"
    );
    assert!(unchosen.0);
    assert_eq!(mode_key(), None);

    let select = |mode: &str| {
        svc.mux
            .write()
            .get_tab_mut(&id)
            .unwrap()
            .restorable_launch
            .selected_session_mode = Some(mode.into());
    };
    select("opaque-default");
    let asking = restored_state(svc, &id);
    assert!(!asking.0, "a chosen asking mode keeps asking after restore");
    assert_eq!(asking.3, serde_json::Value::Null);
    assert_eq!(mode_key(), Some(serde_json::json!("opaque-default")));

    select("opaque-approve-all");
    let approving = restored_state(svc, &id);
    assert!(approving.0);
    assert_eq!(approving.3, serde_json::json!("opaque-approve-all"));
    assert_eq!(mode_key(), Some(serde_json::json!("opaque-approve-all")));
}

#[test]
fn an_agents_own_mode_change_neither_widens_nor_narrows_the_owners_choice() {
    let (fixture, id) = host_approval_fixture("native");
    let svc = &fixture.service;
    let live = |mode: &str| -> RuntimeSettings {
        serde_json::from_value(serde_json::json!({"mode":mode,
            "modes":[{"id":"opaque-default","name":"Default"},{"id":"opaque-approve-all","name":"Approve all"}]}))
        .unwrap()
    };
    let select = |mode: &str| {
        svc.mux
            .write()
            .get_tab_mut(&id)
            .unwrap()
            .restorable_launch
            .selected_session_mode = Some(mode.into());
    };
    select("opaque-default");
    assert!(
        !svc.full_access_with_settings(&id, Some(&live("opaque-approve-all"))),
        "an agent entering its approve-all mode must not lift the owner's asking mode"
    );
    select("opaque-approve-all");
    assert!(
        svc.full_access_with_settings(&id, Some(&live("opaque-default"))),
        "the owner's approve-all choice holds when the agent leaves that mode"
    );
}

#[test]
fn confinement_refuses_the_host_switch_and_locks_the_picker() {
    let (fixture, id) = host_approval_fixture("plain");
    let svc = &fixture.service;
    svc.mux.write().get_tab_mut(&id).unwrap().neighbor_authority =
        Some(crate::multiplexer::types::NeighborAuthority {
            uid: "owner".into(),
            friendship_id: "a".repeat(64),
            root_tab_id: None,
        });
    assert!(svc.set_tab_host_approvals(&id, true).is_err());
    assert_eq!(
        crate::service::agent_runtime::permission_policy::host_approvals_view(svc, &id, false),
        Some("locked")
    );
    assert!(!svc.effective_full_access(&id));
}

#[test]
fn undeclared_spawn_default_is_not_persisted_as_a_runtime_mode() {
    use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("pad-runtime".into());
    tab.session_binding_nonce = Some("nonce".into());
    let directory = tempfile::tempdir().unwrap();
    tab.cwd = Some(directory.path().to_string_lossy().into_owned());
    let fixture = ServiceFixture::new(mux);
    fixture.install_seeded_storage_desk();
    register_neighbor_runtime(&fixture.service);
    let params = SpawnAgentParams {
        agent: Some("pad-runtime".into()),
        skip_permissions: Some(false),
        ..Default::default()
    };
    fixture
        .service
        .prepare_agent_runtime_launch_options(&id, &params)
        .unwrap();
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert!(tab
        .agent_runtime_config
        .as_ref()
        .unwrap()
        .get("mode")
        .is_none());
    let options = tab
        .restorable_launch
        .runtime_launch_options
        .as_ref()
        .unwrap();
    assert!(options.mode.is_empty());
    assert!(runtime_settings(options).mode.is_none());
}

use super::{
    publish_backend_materialization_failure, restored_backend_tab_ids, select_web_dist,
    service_initialized, should_materialize_restored_backend,
};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::multiplexer::types::TabBacking;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use codeterm_types::pty::BootStage;
use std::sync::{mpsc, Arc};
use std::time::Duration;

#[test]
fn restored_backend_queue_excludes_failed_role_launches_even_if_thawed_by_stale_activity() {
    for provider in [None, Some("uninstalled-historical-provider")] {
        let (mut mux, id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.role = Some(codeterm_service_core::multiplexer::types::AgentRole::Orchestrator);
        tab.agent_type = provider.map(str::to_owned);
        tab.agent_session_id = provider.map(|_| "historical-session".into());
        tab.boot_stage = Some(BootStage::SpawnFailed);
        tab.boot_failure_reason = Some("captured account failure".into());
        tab.last_active_at_ms = Some(u64::MAX);
        let fixture = ServiceFixture::new(MultiplexerState::from_session_snapshot(&mux.to_session_snapshot()));
        codeterm_service_core::service::lazy_restore::initialize(&fixture.service, 48);
        fixture.service.mux.write().get_tab_mut(&id).unwrap().frozen = false;
        let before = serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap();
        let service = Arc::new(fixture.service.clone());
        assert!(!restored_backend_tab_ids(&service).contains(&id));
        publish_backend_materialization_failure(&service, &id, "must not replace captured reason");
        assert_eq!(serde_json::to_value(fixture.service.mux.read().get_tab(&id).unwrap()).unwrap(), before);
    }
}

#[test]
fn restored_backend_spawn_failure_publishes_reason_and_preserves_session() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let id = mux.create_tab(&workspace_id).unwrap();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_session_id = Some("saved-session".into());
    tab.cwd = Some("missing-lane".into());
    tab.agent_booting = true;
    tab.boot_stage = Some(BootStage::PtyStarting);
    let fixture = ServiceFixture::new(mux);
    let service = Arc::new(fixture.service.clone());

    publish_backend_materialization_failure(&service, &id, "opaque-spawn-error");

    let mux = service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
    assert_eq!(
        tab.boot_failure_reason.as_deref(),
        Some("opaque-spawn-error")
    );
    assert!(!tab.agent_booting);
    assert_eq!(tab.agent_session_id.as_deref(), Some("saved-session"));
    assert_eq!(tab.cwd.as_deref(), Some("missing-lane"));
    assert!(tab.pty_id.is_none());
}

#[test]
fn process_split_contract_service_acknowledgement_needs_no_frontend() {
    let (sender, receiver) = mpsc::channel();
    sender.send(()).unwrap();
    assert!(service_initialized(receiver));
}

#[test]
fn restore_contract_saved_backends_do_not_require_frontend_state() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.restored_from_snapshot = true;
    tab.agent_session_id = Some("saved-session".into());
    let fixture = ServiceFixture::new(mux);
    let selected = restored_backend_tab_ids(&Arc::new(fixture.service.clone()));
    assert!(selected.contains(&tab_id));
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("saved-session")
    );
}

#[test]
fn process_split_contract_disconnected_initialization_never_grants_readiness() {
    let (sender, receiver) = mpsc::channel();
    drop(sender);
    assert!(!service_initialized(receiver));
}

#[test]
fn process_split_contract_general_and_restored_backends_have_independent_acknowledgements() {
    let (general_sender, general_receiver) = mpsc::channel();
    let (restore_sender, restore_receiver) = mpsc::channel();
    let (done_sender, done_receiver) = mpsc::channel();
    std::thread::spawn(move || {
        done_sender
            .send(service_initialized(general_receiver))
            .unwrap();
    });
    general_sender.send(()).unwrap();
    assert!(done_receiver.recv_timeout(Duration::from_secs(10)).unwrap());
    drop(restore_sender);
    assert!(!service_initialized(restore_receiver));
}

fn web_bundle(parent: &std::path::Path, name: &str) -> std::path::PathBuf {
    let dist = parent.join(name);
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::write(dist.join("index.html"), name).unwrap();
    dist
}

#[test]
fn packaged_web_bundle_is_selected_without_an_explicit_override() {
    let root = tempfile::tempdir().unwrap();
    let packaged = web_bundle(root.path(), "packaged");
    let source_tree = web_bundle(root.path(), "mutable-source-tree");
    let selected = select_web_dist(false, None, [packaged.clone()]);

    assert_eq!(selected, Some(packaged));
    assert_ne!(selected, Some(source_tree));
}

#[test]
fn explicit_dev_bundle_overrides_packaged_assets() {
    let root = tempfile::tempdir().unwrap();
    let packaged = web_bundle(root.path(), "packaged");
    let dev = web_bundle(root.path(), "dev");

    assert_eq!(
        select_web_dist(true, Some(dev.clone()), [packaged]),
        Some(dev)
    );
}

#[test]
fn production_ignores_an_inherited_mutable_override() {
    let root = tempfile::tempdir().unwrap();
    let packaged = web_bundle(root.path(), "packaged");
    let inherited = web_bundle(root.path(), "inherited-dev");

    assert_eq!(
        select_web_dist(false, Some(inherited), [packaged.clone()]),
        Some(packaged),
    );
}

#[test]
fn startup_materializes_only_unprotected_restored_pty_backends() {
    assert!(!should_materialize_restored_backend(
        true,
        false,
        &TabBacking::Pty,
        None,
        true
    ));
    assert!(should_materialize_restored_backend(
        true,
        false,
        &TabBacking::Pty,
        None,
        false,
    ));
    assert!(!should_materialize_restored_backend(
        false,
        false,
        &TabBacking::Pty,
        None,
        false,
    ));
    assert!(!should_materialize_restored_backend(
        true,
        true,
        &TabBacking::Pty,
        None,
        false,
    ));
    assert!(should_materialize_restored_backend(
        true,
        false,
        &TabBacking::ChatBackend {
            plugin_id: "test".into(),
            session_id: "session".into(),
        },
        None,
        false,
    ));
    assert!(!should_materialize_restored_backend(
        true,
        false,
        &TabBacking::Pty,
        Some("live"),
        false,
    ));
}

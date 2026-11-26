use super::{
    await_remote_launch_outcome, classify_spawn_failure, create_lane_bundle_with_limit,
    lane_bundle_size_allowed, preflight_remote_worker_provider, MeshWorkerLaneBundleRequest,
    MAX_LANE_BUNDLE_BYTES,
};
use crate::routes::AppState;
use codeterm_service_core::mesh::types::AccessLevel;
use codeterm_service_core::service::test_fixture::ServiceFixture;

mod bundle_range;

fn app_state(fixture: &ServiceFixture) -> crate::routes::AppState {
    let config = codeterm_service_core::config::schema::AppConfig::default();
    let mut mesh = codeterm_service_core::mesh::MeshManager::new("bundle-origin".into(), 0);
    mesh.insert_account_peer_for_test(codeterm_service_core::mesh::types::PairedPeer {
        instance_id: "peer-origin".into(),
        instance_name: "Origin".into(),
        addresses: vec![],
        api_port: 0,
        public_key: String::new(),
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::Full,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: 0,
        consecutive_misses: 0,
        last_active_ms: None,
    });
    crate::routes::AppState {
        frontend_dev_url: None,
        svc: std::sync::Arc::new(fixture.service.clone()),
        cron: std::sync::Arc::new(parking_lot::Mutex::new(
            codeterm_service_core::cron::store::CronStore::new_with_path(
                fixture.temp.path().join("worker-bundle-crons.json"),
            ),
        )),
        cron_wake: Default::default(),
        api_port: 0,
        update_state: std::sync::Arc::new(parking_lot::Mutex::new(codeterm_update::UpdateState {
            install_method: codeterm_update::InstallMethod::Unknown,
            last_check: None,
            last_check_time: None,
            channel: config.updater.channel,
            phase: codeterm_update::UpdatePhase::Idle,
            last_error: None,
            phase_changed_at: None,
            downloaded_bytes: 0,
            download_total_bytes: None,
        })),
        config: std::sync::Arc::new(parking_lot::Mutex::new(config)),
        mesh: std::sync::Arc::new(parking_lot::Mutex::new(mesh)),
        keepawake: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        resize_owner: std::sync::Arc::new(parking_lot::Mutex::new(Default::default())),
        rest_api: std::sync::Arc::new(codeterm_service_core::startup::RestApiControl::new()),
        test_clock: None,
        events: fixture.service.events.clone(),
        web_dist: None,
    }
}

fn git(cwd: &std::path::Path, args: &[&str]) -> String {
    let mut command = codeterm_util::process_util::git_command();
    let output = command.current_dir(cwd).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn bundle_fixture(
    peer_target: &str,
) -> (
    AppState,
    MeshWorkerLaneBundleRequest,
    String,
    tempfile::TempDir,
    ServiceFixture,
) {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.name", "Worker"]);
    git(&repo, &["config", "user.email", "worker@example.test"]);
    git(
        &repo,
        &["remote", "add", "origin", "https://example.test/repo.git"],
    );
    std::fs::write(repo.join("base.txt"), "base").unwrap();
    git(&repo, &["add", "base.txt"]);
    git(&repo, &["commit", "-qm", "base"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["switch", "-qc", "lane/remote-test"]);
    std::fs::write(repo.join("lane.txt"), "lane").unwrap();
    git(&repo, &["add", "lane.txt"]);
    git(&repo, &["commit", "-qm", "lane"]);
    let tip = git(&repo, &["rev-parse", "HEAD"]);
    let (mut mux, tab_id) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    if let Some(cwd) = mux.get_tab_mut(&tab_id) {
        cwd.cwd = Some(repo.display().to_string());
        cwd.provisioned_cwd = Some(repo.display().to_string());
        cwd.task_binding = Some(codeterm_service_core::multiplexer::types::TaskBinding {
            plan_id: Some("plan-1".into()),
            task_id: Some("task-1".into()),
            attempt_id: Some("attempt-1".into()),
            spawn_handle: None,
            bound_at: 1,
        });
        cwd.report_target_mesh_peer = Some(peer_target.into());
    }
    let fixture = ServiceFixture::new(mux);
    let state = app_state(&fixture);
    let identity = codeterm_service_core::service::worker_lane::legacy_repository_identity(
        repo.to_str().unwrap(),
    )
    .unwrap();
    let request = MeshWorkerLaneBundleRequest {
        repository_identity: identity,
        repository_identities: vec![],
        branch: "lane/remote-test".into(),
        base_commit: base,
        plan_id: "plan-1".into(),
        task_id: "task-1".into(),
        attempt_id: "attempt-1".into(),
        report_sha: tip.clone(),
    };
    (state, request, tip, root, fixture)
}

#[test]
fn worker_dispatch_status_recovers_spawn_and_refuses_foreign_attempt() {
    let (state, lane, _, _root, fixture) = bundle_fixture("peer-origin");
    let tab_id = {
        let mut mux = fixture.service.mux.write();
        let tab_id = mux.all_tabs().next().unwrap().id.clone();
        mux.get_tab_mut(&tab_id).unwrap().session_binding_nonce = Some("worker-nonce".into());
        tab_id
    };
    let mut request = super::MeshWorkerDispatchRequest {
        repository_identity: lane.repository_identity,
        repository_identities: lane.repository_identities,
        base_commit: lane.base_commit,
        plan_id: lane.plan_id,
        task_id: lane.task_id,
        attempt_id: lane.attempt_id,
        origin_tab_id: "origin".into(),
        provider: "provider".into(),
        model: None,
        reasoning_effort: None,
        speed: None,
        title: None,
        description: None,
        task_description_managed: false,
        brief: String::new(),
        clone_url: None,
    };
    let in_flight = state.svc.ext_or_init(super::InFlightDispatches::default);
    let pending = in_flight.begin("peer-origin", &request.attempt_id).unwrap();
    assert!(in_flight.begin("peer-origin", &request.attempt_id).is_err());
    assert_eq!(
        super::observed_worker_dispatch(&state, "peer-origin", &request).unwrap_err(),
        (
            axum::http::StatusCode::CONFLICT,
            super::DISPATCH_PENDING.into()
        )
    );
    drop(pending);
    let _other_peer = in_flight.begin("other-peer", &request.attempt_id).unwrap();
    let response = super::observed_worker_dispatch(&state, "peer-origin", &request)
        .unwrap()
        .0;
    assert_eq!(response["worker_tab_id"], tab_id);
    assert_eq!(response["session_binding_nonce"], "worker-nonce");
    assert_eq!(response["branch"], "lane/remote-test");
    assert_eq!(response["base_commit"], request.base_commit);
    fixture.service.emit_boot_stage(
        &tab_id,
        codeterm_types::pty::BootStage::SpawnFailed,
        Some("launch rolled back"),
    );
    assert_eq!(
        super::observed_worker_dispatch(&state, "peer-origin", &request).unwrap_err(),
        (
            axum::http::StatusCode::CONFLICT,
            "peer_worker_spawn_failed: launch rolled back".into()
        )
    );
    request.attempt_id = "foreign-attempt".into();
    assert_eq!(
        super::observed_worker_dispatch(&state, "peer-origin", &request)
            .unwrap_err()
            .0,
        axum::http::StatusCode::NOT_FOUND
    );
    state
        .mesh
        .lock()
        .paired_peers
        .get_mut("peer-origin")
        .unwrap()
        .access_level = AccessLevel::ReadOnly;
    assert_eq!(
        super::observed_worker_dispatch(&state, "peer-origin", &request)
            .unwrap_err()
            .0,
        axum::http::StatusCode::FORBIDDEN
    );
}

#[test]
fn worker_lane_bundle_size_cap_rejects_oversize_without_allocating_bundle() {
    assert!(lane_bundle_size_allowed(
        MAX_LANE_BUNDLE_BYTES,
        MAX_LANE_BUNDLE_BYTES
    ));
    assert!(!lane_bundle_size_allowed(
        MAX_LANE_BUNDLE_BYTES + 1,
        MAX_LANE_BUNDLE_BYTES
    ));
}

#[test]
fn create_lane_bundle_refuses_binding_to_foreign_origin() {
    let (state, request, _, _root, _fixture) = bundle_fixture("different-peer");
    let error =
        create_lane_bundle_with_limit(state, "peer-origin".into(), request, MAX_LANE_BUNDLE_BYTES)
            .unwrap_err();
    assert_eq!(error.0, axum::http::StatusCode::NOT_FOUND);
}

#[test]
fn create_lane_bundle_refuses_bundle_over_size_limit() {
    let (state, request, _, _root, _fixture) = bundle_fixture("peer-origin");
    let error = create_lane_bundle_with_limit(state, "peer-origin".into(), request, 1).unwrap_err();
    assert_eq!(
        error.0,
        axum::http::StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        error.1
    );
}

#[test]
fn create_lane_bundle_refuses_head_or_lane_tip_different_from_report_sha() {
    let (state, mut request, _, _root, _fixture) = bundle_fixture("peer-origin");
    request.report_sha = "unreviewed-sha".into();
    let error =
        create_lane_bundle_with_limit(state, "peer-origin".into(), request, MAX_LANE_BUNDLE_BYTES)
            .unwrap_err();
    assert_eq!(error.0, axum::http::StatusCode::CONFLICT);
}

#[test]
fn create_lane_bundle_returns_refs_for_owned_attempt() {
    let (state, request, tip, _root, _fixture) = bundle_fixture("peer-origin");
    let response =
        create_lane_bundle_with_limit(state, "peer-origin".into(), request, MAX_LANE_BUNDLE_BYTES)
            .unwrap()
            .0;
    assert_eq!(response["branch"], "lane/remote-test");
    assert_eq!(response["lane_tip"], tip);
    assert!(response["bundle"]
        .as_str()
        .is_some_and(|bundle| !bundle.is_empty()));
}

#[test]
fn remote_worker_spawn_succeeds_on_a_peer_without_a_local_orchestrator() {
    let mut mux = codeterm_service_core::multiplexer::state::MultiplexerState::empty();
    let workspace_id = mux.create_workspace("remote worker peer".into(), None).id;
    let fixture = ServiceFixture::new(mux);
    fixture.set_agent_full_access(true);
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_service_core::agent::test_provider::MockProvider::new("remote-mock")
            .with_launch_binary(codeterm_service_core::agent::test_provider::test_shell_binary()),
    ));
    let worktree = fixture.temp.path().display().to_string();
    let request = super::MeshWorkerDispatchRequest {
        repository_identity: "sha256:fixture".into(),
        repository_identities: vec![],
        base_commit: "base".into(),
        plan_id: "plan-origin".into(),
        task_id: "task-origin".into(),
        attempt_id: "attempt-origin".into(),
        origin_tab_id: "origin-orchestrator".into(),
        provider: "remote-mock".into(),
        model: None,
        reasoning_effort: None,
        speed: None,
        title: Some("Alias".into()),
        description: Some("remote task summary".into()),
        task_description_managed: true,
        brief: "remote brief".into(),
        clone_url: None,
    };
    let mut params = super::remote_worker_spawn_params(
        request.clone(),
        workspace_id.clone(),
        worktree.clone(),
        "peer-origin".into(),
        "origin",
    );
    // Brief delivery needs the desktop session store, which the fixture lacks.
    params.task = None;
    let result = fixture.service.spawn_agent_sync(params).unwrap();
    assert_eq!(result.title, "Alias");
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&result.tab_id).unwrap();
        assert_eq!(tab.title, "Alias");
        assert_eq!(tab.report_target_mesh_peer.as_deref(), Some("peer-origin"));
        let ownership = tab.ownership.as_ref().expect("stamped remote ownership");
        assert_eq!(ownership.peer_id, "peer-origin");
        assert_eq!(
            ownership.controller_tab_id.as_deref(),
            Some("origin-orchestrator")
        );
        assert_eq!(
            tab.report_target_tab_id.as_deref(),
            Some("origin-orchestrator")
        );
        assert_eq!(tab.description.as_deref(), Some("remote task summary"));
        assert_eq!(
            tab.description_origin,
            codeterm_service_core::multiplexer::types::TabDescriptionOrigin::Task
        );
        let binding = tab.task_binding.as_ref().expect("task binding");
        assert_eq!(binding.attempt_id.as_deref(), Some("attempt-origin"));
    }
    fixture.service.close_tab(&result.tab_id).unwrap();

    // Without an explicit persona, the peer allocates a real executor name, never "Remoteattemp".
    let mut unnamed = request;
    unnamed.attempt_id = "attempt_abcdef".into();
    unnamed.title = None;
    let mut unnamed_params = super::remote_worker_spawn_params(
        unnamed,
        workspace_id,
        worktree,
        "peer-origin".into(),
        "origin",
    );
    unnamed_params.task = None;
    let unnamed_result = fixture.service.spawn_agent_sync(unnamed_params).unwrap();
    assert!(
        !unnamed_result.title.starts_with("Remote"),
        "remote worker must not use truncated Remote placeholder, got {}",
        unnamed_result.title
    );
    assert!(
        unnamed_result.title.chars().all(char::is_alphabetic) && !unnamed_result.title.is_empty(),
        "unnamed remote worker should receive an executor persona name, got {}",
        unnamed_result.title
    );
    fixture.service.close_tab(&unnamed_result.tab_id).unwrap();
}

#[test]
fn remote_launch_sign_in_required_classifies_as_actionable_conflict() {
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_service_core::agent::test_provider::MockProvider::new("opaque-provider"),
    ));
    let (status, message) = classify_spawn_failure(
        &fixture.service,
        "opaque-provider",
        "ROG",
        "Runtime tab 961f8af1 degraded: Sign-in required",
        Ok(()),
    );
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    assert!(message.starts_with("peer_provider_auth_required:"));
    assert!(message.contains("opaque-provider"));
    assert!(message.contains("ROG"));

    let (status, message) = classify_spawn_failure(
        &fixture.service,
        "opaque-provider",
        "ROG",
        "StartupSupervisionFailed: missing waiter",
        Ok(()),
    );
    assert_eq!(status, axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    assert!(message.starts_with("peer_worker_spawn_failed:"));
    assert!(message.contains("StartupSupervisionFailed"));
}

#[test]
fn await_remote_launch_outcome_surfaces_ready_failure_and_signed_out_state() {
    let (mux, tab_id) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    ready_tx.send(Ok(())).unwrap();
    assert!(await_remote_launch_outcome(
        &fixture.service,
        &tab_id,
        ready_rx,
        std::time::Duration::from_millis(50),
    )
    .is_ok());

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    ready_tx
        .send(Err("Runtime tab degraded: Sign-in required".into()))
        .unwrap();
    let error = await_remote_launch_outcome(
        &fixture.service,
        &tab_id,
        ready_rx,
        std::time::Duration::from_millis(50),
    )
    .unwrap_err();
    assert!(error.contains("Sign-in required"));

    let (sender, receiver) = tokio::sync::oneshot::channel();
    drop(sender);
    assert!(await_remote_launch_outcome(
        &fixture.service,
        &tab_id,
        receiver,
        std::time::Duration::from_secs(1)
    )
    .unwrap_err()
    .contains("without a readiness result"));
    let (_sender, receiver) = tokio::sync::oneshot::channel();
    assert!(await_remote_launch_outcome(
        &fixture.service,
        &tab_id,
        receiver,
        std::time::Duration::ZERO
    )
    .unwrap_err()
    .contains("readiness deadline elapsed"));

    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .agent_runtime_state =
        Some(serde_json::json!({ "auth": "signedOut", "state": "loginPending" }));
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    drop(ready_tx);
    let error = await_remote_launch_outcome(
        &fixture.service,
        &tab_id,
        ready_rx,
        std::time::Duration::from_millis(50),
    )
    .unwrap_err();
    assert!(
        codeterm_service_core::service::agent_runtime::transport::is_sign_in_required_launch_error(
            &error
        )
    );
}

#[test]
fn preflight_remote_worker_provider_refuses_missing_binary_before_lane() {
    let mut mux = codeterm_service_core::multiplexer::state::MultiplexerState::empty();
    let workspace_id = mux.create_workspace("preflight".into(), None).id;
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_service_core::agent::test_provider::MockProvider::new("missing-bin-provider")
            .with_launch_binary("definitely-not-installed-domios-test-binary-xyz"),
    ));
    let (status, error) = preflight_remote_worker_provider(
        &fixture.service,
        "missing-bin-provider",
        &workspace_id,
        "ROG",
    )
    .unwrap_err();
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    assert!(error.starts_with("peer_worker_spawn_failed:"));
}

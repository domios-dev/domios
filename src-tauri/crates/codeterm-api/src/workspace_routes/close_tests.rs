use super::{close_preview, workspace_close_caller, CloseWorkspaceBody, WorkspaceCloseCaller};
use axum::extract::{Path, State as AxumState};
use axum::http::{HeaderMap, StatusCode};

#[test]
fn workspace_close_route_reads_force_from_body() {
    let no_headers = HeaderMap::new();
    assert_eq!(
        workspace_close_caller(
            &crate::auth::ActorHeaders::for_owner_test(no_headers.clone()),
            &CloseWorkspaceBody::default(),
            "workspace-1"
        ),
        WorkspaceCloseCaller::Unattributed,
    );
    assert_eq!(
        workspace_close_caller(
            &crate::auth::ActorHeaders::for_owner_test(no_headers.clone()),
            &CloseWorkspaceBody {
                force: false,
                confirm: Some("workspace-1".into()),
            },
            "workspace-1",
        ),
        WorkspaceCloseCaller::Unattributed,
    );
    assert_eq!(
        workspace_close_caller(
            &crate::auth::ActorHeaders::for_owner_test(no_headers.clone()),
            &CloseWorkspaceBody {
                force: true,
                confirm: Some("workspace-1".into()),
            },
            "workspace-1",
        ),
        WorkspaceCloseCaller::HumanConfirmedHeaderless,
    );
    assert_eq!(
        workspace_close_caller(
            &crate::auth::ActorHeaders::for_owner_test(no_headers.clone()),
            &CloseWorkspaceBody {
                force: true,
                confirm: Some("workspace-other".into()),
            },
            "workspace-1",
        ),
        WorkspaceCloseCaller::Unattributed,
    );
    let mut agent_headers = HeaderMap::new();
    agent_headers.insert("X-CodeTerm-Tab-Id", "agent-1".parse().unwrap());
    assert_eq!(
        workspace_close_caller(
            &crate::auth::ActorHeaders::for_owner_test(agent_headers.clone()),
            &CloseWorkspaceBody {
                force: true,
                confirm: Some("workspace-1".into()),
            },
            "workspace-1",
        ),
        WorkspaceCloseCaller::AgentTab("agent-1".into()),
    );
}

fn install_blueprints(fixture: &codeterm_service_core::service::test_fixture::ServiceFixture) {
    let storage = codeterm_storage::Storage::open(
        fixture.temp.path().join("preview-storage"),
        Default::default(),
    )
    .unwrap();
    let store = codeterm_sentinel_host::store::Store::with_storage(
        &storage,
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    let handle = codeterm_sentinel_host::scheduler::fixture_handle_with_ui(
        store,
        fixture.service.ui.clone(),
    );
    codeterm_sentinel_host::manage_fixture_ports(&fixture.service, &handle);
}

fn bearer_headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
    headers
}

fn peer_record(
    id: &str,
    public_key: String,
    access_level: codeterm_service_core::mesh::types::AccessLevel,
) -> codeterm_service_core::mesh::types::PairedPeer {
    codeterm_service_core::mesh::types::PairedPeer {
        instance_id: id.into(),
        instance_name: id.into(),
        addresses: Vec::new(),
        api_port: 0,
        public_key,
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: codeterm_service_core::mesh::types::RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    }
}

#[tokio::test]
async fn workspace_close_preview_enforces_owner_and_workspace_authority() {
    use codeterm_service_core::mesh::crown::{
        generate_peer_keypair, sign_request_with_origin, AuthenticatedOrigin,
    };
    use codeterm_service_core::mesh::types::{AccessLevel, Permission};
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;

    let (mut mux, _) = MultiplexerState::new();
    let ws = mux.create_workspace("Auth Target".into(), None).id;
    let tab = mux.create_tab(&ws).unwrap();
    mux.get_tab_mut(&tab).unwrap().session_binding_nonce = Some("tab-nonce".into());
    let fixture = ServiceFixture::new(mux);
    install_blueprints(&fixture);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());

    let (full_sk, full_pk) = generate_peer_keypair();
    let (mgr_sk, mgr_pk) = generate_peer_keypair();
    let (ro_sk, ro_pk) = generate_peer_keypair();
    let (iso_sk, iso_pk) = generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.insert_account_peer_for_test(peer_record("peer-full", full_pk, AccessLevel::Full));
        mesh.insert_account_peer_for_test(peer_record(
            "peer-mgr",
            mgr_pk,
            AccessLevel::Custom(vec![Permission::ManageWorkspaces]),
        ));
        mesh.insert_account_peer_for_test(peer_record("peer-ro", ro_pk, AccessLevel::ReadOnly));
        mesh.insert_account_peer_for_test(peer_record(
            "peer-iso",
            iso_pk,
            AccessLevel::AgentIsolated,
        ));
    }

    let expected_installation_id = state.mesh.lock().ident.identity.instance_id.clone();
    let ok = close_preview(
        AxumState(state.clone()),
        bearer_headers("owner-secret"),
        Path(ws.clone()),
    )
    .await
    .unwrap();
    assert_eq!(ok.0.workspace_id, ws);
    assert_eq!(ok.0.installation_id, expected_installation_id);

    let mut owner_with_tab = bearer_headers("owner-secret");
    owner_with_tab.insert("X-CodeTerm-Tab-Id", tab.parse().unwrap());
    assert_eq!(
        close_preview(AxumState(state.clone()), owner_with_tab, Path(ws.clone()))
            .await
            .unwrap_err()
            .0,
        StatusCode::FORBIDDEN
    );

    let tab_token = fixture
        .service
        .mint_tab_credential(&tab, "tab-nonce")
        .unwrap();
    assert_eq!(
        close_preview(
            AxumState(state.clone()),
            bearer_headers(&tab_token),
            Path(ws.clone()),
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::FORBIDDEN
    );

    for (sk, id) in [(&full_sk, "peer-full"), (&mgr_sk, "peer-mgr")] {
        let token = sign_request_with_origin(sk, id, AuthenticatedOrigin::Owner).unwrap();
        let preview = close_preview(
            AxumState(state.clone()),
            bearer_headers(&token),
            Path(ws.clone()),
        )
        .await
        .expect("authorized owner-origin peer must succeed");
        assert_eq!(preview.0.workspace_id, ws);
    }

    for (sk, id, origin) in [
        (&ro_sk, "peer-ro", AuthenticatedOrigin::Owner),
        (&iso_sk, "peer-iso", AuthenticatedOrigin::Owner),
        (&full_sk, "peer-full", AuthenticatedOrigin::Mesh),
    ] {
        let token = sign_request_with_origin(sk, id, origin).unwrap();
        assert_eq!(
            close_preview(
                AxumState(state.clone()),
                bearer_headers(&token),
                Path(ws.clone()),
            )
            .await
            .unwrap_err()
            .0,
            StatusCode::FORBIDDEN,
            "{id} {origin:?}"
        );
    }

    assert_eq!(
        close_preview(AxumState(state), HeaderMap::new(), Path(ws))
            .await
            .unwrap_err()
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn workspace_close_preview_reports_complete_facts_warnings_and_never_loads_unrelated_plans() {
    use codeterm_service_core::cron::store::CronCreateParams;
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::plan::TaskStatus;
    use codeterm_service_core::service::plan::load_audit::{
        assert_never_loaded, attempt, execution_plan, seed, start, task,
    };
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    use codeterm_types::pty::{RemoteOwnership, RemoteOwnershipTeam};
    use codeterm_types::team::Team;
    use codeterm_types::workspace_close_preview::WorkspaceCloseWarningKind;

    let (mut mux, _) = MultiplexerState::new();
    let ws = mux.create_workspace("Preview Target".into(), None).id;
    let mut local_team = Team::new(ws.clone(), 1);
    local_team.name = Some("Core Team".into());
    let local_team_id = local_team.id.clone();
    mux.upsert_team(local_team);

    let lead = mux.create_tab(&ws).unwrap();
    mux.set_orchestrator_role(&lead).unwrap();
    {
        let t = mux.get_tab_mut(&lead).unwrap();
        t.title = "Lead".into();
        t.agent_type = Some("fixture-provider".into());
        t.team_id = Some(local_team_id.clone());
    }
    let remote_member = mux.create_tab(&ws).unwrap();
    {
        let t = mux.get_tab_mut(&remote_member).unwrap();
        t.title = "RemoteWorker".into();
        t.agent_type = Some("fixture-provider".into());
        t.team_id = Some(local_team_id.clone());
        t.ownership = Some(RemoteOwnership {
            controller_title: "Remote Lead".into(),
            controller_tab_id: None,
            peer_id: "peer-remote".into(),
            peer_name: "Neighbor".into(),
            team: Some(RemoteOwnershipTeam {
                id: local_team_id.clone(),
                title: Some("Core Team".into()),
            }),
        });
    }
    let remote_team_tab = mux.create_tab(&ws).unwrap();
    {
        let t = mux.get_tab_mut(&remote_team_tab).unwrap();
        t.title = "ForeignTeamWorker".into();
        t.team_id = Some("mesh:peer-remote/remote-team".into());
    }
    let standalone_agent = mux.create_tab(&ws).unwrap();
    {
        let t = mux.get_tab_mut(&standalone_agent).unwrap();
        t.title = "SoloAgent".into();
        t.agent_type = Some("fixture-provider".into());
    }

    let fixture = ServiceFixture::new(mux);
    install_blueprints(&fixture);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());

    let mk_cron = |name: &str, team_id: Option<String>| CronCreateParams {
        name: name.into(),
        workspace_id: ws.clone(),
        team_id,
        detach_from_team: false,
        schedule: "* * * * *".into(),
        task: "check".into(),
        description: None,
        agent: Some("fixture".into()),
        model: None,
        reasoning_effort: None,
        cwd: None,
        role: None,
        skill: None,
        creator_tab_id: None,
        report_target_tab_id: None,
        enabled: Some(true),
        max_history: None,
        oneshot: None,
        jitter_max_ms: Some(0),
        tz: None,
        timeout_seconds: None,
    };
    state.cron.lock().create(mk_cron("WS Cron", None)).unwrap();
    state
        .cron
        .lock()
        .create(mk_cron("Team Cron", Some(local_team_id.clone())))
        .unwrap();

    let mut assigned = task("t-assigned", TaskStatus::InProgress, Some(&lead));
    assigned.name = "Assigned Task".into();
    assigned.execution_attempts.push(attempt("att-1", &lead));
    let mut orphan = task("t-orphan", TaskStatus::InProgress, None);
    orphan.name = "Orphan Task".into();
    let mut remote_att_task = task("t-remote", TaskStatus::InProgress, Some(&lead));
    remote_att_task.name = "Remote Attempt Task".into();
    let mut remote_att = attempt("att-rem", &lead);
    remote_att.remote_worktree_path = Some("/remote/lane".into());
    remote_att_task.execution_attempts.push(remote_att);

    let mut selected_plan = execution_plan(
        fixture.temp.path(),
        "Selected Plan Title",
        &lead,
        vec![assigned, orphan, remote_att_task],
    );
    selected_plan.team_id = Some(local_team_id.clone());
    let selected_plan_id = seed(&fixture.service.plan_store, selected_plan);

    let mut unrelated_plan = execution_plan(
        fixture.temp.path(),
        "Unrelated Plan",
        "other-lead",
        vec![task("t-other", TaskStatus::InProgress, Some("other-lead"))],
    );
    unrelated_plan.team_id = Some("other-team".into());
    let unrelated_plan_id = seed(&fixture.service.plan_store, unrelated_plan);

    start(&fixture.service.plan_store);
    let mux_before = serde_json::to_value(fixture.service.mux.read().snapshot()).unwrap();

    let preview = close_preview(
        AxumState(state.clone()),
        bearer_headers("owner-secret"),
        Path(ws.clone()),
    )
    .await
    .unwrap()
    .0;

    assert_eq!(preview.workspace_id, ws);
    assert_eq!(preview.workspace_name, "Preview Target");
    assert_eq!(preview.agent_tabs.len(), 1);
    assert_eq!(preview.agent_tabs[0].title, "SoloAgent");
    assert_eq!(preview.remote_members.len(), 1);
    assert_eq!(preview.remote_members[0].peer_id, "peer-remote");
    assert_eq!(preview.crons, vec!["WS Cron".to_string()]);
    assert_eq!(preview.teams.len(), 2);

    let team = preview
        .teams
        .iter()
        .find(|t| t.id == local_team_id)
        .unwrap();
    assert!(!team.remote);
    assert_eq!(team.crons, vec!["Team Cron".to_string()]);
    assert_eq!(team.tasks.len(), 3);
    assert!(team
        .tasks
        .iter()
        .all(|t| t.plan_title == "Selected Plan Title"));
    assert!(team
        .tasks
        .iter()
        .any(|t| t.task_id == "t-orphan" && t.orphan));
    assert!(team
        .open_attempts
        .iter()
        .any(|a| a.attempt_id == "att-1" && !a.remote));
    assert!(team
        .open_attempts
        .iter()
        .any(|a| a.attempt_id == "att-rem" && a.remote));

    let remote_team = preview
        .teams
        .iter()
        .find(|t| t.id == "mesh:peer-remote/remote-team")
        .unwrap();
    assert!(remote_team.remote);

    let warning_kinds = preview.warnings.iter().map(|w| &w.kind).collect::<Vec<_>>();
    assert!(warning_kinds.contains(&&WorkspaceCloseWarningKind::RemoteMember));
    assert!(warning_kinds.contains(&&WorkspaceCloseWarningKind::RemoteTeam));
    assert!(warning_kinds.contains(&&WorkspaceCloseWarningKind::OrphanTask));
    assert!(warning_kinds.contains(&&WorkspaceCloseWarningKind::RemoteAttempt));

    assert_never_loaded(
        &fixture.service.plan_store,
        &[&selected_plan_id, &unrelated_plan_id],
    );
    let mux_after = serde_json::to_value(fixture.service.mux.read().snapshot()).unwrap();
    assert_eq!(mux_after, mux_before);
}

#[tokio::test]
async fn workspace_close_preview_refuses_protected_missing_unavailable_and_caps_atomically() {
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;

    // Uninitialized blueprints must fail closed with 503 rather than returning a partial preview.
    let (mut mux, _) = MultiplexerState::new();
    let uninit_ws = mux.create_workspace("No Blueprints".into(), None).id;
    let uninit_fixture = ServiceFixture::new(mux);
    let uninit_state = crate::test_support::app_state(&uninit_fixture);
    uninit_state.config.lock().api.token = Some("owner-secret".into());
    assert_eq!(
        close_preview(
            AxumState(uninit_state),
            bearer_headers("owner-secret"),
            Path(uninit_ws),
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::SERVICE_UNAVAILABLE
    );

    let (mut mux, _) = MultiplexerState::new();
    let protected_ws = mux.snapshot().workspaces[0].id.clone();
    mux.get_workspace_mut(&protected_ws).unwrap().protected = true;

    let too_many_rows_ws = mux.create_workspace("Too Many Rows".into(), None).id;
    for _ in 0..4097 {
        mux.create_tab(&too_many_rows_ws).unwrap();
    }

    let too_many_bytes_ws = mux.create_workspace("Too Many Bytes".into(), None).id;
    let long_title = "a".repeat(120_000);
    for _ in 0..20 {
        let tab = mux.create_tab(&too_many_bytes_ws).unwrap();
        let entry = mux.get_tab_mut(&tab).unwrap();
        entry.agent_type = Some("fixture-provider".into());
        entry.title = long_title.clone();
    }

    let fixture = ServiceFixture::new(mux);
    install_blueprints(&fixture);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());

    assert_eq!(
        close_preview(
            AxumState(state.clone()),
            bearer_headers("owner-secret"),
            Path("missing-ws".into()),
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        close_preview(
            AxumState(state.clone()),
            bearer_headers("owner-secret"),
            Path(protected_ws),
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        close_preview(
            AxumState(state.clone()),
            bearer_headers("owner-secret"),
            Path(too_many_rows_ws),
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        close_preview(
            AxumState(state),
            bearer_headers("owner-secret"),
            Path(too_many_bytes_ws),
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

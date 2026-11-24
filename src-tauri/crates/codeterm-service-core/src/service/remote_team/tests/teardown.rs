use super::*;

async fn adopt(fixture: &Fixture, peer: &FakePeer) -> RemoteTeamMember {
    fixture
        .origin
        .service
        .reparent_remote_worker_with_peer(
            fixture.caller(),
            "remote",
            &fixture.worker,
            &fixture.destination,
            peer,
        )
        .await
        .unwrap()
}

#[test]
fn peer_authority_and_offline_decisions_preserve_the_membership_boundary() {
    use crate::mesh::types::AccessLevel;
    assert!(super::super::origin::authorize_peer_transfer(&AccessLevel::Full, true).is_ok());
    assert!(
        super::super::origin::authorize_peer_transfer(&AccessLevel::Full, false)
            .unwrap_err()
            .contains("offline")
    );
    for access in [
        AccessLevel::ReadOnly,
        AccessLevel::AgentIsolated,
        AccessLevel::Custom(Vec::new()),
    ] {
        assert!(super::super::origin::authorize_peer_transfer(&access, true).is_err());
    }
}

#[tokio::test]
async fn remote_finalization_closes_the_exact_worker_and_persists_its_terminal_receipt() {
    let fixture = Fixture::new();
    let peer = fixture.peer();
    let member = adopt(&fixture, &peer).await;
    fixture
        .origin
        .service
        .prepare_remote_team_close(&member.team_id)
        .unwrap();
    let receipt = fixture
        .origin
        .service
        .finalize_remote_team_member_with_peer(&member, true, &peer)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, "closed");
    assert!(fixture
        .remote
        .service
        .mux
        .read()
        .get_tab(&fixture.worker)
        .is_none());
    let reloaded = RemoteTeamStore::open(fixture.origin.temp.path().join("remote_teams.json"));
    assert!(reloaded.members_for_team(&member.team_id).unwrap()[0].finalized);
    fixture
        .origin
        .service
        .complete_remote_team_close(&member.team_id)
        .unwrap();
    assert!(fixture
        .origin
        .service
        .remote_teams
        .members_for_team(&member.team_id)
        .unwrap()
        .is_empty());
    assert!(fixture.origin.service.remote_teams.was_finalized(
        "remote",
        &fixture.worker,
        "fixture-provider",
        "worker"
    ));
    assert!(!fixture.origin.service.remote_teams.was_finalized(
        "remote",
        &fixture.worker,
        "fixture-provider",
        "replacement"
    ));
    assert!(!fixture.origin.service.remote_teams.was_finalized(
        "remote",
        &fixture.worker,
        "replacement-provider",
        "worker"
    ));
}

#[tokio::test]
async fn stale_session_finalization_releases_membership_without_closing_the_replacement() {
    let fixture = Fixture::new();
    let peer = fixture.peer();
    let member = adopt(&fixture, &peer).await;
    {
        let mut mux = fixture.remote.service.mux.write();
        let worker = mux.get_tab_mut(&fixture.worker).unwrap();
        arm(worker, "replacement");
    }
    fixture
        .origin
        .service
        .prepare_remote_team_close(&member.team_id)
        .unwrap();
    let receipt = fixture
        .origin
        .service
        .finalize_remote_team_member_with_peer(&member, true, &peer)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, "released_stale_binding");
    let mux = fixture.remote.service.mux.read();
    let replacement = mux.get_tab(&fixture.worker).unwrap();
    assert_eq!(replacement.agent_session_id.as_deref(), Some("replacement"));
    assert_eq!(replacement.pty_id.as_deref(), Some("pty-replacement"));
}

#[tokio::test]
async fn unreachable_peer_retains_the_local_team_and_members_and_releases_its_provisional_freeze() {
    let fixture = Fixture::new();
    let peer = fixture.peer();
    let member = adopt(&fixture, &peer).await;
    assert!(fixture
        .origin
        .service
        .finalize_remote_team_members(&member.team_id, true)
        .await
        .is_err());
    assert!(fixture
        .origin
        .service
        .mux
        .read()
        .get_team(&member.team_id)
        .is_some());
    let data = fixture.origin.service.remote_teams.snapshot().unwrap();
    assert!(!data.members[&member_key("remote", &fixture.worker)].finalized);
    assert!(!data.closing_teams.contains(&member.team_id));
    assert!(fixture
        .remote
        .service
        .mux
        .read()
        .get_tab(&fixture.worker)
        .is_some());
}

#[tokio::test]
async fn lost_finalization_response_recovers_as_absent_without_closing_another_session() {
    let fixture = Fixture::new();
    let peer = fixture.peer();
    let member = adopt(&fixture, &peer).await;
    fixture
        .origin
        .service
        .prepare_remote_team_close(&member.team_id)
        .unwrap();
    struct LoseClose<'a>(&'a FakePeer, AtomicBool);
    #[async_trait::async_trait]
    impl RemoteTeamPeer for LoseClose<'_> {
        async fn post(&self, path: &str, body: Value) -> Result<Value, String> {
            let value = self.0.post(path, body).await?;
            if self.1.swap(false, Ordering::SeqCst) {
                return Err("lost close response".into());
            }
            Ok(value)
        }
    }
    let transport = LoseClose(&peer, AtomicBool::new(true));
    assert!(fixture
        .origin
        .service
        .finalize_remote_team_member_with_peer(&member, true, &transport)
        .await
        .is_err());
    assert!(
        !fixture
            .origin
            .service
            .remote_teams
            .members_for_team(&member.team_id)
            .unwrap()[0]
            .finalized
    );
    let receipt = fixture
        .origin
        .service
        .finalize_remote_team_member_with_peer(&member, true, &transport)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, "absent");
    assert!(
        fixture
            .origin
            .service
            .remote_teams
            .members_for_team(&member.team_id)
            .unwrap()[0]
            .finalized
    );
}

#[tokio::test]
async fn a_pending_transfer_blocks_team_close_and_destination_lifecycle_changes() {
    let fixture = Fixture::new();
    let peer = fixture.peer();
    struct LoseTransfer<'a>(&'a FakePeer);
    #[async_trait::async_trait]
    impl RemoteTeamPeer for LoseTransfer<'_> {
        async fn post(&self, path: &str, body: Value) -> Result<Value, String> {
            let value = self.0.post(path, body).await?;
            if path == "/api/mesh/worker-transfer" {
                return Err("lost transfer response".into());
            }
            Ok(value)
        }
    }
    assert!(fixture
        .origin
        .service
        .reparent_remote_worker_with_peer(
            fixture.caller(),
            "remote",
            &fixture.worker,
            &fixture.destination,
            &LoseTransfer(&peer)
        )
        .await
        .is_err());
    let destination_team = fixture
        .origin
        .service
        .mux
        .read()
        .get_tab(&fixture.destination)
        .unwrap()
        .team_id
        .clone()
        .unwrap();
    assert!(fixture
        .origin
        .service
        .prepare_remote_team_close(&destination_team)
        .is_err());
    assert!(fixture
        .origin
        .service
        .remote_teams
        .ensure_tab_lifecycle_available(&fixture.destination)
        .is_err());
    assert!(fixture
        .origin
        .service
        .clear_tab_role(&fixture.destination)
        .is_err());
    assert!(fixture
        .origin
        .service
        .close_tab(&fixture.destination)
        .is_err());
}

#[tokio::test]
async fn inventory_reads_durable_members_without_any_report_telemetry() {
    let fixture = Fixture::new();
    let peer = fixture.peer();
    let member = adopt(&fixture, &peer).await;
    assert!(fixture.origin.service.remote_workers.snapshot().is_empty());
    let inventory = fixture
        .origin
        .service
        .team_member_inventory(&member.team_id)
        .unwrap();
    let rows = inventory["members"].as_array().unwrap();
    let remote = rows.iter().find(|row| row["remote"] == true).unwrap();
    assert_eq!(remote["peer"], "remote");
    assert_eq!(remote["remote_tab_id"], fixture.worker);
    assert_eq!(remote["session_id"], "worker");
}

#[tokio::test]
async fn working_remote_members_require_force_and_preflight_never_closes_them() {
    let fixture = Fixture::new();
    let peer = fixture.peer();
    let member = adopt(&fixture, &peer).await;
    fixture
        .remote
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.worker)
        .unwrap()
        .agent_state = Some(
        codeterm_types::pty::TabVerdict {
            state: codeterm_types::pty::TimelineState::Working,
            subtype: None,
        }
        .into(),
    );
    let mut request = RemoteWorkerFinalize {
        worker_tab_id: member.tab_id,
        worker_session_id: member.session_id,
        worker_provider: member.provider,
        expected_parent_tab_id: fixture.destination.clone(),
        force: false,
        check_only: true,
    };
    assert!(fixture
        .remote
        .service
        .finalize_remote_worker("origin", &request)
        .is_err());
    request.force = true;
    assert_eq!(
        fixture
            .remote
            .service
            .finalize_remote_worker("origin", &request)
            .unwrap()
            .outcome,
        "ready"
    );
    assert!(fixture
        .remote
        .service
        .mux
        .read()
        .get_tab(&fixture.worker)
        .is_some());
}

#[tokio::test]
async fn a_different_provider_with_the_same_session_identifier_is_released_without_stop() {
    let fixture = Fixture::new();
    let peer = fixture.peer();
    let member = adopt(&fixture, &peer).await;
    fixture
        .remote
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.worker)
        .unwrap()
        .agent_type = Some("replacement-provider".into());
    fixture
        .origin
        .service
        .prepare_remote_team_close(&member.team_id)
        .unwrap();
    let receipt = fixture
        .origin
        .service
        .finalize_remote_team_member_with_peer(&member, true, &peer)
        .await
        .unwrap();
    assert_eq!(receipt.outcome, "released_stale_binding");
    assert!(fixture
        .remote
        .service
        .mux
        .read()
        .get_tab(&fixture.worker)
        .is_some());
}

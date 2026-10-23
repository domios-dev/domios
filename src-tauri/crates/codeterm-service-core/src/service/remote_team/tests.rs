use super::*;
use crate::multiplexer::{
    state::MultiplexerState,
    types::{AgentRole, Tab},
};
use crate::service::{test_fixture::ServiceFixture, AuthenticatedPlanCaller, ServiceContext};
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
mod teardown;

fn arm(tab: &mut Tab, session: &str) {
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some(session.into());
    tab.session_binding_nonce = Some(format!("nonce-{session}"));
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    tab.pty_id = Some(format!("pty-{session}"));
    tab.chat_view = true;
}

struct Fixture {
    origin: ServiceFixture,
    remote: ServiceFixture,
    source: String,
    destination: String,
    worker: String,
}

impl Fixture {
    fn new() -> Self {
        let (mut mux, source) = MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&source).unwrap();
        let destination = mux.create_tab(&workspace).unwrap();
        arm(mux.get_tab_mut(&source).unwrap(), "source");
        arm(mux.get_tab_mut(&destination).unwrap(), "destination");
        mux.set_orchestrator_role(&destination).unwrap();
        mux.resolve_or_mint_team_for_parent(&destination).unwrap();
        let (mut remote_mux, worker) = MultiplexerState::new();
        let tab = remote_mux.get_tab_mut(&worker).unwrap();
        arm(tab, "worker");
        tab.role = Some(AgentRole::Worker);
        tab.report_target_mesh_peer = Some("origin".into());
        tab.report_target_tab_id = Some(source.clone());
        Self {
            origin: ServiceFixture::new(mux),
            remote: ServiceFixture::new(remote_mux),
            source,
            destination,
            worker,
        }
    }

    fn caller(&self) -> AuthenticatedPlanCaller {
        AuthenticatedPlanCaller {
            tab_id: Some(self.source.clone()),
            session_id: Some("source".into()),
            binding_nonce: Some("nonce-source".into()),
        }
    }

    fn peer(&self) -> FakePeer {
        FakePeer {
            remote: self.remote.service.clone(),
            calls: Mutex::new(Vec::new()),
        }
    }
}

struct FakePeer {
    remote: ServiceContext,
    calls: Mutex<Vec<(String, Value)>>,
}

#[async_trait::async_trait]
impl RemoteTeamPeer for FakePeer {
    async fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        self.calls.lock().push((path.into(), body.clone()));
        let remote = self.remote.clone();
        let path = path.to_string();
        let value = crate::service::blocking::run(move || match path.as_str() {
            "/api/mesh/worker-transfer-info" => serde_json::to_value(
                remote.inspect_remote_worker("origin", &serde_json::from_value(body).unwrap())?,
            )
            .map_err(|error| error.to_string()),
            "/api/mesh/worker-transfer" => {
                serde_json::to_value(remote.transfer_remote_worker_report_target(
                    "origin",
                    &serde_json::from_value(body).unwrap(),
                )?)
                .map_err(|error| error.to_string())
            }
            "/api/mesh/worker-finalize" => serde_json::to_value(
                remote.finalize_remote_worker("origin", &serde_json::from_value(body).unwrap())?,
            )
            .map_err(|error| error.to_string()),
            _ => Err("unexpected fake-peer call".into()),
        })
        .await?;
        Ok(value)
    }
}

#[tokio::test]
async fn fake_peer_mesh_calls_commit_exact_membership_and_keep_the_worker_session() {
    let fixture = Fixture::new();
    let peer = fixture.peer();
    let before = fixture
        .remote
        .service
        .mux
        .read()
        .get_tab(&fixture.worker)
        .unwrap()
        .clone();
    let member = fixture
        .origin
        .service
        .reparent_remote_worker_with_peer(
            fixture.caller(),
            "remote",
            &fixture.worker,
            &fixture.destination,
            &peer,
        )
        .await
        .unwrap();
    assert_eq!(member.peer_id, "remote");
    assert_eq!(member.tab_id, fixture.worker);
    assert_eq!(member.session_id, "worker");
    let saved = fixture
        .origin
        .service
        .remote_teams
        .members_for_team(&member.team_id)
        .unwrap();
    assert_eq!(saved, vec![member]);
    let mux = fixture.remote.service.mux.read();
    let after = mux.get_tab(&fixture.worker).unwrap();
    let (_, report_target) =
        crate::service::hierarchy::resolve_report_target(&mux, &fixture.worker).unwrap();
    assert_eq!(
        report_target,
        Some(crate::service::hierarchy::ReportTarget::Mesh {
            peer: "origin".into(),
            tab: fixture.destination.clone()
        })
    );
    assert_eq!(
        after.report_target_tab_id.as_deref(),
        Some(fixture.destination.as_str())
    );
    assert_eq!(after.agent_session_id, before.agent_session_id);
    assert_eq!(after.session_binding_nonce, before.session_binding_nonce);
    assert_eq!(after.pty_id, before.pty_id);
    let calls = peer.calls.lock();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "/api/mesh/worker-transfer-info");
    assert_eq!(calls[1].0, "/api/mesh/worker-transfer");
    assert_eq!(calls[1].1["caller_tab_id"], fixture.source);
    assert_eq!(calls[1].1["worker_session_id"], "worker");
}

#[tokio::test]
async fn lost_response_reuses_one_durable_operation_and_never_transfers_twice() {
    let fixture = Fixture::new();
    let peer = fixture.peer();
    struct LoseTransfer<'a>(&'a FakePeer, AtomicBool);
    #[async_trait::async_trait]
    impl RemoteTeamPeer for LoseTransfer<'_> {
        async fn post(&self, path: &str, body: Value) -> Result<Value, String> {
            let value = self.0.post(path, body).await?;
            if path == "/api/mesh/worker-transfer" && self.1.swap(false, Ordering::SeqCst) {
                return Err("lost response".into());
            }
            Ok(value)
        }
    }
    let transport = LoseTransfer(&peer, AtomicBool::new(true));
    assert!(fixture
        .origin
        .service
        .reparent_remote_worker_with_peer(
            fixture.caller(),
            "remote",
            &fixture.worker,
            &fixture.destination,
            &transport
        )
        .await
        .is_err());
    let pending = fixture.origin.service.remote_teams.snapshot().unwrap();
    assert_eq!(pending.outgoing.len(), 1);
    assert!(!pending.outgoing.values().next().unwrap().completed);
    let member = fixture
        .origin
        .service
        .reparent_remote_worker_with_peer(
            fixture.caller(),
            "remote",
            &fixture.worker,
            &fixture.destination,
            &transport,
        )
        .await
        .unwrap();
    assert_eq!(
        member.operation_id,
        pending.outgoing.keys().next().unwrap().as_str()
    );
    let remote = fixture.remote.service.remote_teams.snapshot().unwrap();
    assert_eq!(remote.incoming.len(), 1);
    assert_eq!(peer.calls.lock().len(), 3);
}

#[test]
fn persist_failure_publishes_neither_membership_nor_a_partial_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("remote-teams.json");
    let store = RemoteTeamStore::open(path.clone());
    std::fs::create_dir(&path).unwrap();
    assert!(store
        .update(|data| {
            data.closing_teams.insert("team".into());
            Ok(())
        })
        .is_err());
    assert!(store.snapshot().unwrap().closing_teams.is_empty());
    std::fs::remove_dir(&path).unwrap();
    store
        .update(|data| {
            data.closing_teams.insert("team".into());
            Ok(())
        })
        .unwrap();
    assert!(RemoteTeamStore::open(path)
        .snapshot()
        .unwrap()
        .closing_teams
        .contains("team"));
}

#[test]
fn durable_worker_receipt_recovers_a_stale_session_snapshot() {
    let fixture = Fixture::new();
    let request = RemoteWorkerTransfer {
        operation_id: "operation".into(),
        worker_tab_id: fixture.worker.clone(),
        worker_session_id: "worker".into(),
        worker_provider: "fixture-provider".into(),
        expected_parent_tab_id: fixture.source.clone(),
        destination_tab_id: fixture.destination.clone(),
        destination_team_id: Some("origin-team".into()),
        caller_tab_id: Some(fixture.source.clone()),
        owner_confirmed: false,
    };
    let receipt = fixture
        .remote
        .service
        .transfer_remote_worker_report_target("origin", &request)
        .unwrap();
    fixture
        .remote
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.worker)
        .unwrap()
        .report_target_tab_id = Some(fixture.source.clone());
    fixture
        .remote
        .service
        .mux
        .write()
        .get_tab_mut(&fixture.worker)
        .unwrap()
        .reported_session_binding_nonce = None;
    fixture.remote.service.restore_remote_worker_transfers();
    {
        let mut mux = fixture.remote.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.worker).unwrap();
        assert_eq!(tab.reported_session_binding_nonce, None);
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    }
    assert_eq!(
        fixture
            .remote
            .service
            .mux
            .read()
            .get_tab(&fixture.worker)
            .unwrap()
            .report_target_tab_id
            .as_deref(),
        Some(fixture.destination.as_str())
    );
    assert_eq!(
        fixture
            .remote
            .service
            .transfer_remote_worker_report_target("origin", &request)
            .unwrap(),
        receipt
    );
    let mut conflicting = request;
    conflicting.destination_tab_id = "foreign".into();
    assert!(fixture
        .remote
        .service
        .transfer_remote_worker_report_target("origin", &conflicting)
        .is_err());
}

#[tokio::test]
async fn caller_nonce_and_destination_authority_are_checked_before_mesh_calls() {
    let fixture = Fixture::new();
    let peer = fixture.peer();
    let mut wrong = fixture.caller();
    wrong.binding_nonce = Some("foreign".into());
    assert!(fixture
        .origin
        .service
        .reparent_remote_worker_with_peer(
            wrong,
            "remote",
            &fixture.worker,
            &fixture.destination,
            &peer
        )
        .await
        .is_err());
    assert!(fixture
        .origin
        .service
        .reparent_remote_worker_with_peer(
            fixture.caller(),
            "remote",
            &fixture.worker,
            &fixture.source,
            &peer
        )
        .await
        .is_err());
    assert!(peer.calls.lock().is_empty());
}

#[tokio::test]
async fn failed_local_commit_recovers_from_disk_without_a_second_worker_mutation() {
    let fixture = Fixture::new();
    let peer = fixture.peer();
    struct FailCommit<'a>(&'a FakePeer, &'a RemoteTeamStore, AtomicBool);
    #[async_trait::async_trait]
    impl RemoteTeamPeer for FailCommit<'_> {
        async fn post(&self, path: &str, body: Value) -> Result<Value, String> {
            let value = self.0.post(path, body).await?;
            if path == "/api/mesh/worker-transfer" && self.2.swap(false, Ordering::SeqCst) {
                self.1.fail_next_persist();
            }
            Ok(value)
        }
    }
    let transport = FailCommit(
        &peer,
        &fixture.origin.service.remote_teams,
        AtomicBool::new(true),
    );
    assert!(fixture
        .origin
        .service
        .reparent_remote_worker_with_peer(
            fixture.caller(),
            "remote",
            &fixture.worker,
            &fixture.destination,
            &transport
        )
        .await
        .is_err());
    let prepared = fixture.origin.service.remote_teams.snapshot().unwrap();
    assert!(prepared.members.is_empty());
    assert!(!prepared.outgoing.values().next().unwrap().completed);
    let mut recovered = fixture.origin.service.clone();
    recovered.remote_teams = std::sync::Arc::new(RemoteTeamStore::open(
        fixture.origin.temp.path().join("remote_teams.json"),
    ));
    assert!(
        !recovered
            .remote_teams
            .snapshot()
            .unwrap()
            .outgoing
            .values()
            .next()
            .unwrap()
            .completed
    );
    let member = recovered
        .reparent_remote_worker_with_peer(
            fixture.caller(),
            "remote",
            &fixture.worker,
            &fixture.destination,
            &transport,
        )
        .await
        .unwrap();
    assert_eq!(
        fixture
            .remote
            .service
            .remote_teams
            .snapshot()
            .unwrap()
            .incoming
            .len(),
        1
    );
    assert_eq!(
        member.operation_id,
        prepared.outgoing.keys().next().unwrap().as_str()
    );
}

#[test]
fn receipt_commit_failure_does_not_mutate_the_remote_worker() {
    let fixture = Fixture::new();
    let request = RemoteWorkerTransfer {
        operation_id: "operation".into(),
        worker_tab_id: fixture.worker.clone(),
        worker_session_id: "worker".into(),
        worker_provider: "fixture-provider".into(),
        expected_parent_tab_id: fixture.source.clone(),
        destination_tab_id: fixture.destination.clone(),
        destination_team_id: None,
        caller_tab_id: Some(fixture.source.clone()),
        owner_confirmed: false,
    };
    fixture.remote.service.remote_teams.fail_next_persist();
    assert!(fixture
        .remote
        .service
        .transfer_remote_worker_report_target("origin", &request)
        .is_err());
    assert_eq!(
        fixture
            .remote
            .service
            .mux
            .read()
            .get_tab(&fixture.worker)
            .unwrap()
            .report_target_tab_id
            .as_deref(),
        Some(fixture.source.as_str())
    );
    assert!(fixture
        .remote
        .service
        .remote_teams
        .snapshot()
        .unwrap()
        .incoming
        .is_empty());
}

#[test]
fn a_superseded_receipt_cannot_replay_an_older_parent() {
    let fixture = Fixture::new();
    let mut request = RemoteWorkerTransfer {
        operation_id: "first".into(),
        worker_tab_id: fixture.worker.clone(),
        worker_session_id: "worker".into(),
        worker_provider: "fixture-provider".into(),
        expected_parent_tab_id: fixture.source.clone(),
        destination_tab_id: fixture.destination.clone(),
        destination_team_id: None,
        caller_tab_id: Some(fixture.source.clone()),
        owner_confirmed: false,
    };
    let first = fixture
        .remote
        .service
        .transfer_remote_worker_report_target("origin", &request)
        .unwrap();
    let old = request.clone();
    request.operation_id = "second".into();
    request.expected_parent_tab_id = fixture.destination.clone();
    request.caller_tab_id = Some(fixture.destination.clone());
    request.destination_tab_id = "next-parent".into();
    fixture
        .remote
        .service
        .transfer_remote_worker_report_target("origin", &request)
        .unwrap();
    assert_eq!(
        fixture
            .remote
            .service
            .transfer_remote_worker_report_target("origin", &old)
            .unwrap(),
        first
    );
    assert_eq!(
        fixture
            .remote
            .service
            .mux
            .read()
            .get_tab(&fixture.worker)
            .unwrap()
            .report_target_tab_id
            .as_deref(),
        Some("next-parent")
    );
}

#[test]
fn origin_transfer_routes_are_denied_to_every_remote_peer_even_with_full_access() {
    use crate::mesh::types::AccessLevel;
    for path in [
        "/api/agents/reparent",
        "/api/teams/local-team/adopt-remote?ignored=true",
    ] {
        for access in [
            AccessLevel::Full,
            AccessLevel::ReadOnly,
            AccessLevel::AgentIsolated,
        ] {
            assert!(!crate::api_support::access_policy::peer_access_allowed(
                &access,
                "foreign",
                &axum::http::Method::POST,
                path,
                |_| panic!("local origin operations never consult remote ownership")
            ));
        }
    }
}

#[test]
fn local_reparent_preserves_the_session_and_refuses_a_destination_pull() {
    let fixture = Fixture::new();
    let worker = {
        let mut mux = fixture.origin.service.mux.write();
        let workspace = mux.find_workspace_for_tab(&fixture.source).unwrap();
        let id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        arm(tab, "local-worker");
        tab.role = Some(AgentRole::Worker);
        tab.parent_tab_id = Some(fixture.source.clone());
        id
    };
    let pulling = AuthenticatedPlanCaller {
        tab_id: Some(fixture.destination.clone()),
        session_id: Some("destination".into()),
        binding_nonce: Some("nonce-destination".into()),
    };
    assert!(fixture
        .origin
        .service
        .reparent_local_worker(&pulling, &worker, &fixture.destination)
        .is_err());
    fixture
        .origin
        .service
        .reparent_local_worker(&fixture.caller(), &worker, &fixture.destination)
        .unwrap();
    let mux = fixture.origin.service.mux.read();
    let tab = mux.get_tab(&worker).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("local-worker"));
    assert_eq!(tab.pty_id.as_deref(), Some("pty-local-worker"));
    assert_eq!(
        tab.parent_tab_id.as_deref(),
        Some(fixture.destination.as_str())
    );
    assert_eq!(
        tab.team_id,
        mux.get_tab(&fixture.destination).unwrap().team_id
    );
}

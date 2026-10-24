use super::authority::{admission_refusal, membership_refusal};
use super::*;
use async_trait::async_trait;
use codeterm_account::account_mesh::{
    AccountMeshConfig, AccountMeshPorts, AccountMeshStore, Cleanup, Clock, EndpointSource,
    InstallationKeyStore, MembershipRefusal, SessionSource, SignedMembership, UnavailableRegistry,
    VerifiedMembership,
};
use codeterm_mesh::account_mesh as mesh;
use codeterm_mesh::types::{AccessLevel, MeshIdentity, PairedPeer, RELAY_PROTOCOL_VERSION};
use serde_json::json;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

fn signed_in(epoch: u64) -> LifecycleState {
    LifecycleState {
        account_epoch: epoch,
        presence: Presence::SignedIn {
            account_id: "a".repeat(64),
        },
        owner: Some("a".repeat(64)),
        membership: Some(SignedMembership {
            token: "{\"payload\":\"p\"}".into(),
            verified: VerifiedMembership {
                account_id: "a".repeat(64),
                installation_id: "installation-a".into(),
                key_id: "k".repeat(64),
                key_generation: 1,
                slot_revision: 2,
                issued_at: 100,
                expires_at: 3700,
            },
        }),
        revoked_installations: ["installation-c".to_owned()].into(),
        ..LifecycleState::default()
    }
}

#[test]
fn account_mesh_state_reaches_the_gate_with_epoch_presence_membership_and_revocations() {
    let state = mesh_state(&signed_in(3));
    assert_eq!(state.account_epoch, 3);
    assert_eq!(state.signed_in_account(), Some("a".repeat(64).as_str()));
    let membership = state.current_membership(100).expect("current");
    assert_eq!(membership.verified.key_id, "k".repeat(64));
    assert_eq!(membership.verified.slot_revision, 2);
    assert!(
        state.current_membership(3700).is_none(),
        "expiry is exclusive"
    );
    assert!(state.revoked_installations.contains("installation-c"));

    let signed_out = LifecycleState {
        presence: Presence::SignedOut,
        membership: None,
        ..signed_in(4)
    };
    let state = mesh_state(&signed_out);
    assert_eq!(state.presence, mesh::AccountPresence::SignedOut);
    assert!(state.membership.is_none());
    assert_eq!(
        mesh_state(&LifecycleState::default()).presence,
        mesh::AccountPresence::NeverSignedIn
    );
}

#[test]
fn account_mesh_refresh_errors_are_classified_for_retry_or_sign_in() {
    assert_eq!(
        session_refusal(codeterm_account::Error::Credentials),
        AccountMeshRefusal::Unauthenticated
    );
    assert_eq!(
        session_refusal(codeterm_account::Error::Provider),
        AccountMeshRefusal::Unavailable
    );
    assert_eq!(
        session_refusal(codeterm_account::Error::Transport),
        AccountMeshRefusal::Unavailable
    );
}

#[test]
fn account_mesh_only_the_machine_limit_reaches_the_gate_as_mesh_machines() {
    let limit = admission_refusal(AccountMeshRefusal::MachineLimit {
        limit: 2,
        active_count: 2,
    });
    assert_eq!(
        serde_json::to_value(&limit).unwrap(),
        json!({"reason": "mesh.machines", "limit": 2, "active_count": 2})
    );
    for refusal in [
        AccountMeshRefusal::Invalid,
        AccountMeshRefusal::Unauthenticated,
        AccountMeshRefusal::ReauthRequired,
        AccountMeshRefusal::AppCheck,
        AccountMeshRefusal::Forbidden,
        AccountMeshRefusal::Conflict,
        AccountMeshRefusal::RateLimited {
            retry_after_seconds: 30,
        },
        AccountMeshRefusal::AuthorityUnavailable,
        AccountMeshRefusal::Unavailable,
        AccountMeshRefusal::Offline,
        AccountMeshRefusal::VerificationUnavailable,
        AccountMeshRefusal::Revoked,
        AccountMeshRefusal::SignedOut,
        AccountMeshRefusal::Cancelled,
    ] {
        let mapped = admission_refusal(refusal);
        assert!(
            !matches!(mapped, mesh::AdmissionRefusal::MachineLimit { .. }),
            "{refusal:?}"
        );
    }
    assert_eq!(
        admission_refusal(AccountMeshRefusal::SignedOut),
        mesh::AdmissionRefusal::SignedOut
    );
    assert_eq!(
        admission_refusal(AccountMeshRefusal::ReauthRequired),
        mesh::AdmissionRefusal::Unauthenticated
    );
    assert_eq!(
        admission_refusal(AccountMeshRefusal::Conflict),
        mesh::AdmissionRefusal::handshake("conflict")
    );
    assert_eq!(
        membership_refusal(MembershipRefusal::Expired),
        mesh::AdmissionRefusal::Revoked
    );
    assert_eq!(
        membership_refusal(MembershipRefusal::Signature),
        mesh::AdmissionRefusal::handshake("membership-signature")
    );
    assert_eq!(
        membership_refusal(MembershipRefusal::NotYetValid),
        mesh::AdmissionRefusal::handshake("membership-not_yet_valid"),
        "a clock behind the issuer is not a revocation"
    );
}

#[test]
fn account_mesh_snapshot_and_outcomes_match_the_desktop_client() {
    let mut state = signed_in(2);
    state.cleanup = Cleanup::Pending;
    let snapshot = AccountMeshSnapshot {
        view: state.view(),
        pairs: Vec::new(),
        link_requests: Vec::new(),
    };
    let value = serde_json::to_value(&snapshot).unwrap();
    for key in [
        "binding",
        "traffic",
        "cleanup",
        "self",
        "registry",
        "registry_failure",
        "pairs",
        "limit_notice",
    ] {
        assert!(value.get(key).is_some(), "{key}");
    }
    assert_eq!(value["binding"], "current");
    assert_eq!(value["traffic"], "open");
    assert_eq!(value["cleanup"], "pending");
    assert!(
        value.get("membership").is_none(),
        "membership tokens never leave the daemon"
    );

    let refused = AccountMeshOutcome::<()>::of(
        Err(AccountMeshRefusal::MachineLimit {
            limit: 2,
            active_count: 2,
        }),
        Some("r".repeat(64)),
    );
    assert_eq!(
        serde_json::to_value(&refused).unwrap(),
        json!({"ok": false, "request_id": "r".repeat(64),
            "failure": {"reason": "mesh.machines", "limit": 2, "active_count": 2}})
    );
    let done = AccountMeshOutcome::of(Ok(()), None);
    assert_eq!(
        serde_json::to_value(&done).unwrap(),
        json!({"ok": true, "result": null})
    );
}

#[test]
fn account_mesh_start_failure_never_opens_past_a_persisted_sign_out() {
    let signed_out = AccountMeshRecord {
        signed_out: true,
        ..AccountMeshRecord::default()
    };
    assert_eq!(
        fallback_mesh_state(Ok(signed_out)).presence,
        mesh::AccountPresence::SignedOut
    );
    assert_eq!(
        fallback_mesh_state(Err(std::io::Error::other("corrupt"))).presence,
        mesh::AccountPresence::SignedOut,
        "an unreadable record fails closed"
    );
    assert_eq!(
        fallback_mesh_state(Ok(AccountMeshRecord::default())).presence,
        mesh::AccountPresence::NeverSignedIn,
        "an upgrade that never signed out keeps legacy pairs"
    );
}

#[test]
fn account_mesh_unchanged_lifecycle_pushes_do_not_wake_the_gate() {
    let sender = watch::channel(mesh::AccountMeshState::default()).0;
    let mut receiver = sender.subscribe();
    assert!(publish_mesh_state(&sender, mesh_state(&signed_in(1))));
    assert!(receiver.has_changed().unwrap());
    drop(receiver.borrow_and_update());
    assert!(!publish_mesh_state(&sender, mesh_state(&signed_in(1))));
    assert!(
        !receiver.has_changed().unwrap(),
        "same projection, no wakeup"
    );
    assert!(publish_mesh_state(&sender, mesh_state(&signed_in(2))));
}

#[test]
fn account_mesh_not_before_skew_matches_across_account_and_mesh() {
    assert_eq!(
        codeterm_account::account_mesh::NOT_BEFORE_SKEW_SECONDS,
        codeterm_mesh::account_mesh::ports::NOT_BEFORE_SKEW_SECONDS
    );
}

fn legacy_mesh() -> crate::MeshStateHandle {
    Arc::new(parking_lot::Mutex::new(legacy_manager()))
}

fn legacy_manager() -> codeterm_mesh::MeshManager {
    let mut manager = codeterm_mesh::MeshManager::in_memory(
        MeshIdentity {
            instance_id: "local-machine".into(),
            private_key: None,
            public_key: None,
        },
        "local".into(),
        7685,
    );
    manager.insert_legacy_pair_for_test(PairedPeer {
        instance_id: "peer-a".into(),
        instance_name: "Peer A".into(),
        addresses: Vec::new(),
        api_port: 7685,
        public_key: "peer-key".into(),
        paired_at: "before-upgrade".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level: AccessLevel::ReadOnly,
        version: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    });
    manager
}

struct NoSession;

#[async_trait]
impl SessionSource for NoSession {
    async fn session(&self) -> Result<Option<codeterm_account::Session>, AccountMeshRefusal> {
        Ok(None)
    }
}

struct NoKeys;

#[async_trait]
impl InstallationKeyStore for NoKeys {
    async fn load(&self) -> codeterm_account::Result<Option<codeterm_account::Secret>> {
        Ok(None)
    }

    async fn save(&self, _: &codeterm_account::Secret) -> codeterm_account::Result<()> {
        Ok(())
    }
}

struct NoEndpoint;

impl EndpointSource for NoEndpoint {
    fn ready_endpoint(&self) -> Option<codeterm_account::account_mesh::wire::MeshEndpoint> {
        None
    }
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> u64 {
        100
    }
}

#[derive(Default)]
struct HeldSignOutStore {
    record: parking_lot::Mutex<AccountMeshRecord>,
    entered: Notify,
    release: Notify,
}

#[async_trait]
impl AccountMeshStore for HeldSignOutStore {
    async fn load(&self) -> io::Result<AccountMeshRecord> {
        Ok(self.record.lock().clone())
    }

    async fn save(&self, record: &AccountMeshRecord) -> io::Result<()> {
        if record.signed_out {
            self.entered.notify_one();
            self.release.notified().await;
        }
        *self.record.lock() = record.clone();
        Ok(())
    }
}

#[tokio::test]
async fn sign_out_closes_the_mesh_manager_gate_before_persistence_can_suspend() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let mesh_state = fixture.service.ext::<crate::MeshStateHandle>().clone();
    *mesh_state.lock() = legacy_manager();
    let service = Arc::new(fixture.service.clone());
    let transport = Arc::new(TransportBridge {
        service: Arc::downgrade(&service),
        mesh_state: watch::channel(mesh::AccountMeshState::default()).0,
    });
    let store = Arc::new(HeldSignOutStore::default());
    let account_mesh = AccountMesh::new(
        AccountMeshConfig {
            installation_id: codeterm_account::InstallationId::parse("installation-a".into())
                .unwrap(),
            trust: None,
            call_deadline: Duration::from_secs(1),
            withdrawal_deadline: Duration::from_secs(1),
            schedule_leases: false,
        },
        AccountMeshPorts {
            registry: Arc::new(UnavailableRegistry),
            sessions: Arc::new(NoSession),
            keys: Arc::new(NoKeys),
            store: store.clone(),
            endpoint: Arc::new(NoEndpoint),
            transport,
            clock: Arc::new(FixedClock),
        },
    );
    account_mesh.load().await;
    account_mesh
        .identity_changed(Some(&codeterm_account::Identity {
            uid: "account-user".into(),
            email: None,
            display_name: None,
            provider: "test".into(),
            photo_url: None,
        }))
        .await;
    assert!(mesh_state.lock().traffic_open("peer-a"));

    let sign_out = tokio::spawn({
        let account_mesh = account_mesh.clone();
        async move { account_mesh.sign_out(std::future::pending::<()>()).await }
    });
    store.entered.notified().await;
    assert!(!mesh_state.lock().traffic_open("peer-a"));
    sign_out.abort();
    let _ = sign_out.await;
    assert!(!mesh_state.lock().traffic_open("peer-a"));
}

#[tokio::test]
async fn failed_account_mesh_initialization_fallback_persists_sign_out_and_blocks_mesh() {
    let mesh_state = legacy_mesh();
    assert!(mesh_state.lock().traffic_open("peer-a"));
    let temp = tempfile::tempdir().unwrap();
    let store: Arc<dyn AccountMeshStore> = Arc::new(FileAccountMeshStore::new(temp.path().into()));

    super::block_and_persist_sign_out(Some(mesh_state.clone()), store.clone())
        .await
        .unwrap();

    assert!(!mesh_state.lock().traffic_open("peer-a"));
    assert!(store.load().await.unwrap().signed_out);
    assert!(mesh_state.lock().paired_peers.contains_key("peer-a"));
}

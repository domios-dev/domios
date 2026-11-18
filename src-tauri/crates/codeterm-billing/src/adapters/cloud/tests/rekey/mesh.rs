use super::*;
use codeterm_account::account_mesh::{
    account_id,
    wire::{
        ChallengeRequest, GenerationPolicy, InstallationEnrollment, KeyEnrollment,
        MeshAdmissionBody, MeshEndpoint, MeshResult, PriorSessionsRevokedResult,
        PriorSessionsTarget, ProofEndpoint, RegistryRefusal, SignedEnvelope,
    },
    AccountMesh, AccountMeshConfig, AccountMeshPorts, AccountMeshRecord, AccountMeshRefusal,
    AccountMeshStore, Clock, EndpointSource, EnrollTrigger, InstallationKey, InstallationKeyStore,
    LifecycleState, MembershipTrust, MeshRegistry, MeshTransport, RegistryResult, SessionSource,
};
use codeterm_account::Secret;
use codeterm_entitlements::PinnedAuthority;
use parking_lot::Mutex;
use std::{future::Future, task::Poll};

const MESH_DEADLINE: Duration = Duration::from_secs(15);
const NOW: u64 = 1_700_000_060;

struct MeshFixture {
    session: Mutex<Session>,
    key: Mutex<Secret>,
    record: AccountMeshRecord,
    saving_key: Notify,
    release_save: Notify,
    entitlement_challenges: AtomicU64,
    stall_replacement_challenge: bool,
    requesting_replacement_challenge: Notify,
}

impl MeshFixture {
    async fn new(
        session: &Session,
        stall_replacement_challenge: bool,
    ) -> (Arc<Self>, Arc<AccountMesh>) {
        let key = InstallationKey::generate();
        let account = account_id(&session.identity.uid);
        let mut record: AccountMeshRecord = serde_json::from_value(serde_json::json!({
            "keys": { (account.clone()): {"key_id": key.key_id(), "generation": 3} }
        }))
        .unwrap();
        record
            .terminal_refusals
            .insert(account, AccountMeshRefusal::Revoked);
        let fixture = Arc::new(Self {
            session: Mutex::new(session.clone()),
            key: Mutex::new(key.to_secret()),
            record,
            saving_key: Notify::new(),
            release_save: Notify::new(),
            entitlement_challenges: AtomicU64::new(0),
            stall_replacement_challenge,
            requesting_replacement_challenge: Notify::new(),
        });
        let mesh = AccountMesh::new(
            AccountMeshConfig {
                installation_id: id(),
                default_display_name: None,
                trust: Some(MembershipTrust::new(Arc::new(PinnedAuthority::new(
                    activation("fixture-online-v1"),
                )))),
                call_deadline: MESH_DEADLINE,
                withdrawal_deadline: MESH_DEADLINE,
                schedule_leases: false,
            },
            AccountMeshPorts {
                registry: fixture.clone(),
                sessions: fixture.clone(),
                keys: fixture.clone(),
                store: fixture.clone(),
                endpoint: fixture.clone(),
                transport: fixture.clone(),
                clock: fixture.clone(),
            },
        );
        mesh.load().await;
        mesh.identity_changed(Some(&session.identity)).await;
        mesh.enrollment_signed_in();
        (fixture, mesh)
    }
}

#[async_trait]
impl SessionSource for MeshFixture {
    async fn session(&self) -> std::result::Result<Option<Session>, AccountMeshRefusal> {
        Ok(Some(self.session.lock().clone()))
    }

    async fn refresh(&self) -> std::result::Result<Option<Session>, AccountMeshRefusal> {
        self.session().await
    }
}

#[async_trait]
impl InstallationKeyStore for MeshFixture {
    async fn load(&self) -> codeterm_account::Result<Option<Secret>> {
        Ok(Some(self.key.lock().clone()))
    }

    async fn save(&self, key: &Secret) -> codeterm_account::Result<()> {
        self.saving_key.notify_one();
        self.release_save.notified().await;
        *self.key.lock() = key.clone();
        Ok(())
    }
}

#[async_trait]
impl AccountMeshStore for MeshFixture {
    async fn load(&self) -> std::io::Result<AccountMeshRecord> {
        Ok(self.record.clone())
    }

    async fn save(&self, _: &AccountMeshRecord) -> std::io::Result<()> {
        Ok(())
    }
}

impl Clock for MeshFixture {
    fn now(&self) -> u64 {
        NOW
    }
}

impl EndpointSource for MeshFixture {
    fn ready_endpoint(&self) -> Option<MeshEndpoint> {
        None
    }
}

#[async_trait]
impl MeshTransport for MeshFixture {
    async fn apply(&self, _: &LifecycleState) {}

    async fn installation_revoked(&self, _: &str) {}

    async fn presence_changed(&self, _: u64) {}
}

#[async_trait]
impl MeshRegistry for MeshFixture {
    fn handles_call_deadline(&self) -> bool {
        self.stall_replacement_challenge
    }

    async fn revoke_prior_sessions(
        &self,
        _: &Session,
        _: &PriorSessionsTarget,
    ) -> RegistryResult<PriorSessionsRevokedResult> {
        panic!("unexpected prior-session cleanup")
    }

    async fn challenge(&self, _: &Session, request: &ChallengeRequest) -> RegistryResult<String> {
        if request.endpoint == ProofEndpoint::GetEntitlements {
            let previous = self.entitlement_challenges.fetch_add(1, Ordering::SeqCst);
            if previous == 1 && self.stall_replacement_challenge {
                self.requesting_replacement_challenge.notify_one();
                return std::future::pending().await;
            }
        }
        Ok(format!("{NOW}.{}", request.key_id))
    }

    async fn enroll_key(
        &self,
        _: &Session,
        request: &InstallationEnrollment,
    ) -> RegistryResult<KeyEnrollment> {
        assert_eq!(
            request.body.generation_policy,
            Some(GenerationPolicy::Assign)
        );
        assert_eq!(request.proof.generation, 0);
        Ok(KeyEnrollment {
            generation: 4,
            key_id: request.proof.key_id.clone(),
        })
    }

    async fn admission(
        &self,
        _: &Session,
        _: &SignedEnvelope<MeshAdmissionBody>,
    ) -> RegistryResult<MeshResult> {
        Err(RegistryRefusal::Unavailable)
    }
}

struct ObservedMesh {
    mesh: Arc<AccountMesh>,
    replacement_waiting: Notify,
}

#[async_trait]
impl InstallationProofSource for ObservedMesh {
    async fn proof(&self, session: &Session, id: &InstallationId) -> Result<InstallationProof> {
        self.mesh.proof(session, id).await
    }

    async fn replacement_proof(
        &self,
        session: &Session,
        id: &InstallationId,
        generation: u64,
    ) -> Option<InstallationProof> {
        let mut replacement = std::pin::pin!(self.mesh.replacement_proof(session, id, generation));
        let first_poll =
            std::future::poll_fn(|cx| Poll::Ready(replacement.as_mut().poll(cx))).await;
        assert!(
            first_poll.is_pending(),
            "replacement did not wait for key storage"
        );
        self.replacement_waiting.notify_one();
        replacement.await
    }
}

#[derive(Clone, Copy)]
enum DuringRekey {
    SameLogin,
    NewLogin,
    NewAccount,
    SaveDeadline,
    ChallengeDeadline,
}

async fn billing_during_mesh_rekey(
    scenario: DuringRekey,
) -> (Snapshot, Vec<Option<InstallationProof>>) {
    let session = fixture_session();
    let stall_challenge = matches!(scenario, DuringRekey::ChallengeDeadline);
    let (fixture, mesh) = MeshFixture::new(&session, stall_challenge).await;
    let source = Arc::new(ObservedMesh {
        mesh: mesh.clone(),
        replacement_waiting: Notify::new(),
    });
    let transport = Arc::new(RekeyTransport::new("FAILED_PRECONDITION", None, false));
    let billing = fixture_billing(source.clone(), transport.clone());
    let snapshot = tokio::time::timeout(MESH_DEADLINE * 2, async {
        let (snapshot, (rekey,)) = tokio::join!(
            billing.refresh(
                Some(&session),
                RefreshEvent::SignIn,
                std::time::Instant::now()
            ),
            async {
                transport.received_first.notified().await;
                let rekey =
                    tokio::spawn(async move { mesh.refresh(EnrollTrigger::Automatic).await });
                fixture.saving_key.notified().await;
                transport.release_first.notify_one();
                source.replacement_waiting.notified().await;
                assert_eq!(transport.requests.lock().len(), 1);
                assert_eq!(fixture.entitlement_challenges.load(Ordering::SeqCst), 1);

                match scenario {
                    DuringRekey::SameLogin => {}
                    DuringRekey::NewLogin => fixture.session.lock().login_generation += 1,
                    DuringRekey::NewAccount => {
                        let identity = {
                            let mut current = fixture.session.lock();
                            current.identity.uid = "another-account".into();
                            current.login_generation += 1;
                            current.identity.clone()
                        };
                        source.mesh.identity_changed(Some(&identity)).await;
                    }
                    DuringRekey::SaveDeadline => {
                        tokio::time::pause();
                        tokio::time::advance(MESH_DEADLINE).await;
                        // Keep the save blocked until billing has settled through fallback.
                        return (rekey,);
                    }
                    DuringRekey::ChallengeDeadline => {
                        fixture.release_save.notify_one();
                        fixture.requesting_replacement_challenge.notified().await;
                        tokio::time::pause();
                        tokio::time::advance(MESH_DEADLINE).await;
                        return (rekey,);
                    }
                }
                if !matches!(scenario, DuringRekey::NewAccount) {
                    fixture.release_save.notify_one();
                }
                (rekey,)
            }
        );
        if matches!(scenario, DuringRekey::SaveDeadline) {
            fixture.release_save.notify_one();
        }
        let expected = if matches!(scenario, DuringRekey::NewAccount) {
            AccountMeshRefusal::Cancelled
        } else {
            AccountMeshRefusal::Unavailable
        };
        assert_eq!(rekey.await.unwrap(), Err(expected));
        snapshot
    })
    .await
    .expect("billing or Mesh failed to settle");
    let requests = transport.requests.lock().clone();
    if matches!(scenario, DuringRekey::SameLogin) {
        let replacement = requests[1].as_ref().unwrap();
        let key = InstallationKey::from_secret(&fixture.key.lock()).unwrap();
        assert_eq!(replacement.key_id, key.key_id());
    }
    assert_eq!(
        fixture.entitlement_challenges.load(Ordering::SeqCst),
        if stall_challenge || matches!(scenario, DuringRekey::SameLogin) {
            2
        } else {
            1
        }
    );
    (snapshot, requests)
}

#[tokio::test]
async fn in_flight_rekey_verifies_first_settled_billing_read() {
    let (snapshot, requests) = billing_during_mesh_rekey(DuringRekey::SameLogin).await;
    let [Some(first), Some(replacement)] = requests.as_slice() else {
        panic!("expected one replacement signed read, got {requests:?}");
    };
    assert_eq!((first.generation, replacement.generation), (3, 4));
    assert_ne!(first.key_id, replacement.key_id);
    assert_ne!(first.nonce, replacement.nonce);
    assert!(!snapshot.checking);
    assert_eq!(snapshot.verification, None);
    assert_eq!(snapshot.access.unwrap().authority, Authority::Billing);
}

fn assert_unsigned_fallback(snapshot: Snapshot, requests: &[Option<InstallationProof>]) {
    let [Some(first), None] = requests else {
        panic!("expected unsigned fallback, got {requests:?}");
    };
    assert_eq!(first.generation, 3);
    assert!(!snapshot.checking);
    assert_eq!(snapshot.verification, Some(Verification::Unconfirmed));
    assert_eq!(snapshot.access.unwrap().authority, Authority::Free);
}

#[tokio::test]
async fn in_flight_rekey_refuses_old_login_and_account_after_wait() {
    for scenario in [DuringRekey::NewLogin, DuringRekey::NewAccount] {
        let (snapshot, requests) = billing_during_mesh_rekey(scenario).await;
        assert_unsigned_fallback(snapshot, &requests);
    }
}

#[tokio::test]
async fn in_flight_rekey_deadline_settles_billing_while_key_storage_is_blocked() {
    let (snapshot, requests) = billing_during_mesh_rekey(DuringRekey::SaveDeadline).await;
    assert_unsigned_fallback(snapshot, &requests);
}

#[tokio::test]
async fn in_flight_rekey_deadline_also_bounds_replacement_challenge() {
    let (snapshot, requests) = billing_during_mesh_rekey(DuringRekey::ChallengeDeadline).await;
    assert_unsigned_fallback(snapshot, &requests);
}

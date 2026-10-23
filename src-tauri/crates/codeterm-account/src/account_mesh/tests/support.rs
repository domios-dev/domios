use crate::account_mesh::wire::*;
use crate::account_mesh::*;
use crate::{Identity, InstallationId, Secret, Session};
use async_trait::async_trait;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use codeterm_entitlements::{Activation, CompiledKey, DetachedGrant, Mode, PinnedAuthority};
use ed25519_dalek::{Signature, Signer, SigningKey};
use serde_json::json;
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

pub const PROJECT: &str = "demo-domios";
pub const EPOCH: &str = "mesh-epoch-1";
pub const ISSUER_KEY: &str = "issuer-1";
pub const INSTALLATION: &str = "installation-a";
pub const PEER: &str = "installation-b";
pub const START: u64 = 1_800_000_000;

pub type Log = Arc<Mutex<Vec<String>>>;

pub fn note(log: &Log, entry: impl Into<String>) {
    log.lock().unwrap().push(entry.into());
}

pub fn position(log: &Log, prefix: &str) -> usize {
    log.lock()
        .unwrap()
        .iter()
        .position(|entry| entry.starts_with(prefix))
        .unwrap_or_else(|| panic!("missing log entry {prefix}"))
}

pub fn count(log: &Log, prefix: &str) -> usize {
    log.lock()
        .unwrap()
        .iter()
        .filter(|entry| entry.starts_with(prefix))
        .count()
}

pub fn identity(uid: &str) -> Identity {
    Identity {
        uid: uid.into(),
        email: None,
        display_name: None,
        provider: "test".into(),
        photo_url: None,
    }
}

pub fn session(uid: &str) -> Session {
    Session {
        identity: identity(uid),
        id_token: Secret::new(format!("token-{uid}")),
        expires_at: None,
    }
}

pub struct Issuer(SigningKey);
impl Issuer {
    pub fn new() -> Self {
        Self(SigningKey::from_bytes(&[7u8; 32]))
    }
    pub fn trust(&self) -> MembershipTrust {
        let mut der = vec![
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
        ];
        der.extend_from_slice(self.0.verifying_key().as_bytes());
        let spki: &'static str = Box::leak(STANDARD.encode(der).into_boxed_str());
        let keys: &'static [CompiledKey] = Box::leak(Box::new([CompiledKey {
            key_id: ISSUER_KEY,
            spki_der_base64: spki,
        }]));
        MembershipTrust::new(Arc::new(PinnedAuthority::new(Activation {
            project: PROJECT,
            mode: Mode::Test,
            policy_version: 1,
            policy_epoch: EPOCH,
            enforcement: codeterm_entitlements::Enforcement {
                enabled: true,
                rollback: false,
            },
            keys,
        })))
    }
    pub fn sign(&self, payload: &serde_json::Value) -> DetachedGrant {
        let text = codeterm_entitlements::jcs::encode(payload).unwrap();
        let signature = self.0.sign(text.as_bytes()).to_bytes();
        DetachedGrant {
            payload: URL_SAFE_NO_PAD.encode(text.as_bytes()),
            signature: URL_SAFE_NO_PAD.encode(signature),
            key_id: ISSUER_KEY.into(),
            alg: "Ed25519".into(),
        }
    }
    pub fn membership_payload(
        uid: &str,
        key_id: &str,
        generation: u64,
        slot: u64,
        issued_at: u64,
    ) -> serde_json::Value {
        json!({
            "schema_version": 1,
            "iss": "domios-mesh-membership",
            "aud": {"project": PROJECT, "env": "test"},
            "sub": uid,
            "installation_id": INSTALLATION,
            "key_id": key_id,
            "key_generation": generation,
            "slot_revision": slot,
            "authority_fingerprint": "f".repeat(64),
            "policy_epoch": EPOCH,
            "issued_at": issued_at,
            "expires_at": issued_at + 3600,
        })
    }
    pub fn permit_payload(
        uid: &str,
        holder: &MeshTarget,
        peer: &MeshTarget,
        transcript_sha256: &str,
        issued_at: u64,
    ) -> serde_json::Value {
        json!({
            "schema_version": 1, "iss": "domios-mesh-link",
            "aud": {"project": PROJECT, "env": "test"}, "sub": uid,
            "holder": holder, "peer": peer, "transcript_sha256": transcript_sha256,
            "authority_fingerprint": "e".repeat(64), "policy_epoch": EPOCH,
            "issued_at": issued_at, "expires_at": issued_at + 120,
        })
    }
}

#[derive(Default)]
pub struct TestClock(AtomicU64);
impl TestClock {
    pub fn set(&self, now: u64) {
        self.0.store(now, Ordering::SeqCst);
    }
}
impl Clock for TestClock {
    fn now(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
pub struct Script {
    pub membership: Option<RegistryRefusal>,
    pub lose_membership_reply: Option<RegistryRefusal>,
    pub lose_key_reply: Option<RegistryRefusal>,
    pub publication: Option<RegistryRefusal>,
    pub permit: Option<RegistryRefusal>,
    pub gate: Option<Arc<Notify>>,
    pub peer_unpublished: bool,
    /// Seconds the issuer's clock runs ahead of the desktop's.
    pub issuer_ahead: u64,
}

#[derive(Default)]
struct Account {
    key: Option<String>,
    key_generation: u64,
    slot: Option<u64>,
    metadata_revision: u64,
    endpoint: Option<MeshEndpointObservation>,
    inactive: bool,
}

/// Server double: checks every installation proof against the enrolled key and the exact
/// challenged body, then answers like the reviewed admission callable.
pub struct FakeRegistry {
    issuer: Issuer,
    clock: Arc<TestClock>,
    log: Log,
    pub script: Mutex<Script>,
    pub entered: Notify,
    accounts: Mutex<HashMap<String, Account>>,
    challenged: Mutex<Option<serde_json::Value>>,
}

impl FakeRegistry {
    fn new(clock: Arc<TestClock>, log: Log) -> Self {
        Self {
            issuer: Issuer::new(),
            clock,
            log,
            script: Mutex::new(Script::default()),
            entered: Notify::new(),
            accounts: Mutex::new(HashMap::new()),
            challenged: Mutex::new(None),
        }
    }

    fn admit(&self, session: &Session, label: &str) {
        let token = session.id_token.expose();
        note(&self.log, format!("registry:{label}:{token}"));
    }

    fn verify<T: serde::Serialize>(
        &self,
        session: &Session,
        endpoint: &str,
        body: &T,
        proof: &InstallationProof,
        spki: Option<&str>,
    ) -> RegistryResult<()> {
        let body = serde_json::to_value(body).unwrap();
        if self.challenged.lock().unwrap().take().as_ref() != Some(&body) {
            return Err(RegistryRefusal::Conflict);
        }
        let uid = &session.identity.uid;
        let spki = match spki {
            Some(spki) => spki.to_owned(),
            None => self
                .accounts
                .lock()
                .unwrap()
                .get(uid)
                .and_then(|account| account.key.clone())
                .ok_or(RegistryRefusal::Forbidden)?,
        };
        let digest = super::super::key::jcs_sha256_hex(&body).unwrap();
        let signed = json!({
            "domain": "domios.online-installation-proof.v1",
            "project": PROJECT, "mode": "test", "uid": uid, "installation_id": INSTALLATION,
            "policy_epoch": EPOCH, "method": "POST", "endpoint": endpoint, "body_sha256": digest,
            "schema_version": proof.schema_version, "key_id": proof.key_id,
            "generation": proof.generation, "timestamp": proof.timestamp, "nonce": proof.nonce,
        });
        let bytes = codeterm_entitlements::jcs::encode(&signed).unwrap();
        let der = STANDARD.decode(&spki).unwrap();
        let key = codeterm_entitlements::policy::parse_spki(&spki).unwrap();
        let signature: [u8; 64] = URL_SAFE_NO_PAD
            .decode(&proof.signature)
            .unwrap()
            .try_into()
            .unwrap();
        let valid = key
            .verify_strict(bytes.as_bytes(), &Signature::from_bytes(&signature))
            .is_ok()
            && proof.key_id == super::super::key::sha256_hex(&der);
        valid.then_some(()).ok_or(RegistryRefusal::Forbidden)
    }

    pub fn slot(&self, uid: &str) -> Option<u64> {
        self.accounts.lock().unwrap().get(uid).and_then(|a| a.slot)
    }

    pub fn revoke_self(&self, uid: &str) {
        let mut accounts = self.accounts.lock().unwrap();
        let account = accounts.get_mut(uid).expect("account");
        account.slot = None;
        account.inactive = true;
    }

    pub fn endpoint(&self, uid: &str) -> Option<MeshEndpointObservation> {
        self.accounts
            .lock()
            .unwrap()
            .get(uid)
            .and_then(|a| a.endpoint.clone())
    }

    fn rows(&self, uid: &str, key_id: &str) -> Vec<MeshManagementRow> {
        let peer_unpublished = self.script.lock().unwrap().peer_unpublished;
        let accounts = self.accounts.lock().unwrap();
        let account = accounts.get(uid);
        let mut own = row(
            INSTALLATION,
            account.map_or(0, |a| a.metadata_revision),
            None,
        );
        own.key_id = key_id.to_owned();
        own.key_generation = account.map_or(1, |a| a.key_generation.max(1));
        own.slot_revision = account.and_then(|a| a.slot).unwrap_or(1);
        own.active = account.is_none_or(|account| !account.inactive);
        own.endpoint = account.and_then(|a| a.endpoint.clone());
        let mut peer = row(PEER, 0, Some("Laptop"));
        peer.endpoint = (!peer_unpublished).then(|| MeshEndpointObservation {
            tailnet_ip: "100.101.102.103".into(),
            api_port: 7685,
            expires_at: self.clock.now() + 600,
        });
        vec![own, peer]
    }
}

#[async_trait]
impl MeshRegistry for FakeRegistry {
    async fn challenge(
        &self,
        session: &Session,
        request: &ChallengeRequest,
    ) -> RegistryResult<String> {
        self.admit(session, "challenge");
        *self.challenged.lock().unwrap() = Some(request.body.clone());
        Ok(format!(
            "{}.{}.{}",
            self.clock.now(),
            "a".repeat(43),
            "b".repeat(43)
        ))
    }

    async fn enroll_key(
        &self,
        session: &Session,
        request: &InstallationEnrollment,
    ) -> RegistryResult<KeyEnrollment> {
        let label = match &request.body.operation {
            KeyOperation::Enroll => "enroll_key",
            KeyOperation::Recover => "recover_key",
        };
        self.admit(session, label);
        let spki = request.body.public_key_spki_base64.clone();
        self.verify(
            session,
            "enrollInstallation",
            &request.body,
            &request.proof,
            Some(&spki),
        )?;
        let generation = request
            .body
            .expected_generation
            .checked_add(1)
            .ok_or(RegistryRefusal::Invalid)?;
        if request.proof.generation != generation {
            return Err(RegistryRefusal::Invalid);
        }
        let mut accounts = self.accounts.lock().unwrap();
        let account = accounts.entry(session.identity.uid.clone()).or_default();
        match &request.body.operation {
            KeyOperation::Enroll
                if account.key.is_none() && request.body.expected_generation == 0 => {}
            KeyOperation::Recover
                if account.key.is_some()
                    && account.key_generation == request.body.expected_generation => {}
            _ => return Err(RegistryRefusal::Conflict),
        }
        account.key = Some(spki);
        account.key_generation = generation;
        if let Some(refusal) = self.script.lock().unwrap().lose_key_reply.take() {
            return Err(refusal);
        }
        Ok(KeyEnrollment {
            generation,
            key_id: request.proof.key_id.clone(),
        })
    }

    async fn admission(
        &self,
        session: &Session,
        envelope: &SignedEnvelope<MeshAdmissionBody>,
    ) -> RegistryResult<MeshResult> {
        let label = match &envelope.request.operation {
            MeshOperation::Enroll { .. } => "enroll",
            MeshOperation::Renew { .. } => "renew",
            MeshOperation::Revoke { .. } => "revoke",
            MeshOperation::Recover { .. } => "recover",
            MeshOperation::List { .. } => "list",
            MeshOperation::Update { .. } => "update",
            MeshOperation::Publish { .. } => "publish",
            MeshOperation::Withdraw { .. } => "withdraw",
            MeshOperation::Link { .. } => "link",
        };
        self.admit(session, label);
        let membership_op = matches!(
            envelope.request.operation,
            MeshOperation::Enroll { .. } | MeshOperation::Renew { .. }
        );
        if membership_op {
            self.entered.notify_one();
            let gate = self.script.lock().unwrap().gate.clone();
            if let Some(gate) = gate {
                gate.notified().await;
            }
        }
        let proof = &envelope.installation_proof;
        self.verify(session, "meshAdmission", &envelope.request, proof, None)?;
        let script = {
            let script = self.script.lock().unwrap();
            (
                script.membership,
                script.publication,
                script.permit,
                script.issuer_ahead,
            )
        };
        let lost_membership_reply = if membership_op {
            self.script.lock().unwrap().lose_membership_reply.take()
        } else {
            None
        };
        let uid = session.identity.uid.clone();
        let now = self.clock.now() + script.3;
        let mut accounts = self.accounts.lock().unwrap();
        let account = accounts.entry(uid.clone()).or_default();
        match &envelope.request.operation {
            MeshOperation::Enroll { .. } | MeshOperation::Renew { .. } => {
                if let Some(refusal) = script.0 {
                    return Err(refusal);
                }
                let slot = match &envelope.request.operation {
                    MeshOperation::Enroll {
                        expected_slot_revision: 0,
                    } if account.slot.is_none() => 1,
                    MeshOperation::Enroll { .. } => return Err(RegistryRefusal::Conflict),
                    MeshOperation::Renew {
                        expected_slot_revision,
                    } if account.slot == Some(*expected_slot_revision) => {
                        expected_slot_revision + 1
                    }
                    MeshOperation::Renew { .. } => return Err(RegistryRefusal::Conflict),
                    _ => return Err(RegistryRefusal::Invalid),
                };
                account.slot = Some(slot);
                account.inactive = false;
                let payload =
                    Issuer::membership_payload(&uid, &proof.key_id, proof.generation, slot, now);
                let result = MeshResult::Membership(MembershipResult {
                    operation: match &envelope.request.operation {
                        MeshOperation::Enroll { .. } => MembershipOperation::Enroll,
                        MeshOperation::Renew { .. } => MembershipOperation::Renew,
                        _ => return Err(RegistryRefusal::Invalid),
                    },
                    installation_id: INSTALLATION.into(),
                    slot_revision: slot,
                    registry_revision: slot,
                    membership: self.issuer.sign(&payload),
                });
                if let Some(refusal) = lost_membership_reply {
                    return Err(refusal);
                }
                Ok(result)
            }
            MeshOperation::Publish {
                expected_metadata_revision,
                endpoint,
            } => {
                if let Some(refusal) = script.1 {
                    return Err(refusal);
                }
                if *expected_metadata_revision != account.metadata_revision {
                    return Err(RegistryRefusal::Conflict);
                }
                let observation = MeshEndpointObservation {
                    tailnet_ip: endpoint.tailnet_ip.clone(),
                    api_port: endpoint.api_port,
                    expires_at: now + 3600,
                };
                account.metadata_revision += 1;
                account.endpoint = Some(observation.clone());
                Ok(MeshResult::Published(PublishedResult {
                    installation_id: INSTALLATION.into(),
                    endpoint: observation,
                    previous_metadata_revision: *expected_metadata_revision,
                    metadata_revision: account.metadata_revision,
                    slot_revision: account.slot.unwrap_or(1),
                    registry_revision: 9,
                }))
            }
            MeshOperation::Withdraw {
                expected_metadata_revision,
            } => {
                if let Some(refusal) = script.1 {
                    return Err(refusal);
                }
                if *expected_metadata_revision != account.metadata_revision {
                    return Err(RegistryRefusal::Conflict);
                }
                account.metadata_revision += 1;
                account.endpoint = None;
                Ok(MeshResult::Withdrawn(WithdrawnResult {
                    installation_id: INSTALLATION.into(),
                    previous_metadata_revision: *expected_metadata_revision,
                    metadata_revision: account.metadata_revision,
                    slot_revision: account.slot.unwrap_or(1),
                    registry_revision: 10,
                }))
            }
            MeshOperation::List { .. } => {
                drop(accounts);
                Ok(MeshResult::Page(PageResult {
                    registry_revision: 7,
                    rows: self.rows(&uid, &proof.key_id),
                    cursor: None,
                }))
            }
            MeshOperation::Update {
                expected_metadata_revision,
                display_name,
            } => {
                if *expected_metadata_revision != account.metadata_revision {
                    return Err(RegistryRefusal::Conflict);
                }
                account.metadata_revision += 1;
                Ok(MeshResult::Updated(UpdatedResult {
                    installation_id: INSTALLATION.into(),
                    display_name: display_name.clone(),
                    previous_metadata_revision: *expected_metadata_revision,
                    metadata_revision: account.metadata_revision,
                    slot_revision: account.slot.unwrap_or(1),
                    registry_revision: 8,
                }))
            }
            MeshOperation::Revoke { target } => Ok(MeshResult::Revoked(RevokedResult {
                target_installation_id: target.installation_id.clone(),
                target_key_id: target.key_id.clone(),
                target_key_generation: target.key_generation,
                previous_slot_revision: target.slot_revision,
                slot_revision: target.slot_revision + 1,
                registry_revision: 11,
            })),
            MeshOperation::Link {
                expected_slot_revision,
                peer,
                transcript_sha256,
            } => {
                if let Some(refusal) = script.2 {
                    return Err(refusal);
                }
                let holder = MeshTarget {
                    installation_id: INSTALLATION.into(),
                    key_id: proof.key_id.clone(),
                    key_generation: proof.generation,
                    slot_revision: *expected_slot_revision,
                };
                let payload = Issuer::permit_payload(&uid, &holder, peer, transcript_sha256, now);
                Ok(MeshResult::Permit(PermitResult {
                    installation_id: INSTALLATION.into(),
                    peer_installation_id: peer.installation_id.clone(),
                    slot_revision: *expected_slot_revision,
                    registry_revision: 12,
                    permit: self.issuer.sign(&payload),
                }))
            }
            MeshOperation::Recover { .. } => Err(RegistryRefusal::Invalid),
        }
    }
}

pub fn row(id: &str, metadata_revision: u64, name: Option<&str>) -> MeshManagementRow {
    MeshManagementRow {
        installation_id: id.into(),
        key_id: "c".repeat(64),
        key_generation: 1,
        slot_revision: 1,
        active: true,
        admitted_at: START,
        authenticated_at: START,
        display_name: name.map(str::to_owned),
        metadata_revision,
        publication: PublicationState::Complete,
        membership_expires_at: Some(START + 3600),
        eligibility: Eligibility::Eligible,
        endpoint: None,
    }
}

#[derive(Default)]
pub struct FakeSessions(pub Mutex<Option<Session>>);
#[async_trait]
impl SessionSource for FakeSessions {
    async fn session(&self) -> Result<Option<Session>, AccountMeshRefusal> {
        Ok(self.0.lock().unwrap().clone())
    }
}

#[derive(Default)]
pub struct MemoryKeys(pub Mutex<Option<Secret>>);
#[async_trait]
impl InstallationKeyStore for MemoryKeys {
    async fn load(&self) -> crate::Result<Option<Secret>> {
        Ok(self.0.lock().unwrap().clone())
    }
    async fn save(&self, secret: &Secret) -> crate::Result<()> {
        *self.0.lock().unwrap() = Some(secret.clone());
        Ok(())
    }
}

pub struct MemoryStore {
    pub record: Mutex<AccountMeshRecord>,
    pub corrupt: Mutex<bool>,
    pub fail_save: Mutex<bool>,
    log: Log,
}
#[async_trait]
impl AccountMeshStore for MemoryStore {
    async fn load(&self) -> io::Result<AccountMeshRecord> {
        if *self.corrupt.lock().unwrap() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(self.record.lock().unwrap().clone())
    }
    async fn save(&self, record: &AccountMeshRecord) -> io::Result<()> {
        if *self.fail_save.lock().unwrap() {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        note(&self.log, format!("save:signed_out={}", record.signed_out));
        *self.record.lock().unwrap() = record.clone();
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeEndpoint(pub Mutex<Option<MeshEndpoint>>);
impl EndpointSource for FakeEndpoint {
    fn ready_endpoint(&self) -> Option<MeshEndpoint> {
        *self.0.lock().unwrap()
    }
}

pub struct FakeTransport {
    log: Log,
    pub revoked: Mutex<Vec<String>>,
    pub last: Mutex<Option<LifecycleState>>,
}
#[async_trait]
impl MeshTransport for FakeTransport {
    async fn apply(&self, state: &LifecycleState) {
        let kind = match state.presence {
            Presence::NeverSignedIn => "never",
            Presence::SignedIn { .. } => "signed_in",
            Presence::SignedOut => "signed_out",
        };
        note(
            &self.log,
            format!("transport:{kind}:{}", state.account_epoch),
        );
        *self.last.lock().unwrap() = Some(state.clone());
    }
    async fn installation_revoked(&self, installation_id: &str) {
        self.revoked
            .lock()
            .unwrap()
            .push(installation_id.to_owned());
    }
}

pub fn tailnet_endpoint() -> MeshEndpoint {
    MeshEndpoint::new(IpAddr::V4(Ipv4Addr::new(100, 64, 1, 2)), 7685).unwrap()
}

pub struct Harness {
    pub mesh: Arc<AccountMesh>,
    pub registry: Arc<FakeRegistry>,
    pub sessions: Arc<FakeSessions>,
    pub keys: Arc<MemoryKeys>,
    pub store: Arc<MemoryStore>,
    pub endpoint: Arc<FakeEndpoint>,
    pub transport: Arc<FakeTransport>,
    pub clock: Arc<TestClock>,
    pub log: Log,
    trust: Option<MembershipTrust>,
    leases: bool,
}

impl Harness {
    pub async fn new() -> Self {
        Self::with_trust(Some(Issuer::new().trust())).await
    }

    pub async fn with_trust(trust: Option<MembershipTrust>) -> Self {
        Self::build(trust, false).await
    }

    pub async fn build(trust: Option<MembershipTrust>, corrupt: bool) -> Self {
        Self::build_with(trust, corrupt, false).await
    }

    /// Lease timers armed, for tests that run on paused tokio time.
    pub async fn with_leases() -> Self {
        Self::build_with(Some(Issuer::new().trust()), false, true).await
    }

    async fn build_with(trust: Option<MembershipTrust>, corrupt: bool, leases: bool) -> Self {
        let log: Log = Arc::default();
        let clock = Arc::new(TestClock::default());
        clock.set(START);
        let registry = Arc::new(FakeRegistry::new(clock.clone(), log.clone()));
        let store = Arc::new(MemoryStore {
            record: Mutex::default(),
            corrupt: Mutex::new(corrupt),
            fail_save: Mutex::new(false),
            log: log.clone(),
        });
        let transport = Arc::new(FakeTransport {
            log: log.clone(),
            revoked: Mutex::default(),
            last: Mutex::default(),
        });
        let mut harness = Self {
            mesh: AccountMesh::new(
                config(None, false),
                ports(&registry, &store, &transport, &clock),
            ),
            registry,
            sessions: Arc::default(),
            keys: Arc::default(),
            store,
            endpoint: Arc::default(),
            transport,
            clock,
            log,
            trust,
            leases,
        };
        harness.mesh = harness.controller();
        harness.mesh.load().await;
        harness
    }

    fn controller(&self) -> Arc<AccountMesh> {
        AccountMesh::new(
            config(self.trust.clone(), self.leases),
            AccountMeshPorts {
                registry: self.registry.clone(),
                sessions: self.sessions.clone(),
                keys: self.keys.clone(),
                store: self.store.clone(),
                endpoint: self.endpoint.clone(),
                transport: self.transport.clone(),
                clock: self.clock.clone(),
            },
        )
    }

    /// A fresh controller over the same durable store, key store and server.
    pub async fn restart(&mut self) {
        self.mesh = self.controller();
        self.mesh.load().await;
    }

    pub async fn sign_in(&self, uid: &str) {
        *self.sessions.0.lock().unwrap() = Some(session(uid));
        self.mesh.identity_changed(Some(&identity(uid))).await;
    }

    /// The service order: Mesh sign-out first, then the account releases its credentials.
    pub async fn logout(&self) -> Cleanup {
        let release = async {
            *self.sessions.0.lock().unwrap() = None;
            self.mesh.identity_changed(None).await;
        };
        self.mesh.sign_out(release).await.0
    }

    pub fn calls(&self, label: &str) -> usize {
        count(&self.log, &format!("registry:{label}:"))
    }

    pub fn state(&self) -> Arc<LifecycleState> {
        self.mesh.state()
    }
}

fn config(trust: Option<MembershipTrust>, schedule_leases: bool) -> AccountMeshConfig {
    AccountMeshConfig {
        installation_id: InstallationId::parse(INSTALLATION.into()).unwrap(),
        trust,
        call_deadline: Duration::from_secs(30),
        withdrawal_deadline: Duration::from_secs(30),
        schedule_leases,
    }
}

fn ports(
    registry: &Arc<FakeRegistry>,
    store: &Arc<MemoryStore>,
    transport: &Arc<FakeTransport>,
    clock: &Arc<TestClock>,
) -> AccountMeshPorts {
    AccountMeshPorts {
        registry: registry.clone(),
        sessions: Arc::new(FakeSessions::default()),
        keys: Arc::new(MemoryKeys::default()),
        store: store.clone(),
        endpoint: Arc::new(FakeEndpoint::default()),
        transport: transport.clone(),
        clock: clock.clone(),
    }
}

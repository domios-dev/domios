mod proofs;
mod registry;
use crate::account_mesh::wire::*;
use crate::account_mesh::*;
use crate::{Identity, InstallationId, Secret, Session};
use async_trait::async_trait;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use codeterm_entitlements::{Activation, CompiledKey, DetachedGrant, Mode, PinnedAuthority};
use ed25519_dalek::{Signature, Signer, SigningKey};
use proofs::ProofChallenges;
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
        login_generation: 0,
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
    pub update: Option<RegistryRefusal>,
    pub update_race: Option<String>,
    pub reject_update_after_race: bool,
    pub sync_cursor: Option<MeshCursor>,
    pub rename_race: Option<String>,
    pub enrollment: Option<RegistryRefusal>,
    pub challenge: Option<RegistryRefusal>,
    pub challenge_once: Option<RegistryRefusal>,
    pub membership_seconds: Option<u64>,
    pub disable_renew_replay: bool,
    pub renew_bodies: Vec<MeshAdmissionBody>,
    pub key_requests: Vec<InstallationEnrollment>,
    pub recovery_bodies: Vec<MeshAdmissionBody>,
    pub recovery_prepared: Vec<SignedEnvelope<MeshAdmissionBody>>,
    pub endpoint_seconds: Option<u64>,
    pub membership_registry_revision: Option<u64>,
    pub membership_signed_registry: Option<DetachedGrant>,
    pub registry_revision: Option<u64>,
    pub enrollment_gate: Option<Arc<Notify>>,
    pub membership: Option<RegistryRefusal>,
    pub membership_once: Option<RegistryRefusal>,
    pub lose_membership_reply: Option<RegistryRefusal>,
    pub lose_key_reply: Option<RegistryRefusal>,
    pub publication: Option<RegistryRefusal>,
    pub presence: Option<RegistryRefusal>,
    pub list: Option<RegistryRefusal>,
    pub gate: Option<Arc<Notify>>,
    pub list_gate: Option<Arc<Notify>>,
    pub sync_gate: Option<Arc<Notify>>,
    pub peer_unpublished: bool,
    pub peer_key: Option<(String, u64)>,
    pub peer_active: Option<bool>,
    pub peer_presence: RegistryPresence,
    pub peer_name: Option<String>,
    pub peer_metadata_revision: u64,
    pub peer_membership_expires_at: Option<u64>,
    /// Seconds the issuer's clock runs ahead of the desktop's.
    pub issuer_ahead: u64,
}

#[derive(Default)]
struct Account {
    display_name: Option<String>,
    key: Option<String>,
    key_generation: u64,
    revoked_generation: u64,
    slot: Option<u64>,
    slot_key: Option<(String, u64)>,
    renew_receipt: Option<(MeshAdmissionBody, MeshResult)>,
    recovery_receipt: Option<(SignedEnvelope<MeshAdmissionBody>, MeshResult)>,
    metadata_revision: u64,
    endpoint: Option<MeshEndpointObservation>,
    inactive: bool,
    presence: RegistryPresence,
}

/// Server double: checks every installation proof against the enrolled key and the exact
/// challenged body, then answers like the reviewed admission callable.
pub struct FakeRegistry {
    issuer: Issuer,
    revision: AtomicU64,
    clock: Arc<TestClock>,
    log: Log,
    pub script: Mutex<Script>,
    pub entered: Notify,
    pub list_entered: Notify,
    pub sync_entered: Notify,
    accounts: Mutex<HashMap<String, Account>>,
    challenged: Mutex<ProofChallenges>,
}

impl FakeRegistry {
    fn new(clock: Arc<TestClock>, log: Log) -> Self {
        Self {
            issuer: Issuer::new(),
            revision: AtomicU64::new(0),
            clock,
            log,
            script: Mutex::new(Script::default()),
            entered: Notify::new(),
            list_entered: Notify::new(),
            sync_entered: Notify::new(),
            accounts: Mutex::new(HashMap::new()),
            challenged: Mutex::new(ProofChallenges::default()),
        }
    }

    fn admit(&self, session: &Session, label: &str) {
        let token = session.id_token.expose();
        note(&self.log, format!("registry:{label}:{token}"));
    }

    pub fn slot(&self, uid: &str) -> Option<u64> {
        self.accounts.lock().unwrap().get(uid).and_then(|a| a.slot)
    }

    pub fn revoke_self(&self, uid: &str) {
        let mut accounts = self.accounts.lock().unwrap();
        let account = accounts.get_mut(uid).expect("account");
        account.inactive = true;
        account.revoked_generation = account.key_generation;
    }

    pub fn presence(&self, uid: &str) -> RegistryPresence {
        self.accounts
            .lock()
            .unwrap()
            .get(uid)
            .map_or(RegistryPresence::Present, |a| a.presence)
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
            account.and_then(|a| a.display_name.as_deref()),
        );
        own.key_id = account
            .and_then(|a| a.slot_key.as_ref())
            .map_or_else(|| key_id.to_owned(), |(key, _)| key.clone());
        own.key_generation = account
            .and_then(|a| a.slot_key.as_ref())
            .map_or(1, |(_, generation)| *generation);
        own.slot_revision = account.and_then(|a| a.slot).unwrap_or(1);
        own.active = account.is_none_or(|account| !account.inactive);
        own.presence = account.map_or(RegistryPresence::Present, |account| account.presence);
        own.endpoint = account.and_then(|a| a.endpoint.clone());
        let mut peer = row(PEER, 0, Some("Laptop"));
        if let Some(active) = self.script.lock().unwrap().peer_active {
            peer.active = active;
        }
        if let Some((key_id, generation)) = self.script.lock().unwrap().peer_key.clone() {
            peer.key_id = key_id;
            peer.key_generation = generation;
        }
        if let Some(name) = self.script.lock().unwrap().peer_name.clone() {
            peer.display_name = Some(name);
        }
        peer.metadata_revision = self.script.lock().unwrap().peer_metadata_revision;
        peer.presence = self.script.lock().unwrap().peer_presence;
        if let Some(expiry) = self.script.lock().unwrap().peer_membership_expires_at {
            peer.membership_expires_at = Some(expiry);
        }
        peer.endpoint = (!peer_unpublished).then(|| MeshEndpointObservation {
            transport: None,
            tailnet_ip: "100.101.102.103".into(),
            api_port: 7685,
            expires_at: self.clock.now() + 600,
        });
        vec![own, peer]
    }
}

pub fn row(id: &str, metadata_revision: u64, name: Option<&str>) -> MeshManagementRow {
    MeshManagementRow {
        installation_id: id.into(),
        key_id: "c".repeat(64),
        key_generation: 1,
        slot_revision: 1,
        active: true,
        presence: RegistryPresence::Present,
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
pub struct FakeSessions(
    pub Mutex<Option<Session>>,
    pub AtomicU64,
    pub Mutex<Option<AccountMeshRefusal>>,
);
#[async_trait]
impl SessionSource for FakeSessions {
    async fn session(&self) -> Result<Option<Session>, AccountMeshRefusal> {
        Ok(self.0.lock().unwrap().clone())
    }
    async fn refresh(&self) -> Result<Option<Session>, AccountMeshRefusal> {
        self.1.fetch_add(1, Ordering::SeqCst);
        if let Some(refusal) = *self.2.lock().unwrap() {
            return Err(refusal);
        }
        let mut session = self.0.lock().unwrap();
        if let Some(session) = session.as_mut() {
            session.id_token = Secret::new("refreshed-token");
        }
        Ok(session.clone())
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

mod store;
pub use store::MemoryStore;

#[derive(Default)]
pub struct FakeEndpoint(pub Mutex<Option<MeshEndpoint>>);
impl EndpointSource for FakeEndpoint {
    fn ready_endpoint(&self) -> Option<MeshEndpoint> {
        self.0.lock().unwrap().clone()
    }
}

pub struct FakeTransport {
    pub peers_reachable: std::sync::atomic::AtomicBool,
    pub endpoints: Mutex<Vec<(Option<MeshEndpoint>, u64, String)>>,
    pub snapshots: Mutex<Vec<DetachedGrant>>,
    log: Log,
    pub revoked: Mutex<Vec<String>>,
    pub last: Mutex<Option<LifecycleState>>,
}
#[async_trait]
impl MeshTransport for FakeTransport {
    fn peers_reachable(&self) -> bool {
        self.peers_reachable
            .load(std::sync::atomic::Ordering::SeqCst)
    }
    async fn endpoint_changed(
        &self,
        endpoint: Option<MeshEndpoint>,
        sequence: u64,
        membership: &str,
    ) -> bool {
        self.endpoints
            .lock()
            .unwrap()
            .push((endpoint, sequence, membership.to_owned()));
        true
    }
    async fn registry_changed(&self, snapshot: &DetachedGrant) {
        self.snapshots.lock().unwrap().push(snapshot.clone());
    }
    async fn presence_changed(&self, metadata_revision: u64) {
        note(&self.log, format!("transport:presence:{metadata_revision}"));
    }
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
    pub default_name: Option<String>,
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
            save_gate: Mutex::new(None),
            save_entered: Notify::new(),
            log: log.clone(),
        });
        let transport = Arc::new(FakeTransport {
            peers_reachable: Default::default(),
            endpoints: Default::default(),
            snapshots: Mutex::default(),
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
            default_name: None,
        };
        harness.mesh = harness.controller();
        harness.mesh.load().await;
        harness
    }

    fn controller(&self) -> Arc<AccountMesh> {
        let mut config = config(self.trust.clone(), self.leases);
        config.default_display_name = self.default_name.clone();
        AccountMesh::new(
            config,
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
        default_display_name: None,
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

pub async fn wait_for_event<T>(event: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), event)
        .await
        .expect("mesh test event did not arrive within its deadline")
}

/// Build a complete server-signed successor without using a registry read port.
pub fn registry_successor(
    harness: &Harness,
    change: impl FnOnce(&mut Vec<MeshManagementRow>),
) -> DetachedGrant {
    let state = harness.state();
    let mut rows = state.registry.as_ref().unwrap().rows.clone();
    change(&mut rows);
    let revoked: Vec<_> = rows
        .iter()
        .filter(|row| !row.active)
        .map(|row| row.installation_id.clone())
        .collect();
    Issuer::new().sign(&json!({
        "schema_version": 1, "iss": "domios-mesh-registry",
        "aud": {"project": PROJECT, "env": "test"}, "policy_epoch": EPOCH,
        "account_id": state.signed_in_account().unwrap(),
        "revision": state.signed_registry.as_ref().unwrap().revision + 1,
        "issued_at": harness.clock.now(), "members": rows, "revoked_installations": revoked
    }))
}

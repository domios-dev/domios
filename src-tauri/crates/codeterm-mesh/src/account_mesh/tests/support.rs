use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::account_mesh::link::LinkTransport;
use crate::account_mesh::protocol::{
    AdoptCommit, AdoptOffer, LinkApproval, LinkApprovalReply, LinkCommit, LinkCommitReply,
    LinkConfirm, LinkConfirmReply, LinkHello, ADOPT_COMMIT_PATH, ADOPT_OFFER_PATH,
    LINK_APPROVAL_PATH, LINK_COMMIT_PATH, LINK_CONFIRM_PATH, LINK_HELLO_PATH,
};
use crate::account_mesh::{
    AccountMeshAuthority, AccountMeshState, AccountPresence, AdmissionRefusal, BoxFuture,
    LinkAuthorizationRequest, LinkPermitExpectation, LinkTarget, SignedMembership,
    VerifiedMembership,
};
use crate::types::{AccessLevel, MeshIdentity, PairedPeer, PeerAddress, RELAY_PROTOCOL_VERSION};
use crate::{MeshManager, MeshState};

pub const ACCOUNT: &str = "account-a";
pub const OTHER_ACCOUNT: &str = "account-b";
pub const START: u64 = 10_000;
pub const MEMBERSHIP_TTL: u64 = 3_600;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn membership_token(account: &str, installation: &str, expires_at: u64) -> String {
    format!("m|{account}|{installation}|{expires_at}")
}

pub fn membership(account: &str, installation: &str, expires_at: u64) -> SignedMembership {
    SignedMembership {
        token: membership_token(account, installation, expires_at),
        verified: VerifiedMembership {
            account_id: account.into(),
            installation_id: installation.into(),
            key_id: format!("key-{installation}"),
            key_generation: 1,
            slot_revision: 1,
            issued_at: 0,
            expires_at,
        },
    }
}

/// Deterministic account authority: possession is bound to the installation id, permits to
/// the transcript and both installations; `online` models backend reachability.
pub struct FakeAuthority {
    pub installation_id: String,
    pub online: AtomicBool,
    pub authorize_calls: AtomicUsize,
    pub registry: Mutex<HashMap<String, LinkTarget>>,
}

impl FakeAuthority {
    pub fn new(installation_id: &str) -> Arc<Self> {
        Arc::new(Self {
            installation_id: installation_id.into(),
            online: AtomicBool::new(true),
            authorize_calls: AtomicUsize::new(0),
            registry: Mutex::new(HashMap::new()),
        })
    }

    pub(super) fn permit(holder: &str, peer: &str, account: &str, transcript: &[u8; 32]) -> String {
        format!("permit|{holder}|{peer}|{account}|{}", hex(transcript))
    }
}

impl AccountMeshAuthority for FakeAuthority {
    fn verify_membership(
        &self,
        token: &str,
        now: u64,
    ) -> Result<VerifiedMembership, AdmissionRefusal> {
        let parts: Vec<&str> = token.split('|').collect();
        let [_, account, installation, expires] = parts.as_slice() else {
            return Err(AdmissionRefusal::handshake("bad membership"));
        };
        let expires: u64 = expires
            .parse()
            .map_err(|_| AdmissionRefusal::handshake("bad expiry"))?;
        if now >= expires {
            return Err(AdmissionRefusal::handshake("membership expired"));
        }
        Ok(membership(account, installation, expires).verified)
    }

    fn sign_possession(&self, transcript: &[u8; 32]) -> Result<String, AdmissionRefusal> {
        Ok(format!("p|{}|{}", self.installation_id, hex(transcript)))
    }

    fn verify_possession(
        &self,
        membership: &VerifiedMembership,
        transcript: &[u8; 32],
        signature: &str,
    ) -> bool {
        signature == format!("p|{}|{}", membership.installation_id, hex(transcript))
    }

    fn authorize_link(
        &self,
        request: LinkAuthorizationRequest,
    ) -> BoxFuture<'_, Result<String, AdmissionRefusal>> {
        Box::pin(async move {
            if !self.online.load(Ordering::SeqCst) {
                return Err(AdmissionRefusal::Offline);
            }
            self.authorize_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Self::permit(
                &request.local_installation_id,
                &request.peer_installation_id,
                &request.account_id,
                &request.transcript,
            ))
        })
    }

    fn verify_link_permit(
        &self,
        permit: &str,
        expected: &LinkPermitExpectation,
        _now: u64,
    ) -> Result<(), AdmissionRefusal> {
        let wanted = Self::permit(
            &expected.holder_installation_id,
            &expected.peer_installation_id,
            &expected.account_id,
            &expected.transcript,
        );
        if permit == wanted {
            Ok(())
        } else {
            Err(AdmissionRefusal::handshake("permit mismatch"))
        }
    }

    fn resolve_link_target(
        &self,
        installation_id: &str,
    ) -> BoxFuture<'_, Result<LinkTarget, AdmissionRefusal>> {
        let found = self.registry.lock().get(installation_id).cloned();
        Box::pin(async move {
            if !self.online.load(Ordering::SeqCst) {
                return Err(AdmissionRefusal::Offline);
            }
            found.ok_or(AdmissionRefusal::PeerUnreachable)
        })
    }
}

pub struct Machine {
    pub mesh: MeshState,
    pub authority: Arc<FakeAuthority>,
    pub installation_id: String,
    pub ip: String,
    pub port: u16,
}

impl Machine {
    pub fn id(&self) -> String {
        self.mesh.lock().ident.identity.instance_id.clone()
    }

    pub fn public_key(&self) -> String {
        self.mesh
            .lock()
            .ident
            .identity
            .public_key
            .clone()
            .expect("test identity has a key")
    }

    pub fn as_peer(&self) -> PairedPeer {
        PairedPeer {
            instance_id: self.id(),
            instance_name: format!("machine-{}", self.installation_id),
            addresses: vec![PeerAddress::from_ip(&self.ip)],
            api_port: self.port,
            public_key: self.public_key(),
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
        }
    }

    pub fn sign_in(&self, epoch: u64, account: &str, expires_at: u64) {
        self.mesh.lock().apply_account_state(AccountMeshState {
            account_epoch: epoch,
            presence: AccountPresence::SignedIn {
                account_id: account.into(),
            },
            membership: Some(membership(account, &self.installation_id, expires_at)),
            revoked_installations: BTreeSet::new(),
            unavailable_installations: BTreeSet::new(),
            plan_limit_locked: false,
        });
    }

    pub fn sign_out(&self, epoch: u64) {
        self.mesh.lock().apply_account_state(AccountMeshState {
            account_epoch: epoch,
            presence: AccountPresence::SignedOut,
            membership: None,
            revoked_installations: BTreeSet::new(),
            unavailable_installations: BTreeSet::new(),
            plan_limit_locked: false,
        });
    }
}

pub fn clock() -> Arc<AtomicU64> {
    Arc::new(AtomicU64::new(START))
}

pub fn machine(index: u8, clock: &Arc<AtomicU64>) -> Machine {
    let (private_key, public_key) = crate::crown::generate_peer_keypair();
    let identity = MeshIdentity {
        instance_id: uuid::Uuid::new_v4().to_string(),
        private_key: Some(private_key),
        public_key: Some(public_key),
    };
    let mut manager = MeshManager::in_memory(identity, format!("machine-{index}"), 7685);
    let now = clock.clone();
    manager.set_clock(Arc::new(move || now.load(Ordering::SeqCst)));
    let installation_id = format!("inst-{index}");
    let authority = FakeAuthority::new(&installation_id);
    manager.bind_account_authority(authority.clone());
    Machine {
        mesh: Arc::new(Mutex::new(manager)),
        authority,
        installation_id,
        ip: format!("100.64.0.{index}"),
        port: 7685,
    }
}

/// Publishes each machine in every machine's registry view.
pub fn enroll_all(machines: &[&Machine]) {
    for viewer in machines {
        let mut registry = viewer.authority.registry.lock();
        for target in machines {
            registry.insert(
                target.installation_id.clone(),
                LinkTarget {
                    installation_id: target.installation_id.clone(),
                    ip: target.ip.clone(),
                    api_port: target.port,
                },
            );
        }
    }
}

type AfterStep = Arc<dyn Fn(&'static str) + Send + Sync>;

/// Direct in-process P2P: routes by the target endpoint to the responder's own handlers.
pub struct Loopback {
    routes: HashMap<(String, u16), MeshState>,
    after: Option<AfterStep>,
    pub posts: AtomicUsize,
    lose_reply_of: Option<&'static str>,
    approval_decision: Option<bool>,
    approval_entered: Option<Arc<tokio::sync::Notify>>,
}

impl Loopback {
    pub fn new(machines: &[&Machine]) -> Self {
        Self {
            routes: machines
                .iter()
                .map(|m| ((m.ip.clone(), m.port), m.mesh.clone()))
                .collect(),
            after: None,
            posts: AtomicUsize::new(0),
            lose_reply_of: None,
            approval_decision: Some(true),
            approval_entered: None,
        }
    }

    /// The responder handles `path`, but its reply never reaches the initiator.
    pub fn lose_reply_of(mut self, path: &'static str) -> Self {
        self.lose_reply_of = Some(path);
        self
    }

    pub fn hold_approval(mut self, entered: Arc<tokio::sync::Notify>) -> Self {
        self.approval_decision = None;
        self.approval_entered = Some(entered);
        self
    }

    pub fn after_each(mut self, after: AfterStep) -> Self {
        self.after = Some(after);
        self
    }
}

fn decode<T: serde::de::DeserializeOwned>(body: serde_json::Value) -> Result<T, AdmissionRefusal> {
    serde_json::from_value(body).map_err(|e| AdmissionRefusal::handshake(e.to_string()))
}

fn encode<T: serde::Serialize>(value: T) -> Result<serde_json::Value, AdmissionRefusal> {
    serde_json::to_value(value).map_err(|e| AdmissionRefusal::handshake(e.to_string()))
}

fn caller(mesh: &MeshState, bearer: Option<String>) -> Result<String, AdmissionRefusal> {
    let token = bearer.ok_or(AdmissionRefusal::Unauthenticated)?;
    let mut manager = mesh.lock();
    let peer = manager
        .authenticate_request(&token)
        .ok_or(AdmissionRefusal::Unauthenticated)?;
    manager
        .check_and_record_nonce(&token)
        .map_err(|_| AdmissionRefusal::Unauthenticated)?;
    Ok(peer)
}

impl LinkTransport for Loopback {
    fn post(
        &self,
        ip: &str,
        port: u16,
        path: &'static str,
        bearer: Option<String>,
        body: serde_json::Value,
    ) -> BoxFuture<'_, Result<serde_json::Value, AdmissionRefusal>> {
        let target = self.routes.get(&(ip.to_string(), port)).cloned();
        let after = self.after.clone();
        let lost = self.lose_reply_of == Some(path);
        let approval_decision = self.approval_decision;
        let approval_entered = self.approval_entered.clone();
        self.posts.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let mesh = target.ok_or(AdmissionRefusal::PeerUnreachable)?;
            let reply = match path {
                LINK_HELLO_PATH => {
                    let hello: LinkHello = decode(body)?;
                    let reply = mesh.lock().accept_link_hello(hello)?;
                    encode(reply)
                }
                LINK_APPROVAL_PATH => {
                    let proof: LinkApproval = decode(body)?;
                    mesh.lock().request_link_approval(&proof)?;
                    if let Some(entered) = approval_entered {
                        entered.notify_one();
                    }
                    if let Some(approve) = approval_decision {
                        mesh.lock().decide_link(&proof.link_id, approve)?;
                    }
                    crate::account_mesh::approval::wait_for_link_approval(&mesh, &proof.link_id)
                        .await?;
                    encode(LinkApprovalReply { approved: true })
                }
                LINK_CONFIRM_PATH => {
                    let confirm: LinkConfirm = decode(body)?;
                    let confirmation = mesh.lock().begin_link_confirm(&confirm)?;
                    let permit = confirmation
                        .authority
                        .authorize_link(confirmation.request.clone())
                        .await?;
                    mesh.lock().finish_link_confirm(confirmation)?;
                    encode(LinkConfirmReply { permit })
                }
                LINK_COMMIT_PATH => {
                    let commit: LinkCommit = decode(body)?;
                    mesh.lock().commit_link(&commit)?;
                    encode(LinkCommitReply { committed: true })
                }
                ADOPT_OFFER_PATH => {
                    let peer = caller(&mesh, bearer)?;
                    let offer: AdoptOffer = decode(body)?;
                    let reply = mesh.lock().accept_adopt_offer(&peer, offer)?;
                    encode(reply)
                }
                ADOPT_COMMIT_PATH => {
                    let peer = caller(&mesh, bearer)?;
                    let commit: AdoptCommit = decode(body)?;
                    let reply = mesh.lock().commit_adoption(&peer, commit)?;
                    encode(reply)
                }
                _ => Err(AdmissionRefusal::handshake("unknown path")),
            };
            if let Some(after) = after {
                after(path);
            }
            if lost {
                return Err(AdmissionRefusal::PeerUnreachable);
            }
            reply
        })
    }
}

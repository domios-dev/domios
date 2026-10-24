use super::key::{jcs_sha256_hex, sha256_hex, InstallationKey, ProofContext};
use super::lifecycle::AccountMesh;
use super::membership::{
    account_id, membership_token, parse_membership_token, verify_link_permit, verify_membership,
    LinkPermitExpectation, MembershipExpectation, MembershipRefusal, MembershipTrust,
    VerifiedMembership,
};
use super::ports::RegistryResult;
use super::state::{
    AccountMeshRefusal, EnrollTrigger, Enrollment, KeyBinding, Presence, Publication,
    RegistryObservation, SelfInstallation, SignedMembership,
};
use super::wire::*;
use crate::Session;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::Serialize;
use std::future::Future;
use std::sync::Arc;

type Outcome<T> = Result<T, AccountMeshRefusal>;

const MAX_LIST_PAGES: usize = 8;

fn enrollment_requires_reauthentication(session: &Session, now: u64) -> bool {
    let Some(payload) = session.id_token.expose().split('.').nth(1) else {
        return false;
    };
    let auth_time = URL_SAFE_NO_PAD.decode(payload).ok().and_then(|bytes| {
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()?
            .get("auth_time")?
            .as_u64()
    });
    auth_time.is_some_and(|auth_time| now.saturating_sub(auth_time) > 300)
}

pub(super) fn request_id() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Everything one signed call needs, captured for a single account epoch.
pub(super) struct Signed {
    pub(super) epoch: u64,
    pub(super) session: Session,
    pub(super) account: String,
    pub(super) trust: MembershipTrust,
    pub(super) key: Arc<InstallationKey>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkAuthorization {
    pub transcript: [u8; 32],
    pub local_installation_id: String,
    pub peer_installation_id: String,
    pub account_id: String,
}

/// A new-link address read online from the same account's registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkTarget {
    pub target: MeshTarget,
    pub endpoint: MeshEndpoint,
}

impl AccountMesh {
    pub(super) fn installation(&self) -> String {
        self.config.installation_id.as_str().to_owned()
    }

    async fn call<T>(&self, call: impl Future<Output = RegistryResult<T>>) -> Outcome<T> {
        tokio::time::timeout(self.config.call_deadline, call)
            .await
            .map_err(|_| AccountMeshRefusal::Offline)?
            .map_err(AccountMeshRefusal::from)
    }

    pub(super) async fn key(&self) -> Outcome<Arc<InstallationKey>> {
        if let Some(key) = self.with(|inner| inner.key.clone()) {
            return Ok(key);
        }
        let _guard = self.key_lock.lock().await;
        if let Some(key) = self.with(|inner| inner.key.clone()) {
            return Ok(key);
        }
        let stored = self
            .ports
            .keys
            .load()
            .await
            .map_err(|_| AccountMeshRefusal::Unavailable)?;
        let key = match stored {
            Some(secret) => {
                InstallationKey::from_secret(&secret).ok_or(AccountMeshRefusal::Unavailable)?
            }
            None => {
                let key = InstallationKey::generate();
                self.ports
                    .keys
                    .save(&key.to_secret())
                    .await
                    .map_err(|_| AccountMeshRefusal::Unavailable)?;
                key
            }
        };
        let key = Arc::new(key);
        self.with(|inner| inner.key = Some(key.clone()));
        Ok(key)
    }

    fn presence_refusal(&self) -> AccountMeshRefusal {
        match self.state().presence {
            Presence::SignedOut => AccountMeshRefusal::SignedOut,
            _ => AccountMeshRefusal::Unauthenticated,
        }
    }

    /// Captures the session only if it still belongs to the signed-in account of `epoch`.
    async fn signed(&self, epoch: Option<u64>) -> Outcome<Signed> {
        let trust = self
            .config
            .trust
            .clone()
            .ok_or(AccountMeshRefusal::VerificationUnavailable)?;
        let (current, signed_in) = self.with(|inner| {
            (
                inner.state.account_epoch,
                inner.state.signed_in_account().map(str::to_owned),
            )
        });
        let epoch = epoch.unwrap_or(current);
        let Some(signed_in) = signed_in else {
            return Err(self.presence_refusal());
        };
        let session = self
            .ports
            .sessions
            .session()
            .await?
            .ok_or(AccountMeshRefusal::Unauthenticated)?;
        let account = account_id(&session.identity.uid);
        if account != signed_in || !self.with(|inner| inner.is_current(epoch)) {
            return Err(AccountMeshRefusal::Cancelled);
        }
        let key = self.key().await?;
        Ok(Signed {
            epoch,
            session,
            account,
            trust,
            key,
        })
    }

    async fn prove<T: Serialize>(
        &self,
        signed: &Signed,
        endpoint: ProofEndpoint,
        body: &T,
        generation: u64,
    ) -> Outcome<InstallationProof> {
        let body = serde_json::to_value(body).map_err(|_| AccountMeshRefusal::Invalid)?;
        let body_sha256 = jcs_sha256_hex(&body).ok_or(AccountMeshRefusal::Invalid)?;
        let challenge = ChallengeRequest {
            endpoint,
            key_id: signed.key.key_id(),
            body,
        };
        let nonce = self
            .call(self.ports.registry.challenge(&signed.session, &challenge))
            .await?;
        let context = ProofContext {
            project: signed.trust.project().to_owned(),
            mode: signed.trust.mode(),
            uid: signed.session.identity.uid.clone(),
            installation_id: self.installation(),
            policy_epoch: signed.trust.policy_epoch().to_owned(),
            method: "POST",
            endpoint,
            body_sha256,
        };
        signed
            .key
            .prove(&context, generation, self.now(), nonce)
            .ok_or(AccountMeshRefusal::Invalid)
    }

    /// The server key generation for this account, enrolling the installation key once.
    async fn generation(&self, signed: &Signed) -> Outcome<u64> {
        let _guard = self.key_lock.lock().await;
        if self.state().last_refusal == Some(AccountMeshRefusal::ReauthRequired) {
            return Err(AccountMeshRefusal::ReauthRequired);
        }
        let key_id = signed.key.key_id();
        let known = self.with(|inner| inner.record.keys.get(&signed.account).cloned());
        if let Some(binding) = known.as_ref().filter(|binding| binding.key_id == key_id) {
            return Ok(binding.generation);
        }
        if known.is_none() && enrollment_requires_reauthentication(&signed.session, self.now()) {
            return Err(AccountMeshRefusal::ReauthRequired);
        }
        let expected_generation = known.as_ref().map_or(0, |binding| binding.generation);
        let mut generation = expected_generation
            .checked_add(1)
            .ok_or(AccountMeshRefusal::Invalid)?;
        let body = InstallationKeyBody {
            installation_id: self.installation(),
            public_key_spki_base64: signed.key.spki_base64(),
            operation: if expected_generation == 0 {
                KeyOperation::Enroll
            } else {
                KeyOperation::Recover
            },
            expected_generation,
        };
        let proof = self
            .prove(signed, ProofEndpoint::EnrollInstallation, &body, generation)
            .await?;
        let request = InstallationEnrollment { body, proof };
        let enrolled = match self
            .call(self.ports.registry.enroll_key(&signed.session, &request))
            .await
        {
            Err(AccountMeshRefusal::Conflict) if known.is_none() => {
                generation = 2;
                let body = InstallationKeyBody {
                    installation_id: self.installation(),
                    public_key_spki_base64: signed.key.spki_base64(),
                    operation: KeyOperation::Recover,
                    expected_generation: 1,
                };
                let proof = self
                    .prove(signed, ProofEndpoint::EnrollInstallation, &body, generation)
                    .await?;
                let request = InstallationEnrollment { body, proof };
                self.call(self.ports.registry.enroll_key(&signed.session, &request))
                    .await?
            }
            result => result?,
        };
        if enrolled.key_id != key_id || enrolled.generation != generation {
            return Err(AccountMeshRefusal::Invalid);
        }
        let committed = self.with(|inner| {
            if !inner.is_current(signed.epoch) && signed.epoch != 0 {
                return false;
            }
            inner.record.keys.insert(
                signed.account.clone(),
                KeyBinding {
                    key_id,
                    generation: enrolled.generation,
                },
            );
            true
        });
        if !committed {
            return Err(AccountMeshRefusal::Cancelled);
        }
        self.persist().await;
        Ok(enrolled.generation)
    }

    /// Every registry operation goes through the one proof-bound admission callable.
    pub(super) async fn admission(
        &self,
        signed: &Signed,
        operation: MeshOperation,
    ) -> Outcome<MeshResult> {
        let generation = self.generation(signed).await?;
        let request = MeshAdmissionBody {
            schema_version: MESH_SCHEMA_VERSION,
            installation_id: self.installation(),
            request_id: request_id(),
            operation,
        };
        self.with(|inner| inner.last_request = Some(request.request_id.clone()));
        let installation_proof = self
            .prove(signed, ProofEndpoint::MeshAdmission, &request, generation)
            .await?;
        let envelope = SignedEnvelope {
            request,
            installation_proof,
        };
        self.call(self.ports.registry.admission(&signed.session, &envelope))
            .await
    }

    fn own_slot(&self, account: &str) -> Option<u64> {
        self.with(|inner| inner.record.slots.get(account).copied())
    }

    async fn reconcile_slot(&self, signed: &Signed) -> Outcome<Option<u64>> {
        let observation = self.read_registry(signed).await?;
        let Some(row) = observation
            .rows
            .iter()
            .find(|row| row.installation_id == self.installation())
            .filter(|row| {
                row.active
                    && row.key_id == signed.key.key_id()
                    && row.key_generation > 0
                    && row.slot_revision > 0
            })
        else {
            return Ok(None);
        };
        let key_id = row.key_id.clone();
        let generation = row.key_generation;
        let slot_revision = row.slot_revision;
        let current = self.with(|inner| {
            if !inner.is_current(signed.epoch) && signed.epoch != 0 {
                return false;
            }
            inner
                .record
                .keys
                .insert(signed.account.clone(), KeyBinding { key_id, generation });
            inner
                .record
                .slots
                .insert(signed.account.clone(), slot_revision);
            true
        });
        if !current {
            return Err(AccountMeshRefusal::Cancelled);
        }
        self.persist().await;
        Ok(Some(slot_revision))
    }

    pub(super) async fn enroll_once(&self, epoch: u64, trigger: EnrollTrigger) -> Outcome<()> {
        let signed = self.signed(Some(epoch)).await?;
        let now = self.now();
        let current = self.with(|inner| inner.state.current_membership(now).is_some());
        if current
            && (trigger == EnrollTrigger::Network || self.state().publication_failure.is_some())
        {
            self.set_registry_presence(&signed, RegistryPresence::Present)
                .await?;
            return self.reconcile_publication(&signed).await;
        }
        let slot = self.own_slot(&signed.account);
        let operation = match slot {
            Some(expected_slot_revision) => MeshOperation::Renew {
                expected_slot_revision,
            },
            None => MeshOperation::Enroll {
                expected_slot_revision: 0,
            },
        };
        let result = match self.admission(&signed, operation).await {
            Ok(result) => result,
            Err(AccountMeshRefusal::Conflict | AccountMeshRefusal::Forbidden) => {
                match self.reconcile_slot(&signed).await? {
                    Some(slot_revision) => {
                        self.admission(
                            &signed,
                            MeshOperation::Renew {
                                expected_slot_revision: slot_revision,
                            },
                        )
                        .await?
                    }
                    None => {
                        self.admission(
                            &signed,
                            MeshOperation::Enroll {
                                expected_slot_revision: 0,
                            },
                        )
                        .await?
                    }
                }
            }
            Err(refusal) => return Err(refusal),
        };
        let MeshResult::Membership(result) = result else {
            return Err(AccountMeshRefusal::Invalid);
        };
        let installation = self.installation();
        let key_id = signed.key.key_id();
        let generation = self.generation(&signed).await?;
        if result.installation_id != installation {
            return Err(AccountMeshRefusal::Invalid);
        }
        let verified = verify_membership(
            &result.membership,
            &signed.trust,
            &MembershipExpectation {
                account_id: &signed.account,
                installation_id: Some(&installation),
                key_id: Some(&key_id),
                key_generation: Some(generation),
                slot_revision: Some(result.slot_revision),
            },
            self.now(),
        )
        .map_err(|_| AccountMeshRefusal::Invalid)?;
        let token = membership_token(&result.membership);
        let committed = self.with(|inner| {
            if !inner.is_current(epoch) {
                return false;
            }
            inner
                .record
                .slots
                .insert(signed.account.clone(), verified.slot_revision);
            let display_name = inner
                .state
                .self_installation
                .as_ref()
                .and_then(|s| s.display_name.clone());
            let metadata_revision = inner.metadata_revision.unwrap_or(0);
            inner.published_endpoint = None;
            inner.update(&self.watch, |state| {
                state.membership_epoch += 1;
                state.self_installation = Some(SelfInstallation {
                    installation_id: verified.installation_id.clone(),
                    key_id: verified.key_id.clone(),
                    key_generation: verified.key_generation,
                    slot_revision: verified.slot_revision,
                    metadata_revision,
                    display_name,
                });
                state.membership = Some(SignedMembership { token, verified });
            });
            true
        });
        if !committed {
            return Err(AccountMeshRefusal::Cancelled);
        }
        self.persist().await;
        if let Err(refusal) = self.ensure_display_name(&signed).await {
            self.with(|inner| {
                if inner.is_current(epoch) {
                    if matches!(
                        refusal,
                        AccountMeshRefusal::Conflict | AccountMeshRefusal::Forbidden
                    ) {
                        inner.metadata_revision = None;
                    }
                    inner.update(&self.watch, |state| state.registry_failure = Some(refusal));
                }
            });
        }
        self.push().await;
        self.set_registry_presence(&signed, RegistryPresence::Present)
            .await?;
        self.reconcile_publication(&signed).await
    }

    async fn ensure_display_name(&self, signed: &Signed) -> Outcome<()> {
        let Some(name) = self
            .config
            .default_display_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            return Ok(());
        };
        let registry = self.read_registry(signed).await?;
        let own = registry
            .rows
            .iter()
            .find(|row| row.installation_id == self.installation())
            .ok_or(AccountMeshRefusal::Invalid)?;
        if own.display_name.is_some() {
            return Ok(());
        }
        let MeshResult::Updated(updated) = self
            .admission(
                signed,
                MeshOperation::Update {
                    expected_metadata_revision: own.metadata_revision,
                    display_name: name.to_owned(),
                },
            )
            .await?
        else {
            return Err(AccountMeshRefusal::Invalid);
        };
        if updated.installation_id != self.installation() {
            return Err(AccountMeshRefusal::Invalid);
        }
        self.remember_metadata(
            signed.epoch,
            updated.metadata_revision,
            Some(updated.display_name),
        );
        self.read_registry(signed).await?;
        Ok(())
    }

    pub(super) fn remember_metadata(
        &self,
        epoch: u64,
        revision: u64,
        display_name: Option<String>,
    ) {
        self.with(|inner| {
            if epoch != 0 && !inner.is_current(epoch) {
                return;
            }
            inner.metadata_revision = Some(revision);
            inner.update(&self.watch, |state| {
                if let Some(current) = state.self_installation.as_mut() {
                    current.metadata_revision = revision;
                    if display_name.is_some() {
                        current.display_name = display_name;
                    }
                }
            });
        });
    }

    pub(super) async fn metadata_revision(&self, signed: &Signed) -> Outcome<u64> {
        if let Some(revision) = self.with(|inner| inner.metadata_revision) {
            return Ok(revision);
        }
        self.read_registry(signed).await?;
        self.with(|inner| inner.metadata_revision)
            .ok_or(AccountMeshRefusal::Invalid)
    }

    /// Publishes (`Some`) or withdraws (`None`) this installation's endpoint; true when published.
    async fn publish(&self, signed: &Signed, endpoint: Option<MeshEndpoint>) -> Outcome<bool> {
        let expected_metadata_revision = self.metadata_revision(signed).await?;
        let operation = match endpoint {
            Some(endpoint) => MeshOperation::Publish {
                expected_metadata_revision,
                endpoint: endpoint.wire(),
            },
            None => MeshOperation::Withdraw {
                expected_metadata_revision,
            },
        };
        let result = self.admission(signed, operation).await;
        if result == Err(AccountMeshRefusal::Conflict) {
            self.with(|inner| inner.metadata_revision = None);
        }
        let installation = self.installation();
        let (published, metadata_revision) = match result? {
            MeshResult::Published(result) if result.installation_id == installation => {
                (true, result.metadata_revision)
            }
            MeshResult::Withdrawn(result) if result.installation_id == installation => {
                (false, result.metadata_revision)
            }
            _ => return Err(AccountMeshRefusal::Invalid),
        };
        self.remember_metadata(signed.epoch, metadata_revision, None);
        Ok(published)
    }

    /// Publishes only a listener that is already reachable; publication intent is durable
    /// before the call so sign-out always knows a withdrawal may be owed.
    async fn reconcile_publication(&self, signed: &Signed) -> Outcome<()> {
        let ready = self.ports.endpoint.ready_endpoint();
        if self.with(|inner| {
            inner.published_endpoint == ready
                && inner.state.publication == Publication::Published
                && inner.state.publication_failure.is_none()
        }) {
            return Ok(());
        }
        let intended = self.with(|inner| {
            if !inner.is_current(signed.epoch) {
                return None;
            }
            let published = inner.record.published;
            if ready.is_some() {
                inner.record.published = true;
            }
            Some(published)
        });
        let Some(was_published) = intended else {
            return Err(AccountMeshRefusal::Cancelled);
        };
        if ready.is_some() && !was_published {
            self.persist().await;
        }
        let mut failure = None;
        let publication = match ready {
            Some(endpoint) => match self.publish(signed, Some(endpoint)).await {
                Ok(true) => Publication::Published,
                Err(refusal) => {
                    failure = Some(refusal);
                    self.state().publication
                }
                Ok(false) => Publication::NotReady,
            },
            None if was_published => {
                let result = self.publish(signed, None).await;
                failure = result.err();
                if result == Ok(false) {
                    self.with(|inner| inner.record.published = false);
                    self.persist().await;
                }
                Publication::NotReady
            }
            None => Publication::NotReady,
        };
        let changed = self.with(|inner| {
            if !inner.is_current(signed.epoch) {
                return false;
            }
            if failure.is_none() {
                inner.published_endpoint = ready;
            }
            inner.update(&self.watch, |state| {
                state.publication = publication;
                state.publication_failure = failure;
            });
            true
        });
        if changed {
            self.push().await;
        }
        failure.map_or(Ok(()), Err)
    }

    /// The outgoing account's still-valid session, restricted to its enrolled key.
    pub(super) async fn outgoing_signed(&self, session: &Session) -> Outcome<Signed> {
        let trust = self
            .config
            .trust
            .clone()
            .ok_or(AccountMeshRefusal::VerificationUnavailable)?;
        let signed = Signed {
            epoch: 0,
            account: account_id(&session.identity.uid),
            session: session.clone(),
            trust,
            key: self.key().await?,
        };
        let enrolled = self.with(|inner| {
            inner
                .record
                .keys
                .get(&signed.account)
                .is_some_and(|b| b.key_id == signed.key.key_id())
        });
        if !enrolled {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(signed)
    }

    pub(super) async fn withdraw_endpoint(&self, session: &Session) -> Outcome<()> {
        let signed = self.outgoing_signed(session).await?;
        match self.publish(&signed, None).await? {
            false => Ok(()),
            true => Err(AccountMeshRefusal::Unavailable),
        }
    }

    /// One bounded online read of the account registry; a stale observation for display only.
    async fn read_registry(&self, signed: &Signed) -> Outcome<RegistryObservation> {
        let mut rows = Vec::new();
        let mut cursor = None;
        let mut registry_revision = 0;
        for _ in 0..MAX_LIST_PAGES {
            let page = match self
                .admission(signed, MeshOperation::List { cursor })
                .await?
            {
                MeshResult::Page(page) => page,
                _ => return Err(AccountMeshRefusal::Invalid),
            };
            registry_revision = page.registry_revision;
            rows.extend(page.rows);
            cursor = page.cursor;
            if cursor.is_none() {
                break;
            }
        }
        let installation = self.installation();
        if let Some(own) = rows.iter().find(|row| row.installation_id == installation) {
            self.remember_metadata(
                signed.epoch,
                own.metadata_revision,
                own.display_name.clone(),
            );
        }
        let observation = RegistryObservation {
            observed_at: self.now(),
            registry_revision,
            rows,
        };
        self.with(|inner| {
            if inner.is_current(signed.epoch) || signed.epoch == 0 {
                inner.update(&self.watch, |state| {
                    state.registry = Some(observation.clone());
                    state.registry_failure = None;
                });
            }
        });
        Ok(observation)
    }

    pub async fn list(&self) -> Outcome<RegistryObservation> {
        let signed = self.signed(None).await?;
        let result = self.read_registry(&signed).await;
        self.with(|inner| {
            if !inner.is_current(signed.epoch) {
                return Err(AccountMeshRefusal::Cancelled);
            }
            match result {
                Ok(observation) => Ok(observation),
                Err(refusal) => {
                    inner.update(&self.watch, |state| state.registry_failure = Some(refusal));
                    Err(refusal)
                }
            }
        })
    }

    /// Server `update`, actor-only: renames this installation.
    pub async fn rename_self(
        &self,
        expected_metadata_revision: u64,
        display_name: String,
    ) -> Outcome<UpdatedResult> {
        let signed = self.signed(None).await?;
        let operation = MeshOperation::Update {
            expected_metadata_revision,
            display_name,
        };
        let MeshResult::Updated(updated) = self.admission(&signed, operation).await? else {
            return Err(AccountMeshRefusal::Invalid);
        };
        if updated.installation_id != self.installation() {
            return Err(AccountMeshRefusal::Invalid);
        }
        self.remember_metadata(
            signed.epoch,
            updated.metadata_revision,
            Some(updated.display_name.clone()),
        );
        self.push().await;
        Ok(updated)
    }

    /// Server `revoke` releases the slot once; transport owns removing the pairing.
    pub async fn revoke(&self, target: MeshTarget) -> Outcome<RevokedResult> {
        let signed = self.signed(None).await?;
        let MeshResult::Revoked(revoked) = self
            .admission(&signed, MeshOperation::Revoke { target })
            .await?
        else {
            return Err(AccountMeshRefusal::Invalid);
        };
        let own = revoked.target_installation_id == self.installation();
        self.with(|inner| {
            inner
                .record
                .revoked_installations
                .insert(revoked.target_installation_id.clone());
            if own {
                inner.record.slots.remove(&signed.account);
            }
            if inner.is_current(signed.epoch) {
                inner.update(&self.watch, |state| {
                    state
                        .revoked_installations
                        .insert(revoked.target_installation_id.clone());
                    if own {
                        state.membership = None;
                        state.self_installation = None;
                        state.enrollment = Enrollment::Idle;
                    }
                });
            }
        });
        self.persist().await;
        self.push().await;
        self.ports
            .transport
            .installation_revoked(&revoked.target_installation_id)
            .await;
        Ok(revoked)
    }

    /// Fresh online slot identity of an active, eligible installation; no endpoint needed.
    pub async fn resolve_link_identity(&self, installation_id: &str) -> Outcome<MeshTarget> {
        let signed = self.signed(None).await?;
        let observation = self.read_registry(&signed).await?;
        let row = observation
            .rows
            .iter()
            .find(|row| row.installation_id == installation_id)
            .ok_or(AccountMeshRefusal::Invalid)?;
        if !row.active || row.eligibility != Eligibility::Eligible {
            return Err(AccountMeshRefusal::Revoked);
        }
        if row.presence == RegistryPresence::SignedOut {
            return Err(AccountMeshRefusal::SignedOut);
        }
        let target = row.target();
        if !self.with(|inner| inner.is_current(signed.epoch)) {
            return Err(AccountMeshRefusal::Cancelled);
        }
        Ok(target)
    }

    /// Fresh online registry read; the only source of a new-link address.
    pub async fn resolve_link_target(&self, installation_id: &str) -> Outcome<LinkTarget> {
        let signed = self.signed(None).await?;
        let observation = self.read_registry(&signed).await?;
        let now = self.now();
        let row = observation
            .rows
            .iter()
            .find(|row| row.installation_id == installation_id)
            .ok_or(AccountMeshRefusal::Invalid)?;
        if !row.active || row.eligibility != Eligibility::Eligible {
            return Err(AccountMeshRefusal::Revoked);
        }
        if row.presence == RegistryPresence::SignedOut {
            return Err(AccountMeshRefusal::SignedOut);
        }
        let endpoint = row
            .endpoint
            .as_ref()
            .filter(|endpoint| endpoint.expires_at > now)
            .and_then(|endpoint| MeshEndpoint::parse(&endpoint.tailnet_ip, endpoint.api_port))
            .ok_or(AccountMeshRefusal::Unavailable)?;
        let target = row.target();
        let current = self.with(|inner| {
            let current = inner.is_current(signed.epoch);
            if current {
                inner
                    .resolved
                    .insert(target.installation_id.clone(), target.clone());
            }
            current
        });
        if !current {
            return Err(AccountMeshRefusal::Cancelled);
        }
        Ok(LinkTarget { target, endpoint })
    }

    /// Fresh online, per-side authorization for a NEW link; never served from any cache.
    /// The peer target only names the request; the server re-checks it against the registry.
    pub async fn authorize_link(&self, request: LinkAuthorization) -> Outcome<String> {
        let now = self.now();
        let epoch = self.with(|inner| {
            let state = &inner.state;
            match &state.presence {
                Presence::SignedOut => Err(AccountMeshRefusal::SignedOut),
                Presence::NeverSignedIn => Err(AccountMeshRefusal::Unauthenticated),
                Presence::SignedIn { account_id } if *account_id != request.account_id => {
                    Err(AccountMeshRefusal::Unauthenticated)
                }
                Presence::SignedIn { .. } if state.plan_limit_locked() => {
                    Err(AccountMeshRefusal::PlanLimitLocked)
                }
                Presence::SignedIn { .. } if state.current_membership(now).is_none() => {
                    Err(AccountMeshRefusal::Revoked)
                }
                Presence::SignedIn { .. } => Ok(state.account_epoch),
            }
        })?;
        if request.local_installation_id != self.installation() {
            return Err(AccountMeshRefusal::Invalid);
        }
        let resolved =
            self.with(|inner| inner.resolved.get(&request.peer_installation_id).cloned());
        // A responder authorizes on the initiator's slot identity; it never dials that peer.
        let peer = match resolved {
            Some(peer) => peer,
            None => {
                self.resolve_link_identity(&request.peer_installation_id)
                    .await?
            }
        };
        let signed = self.signed(Some(epoch)).await?;
        let expected_slot_revision = self
            .own_slot(&signed.account)
            .ok_or(AccountMeshRefusal::Revoked)?;
        let operation = MeshOperation::Link {
            expected_slot_revision,
            peer,
            transcript_sha256: sha256_hex(&request.transcript),
        };
        let MeshResult::Permit(result) = self.admission(&signed, operation).await? else {
            return Err(AccountMeshRefusal::Invalid);
        };
        if result.installation_id != self.installation()
            || result.peer_installation_id != request.peer_installation_id
        {
            return Err(AccountMeshRefusal::Invalid);
        }
        if !self.with(|inner| inner.is_current(epoch)) {
            return Err(AccountMeshRefusal::Cancelled);
        }
        Ok(membership_token(&result.permit))
    }

    /// Verifies a peer's membership against the signed-in account; never against a cache.
    pub fn verify_peer_membership(
        &self,
        token: &str,
        now: u64,
    ) -> Result<VerifiedMembership, MembershipRefusal> {
        let trust = self
            .config
            .trust
            .as_ref()
            .ok_or(MembershipRefusal::UnknownKey)?;
        let account = self
            .with(|inner| inner.state.signed_in_account().map(str::to_owned))
            .ok_or(MembershipRefusal::Subject)?;
        let grant = parse_membership_token(token)?;
        verify_membership(
            &grant,
            trust,
            &MembershipExpectation {
                account_id: &account,
                ..MembershipExpectation::default()
            },
            now,
        )
    }

    pub fn verify_peer_link_permit(
        &self,
        permit: &str,
        expected: &LinkPermitExpectation<'_>,
        now: u64,
    ) -> Result<(), MembershipRefusal> {
        let trust = self
            .config
            .trust
            .as_ref()
            .ok_or(MembershipRefusal::UnknownKey)?;
        let own = self
            .with(|inner| {
                inner.state.self_installation.as_ref().map(|s| MeshTarget {
                    installation_id: s.installation_id.clone(),
                    key_id: s.key_id.clone(),
                    key_generation: s.key_generation,
                    slot_revision: s.slot_revision,
                })
            })
            .ok_or(MembershipRefusal::Slot)?;
        let expected = LinkPermitExpectation {
            peer_target: Some(&own),
            ..*expected
        };
        let grant = parse_membership_token(permit)?;
        verify_link_permit(&grant, trust, &expected, now)
    }

    /// Possession over the handshake transcript, only while this account may use the Mesh.
    pub fn sign_possession(&self, transcript: &[u8; 32]) -> Outcome<String> {
        self.with(|inner| {
            if inner.state.signed_in_account().is_none() {
                return Err(match inner.state.presence {
                    Presence::SignedOut => AccountMeshRefusal::SignedOut,
                    _ => AccountMeshRefusal::Unauthenticated,
                });
            }
            inner
                .key
                .as_ref()
                .map(|key| key.sign_possession(transcript))
                .ok_or(AccountMeshRefusal::Unavailable)
        })
    }

    pub fn verify_possession(
        &self,
        membership: &VerifiedMembership,
        transcript: &[u8; 32],
        possession: &str,
    ) -> bool {
        super::key::verify_possession(&membership.key_id, transcript, possession)
    }
}

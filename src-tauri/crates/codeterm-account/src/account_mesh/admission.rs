use super::key::sha256_hex;
use super::lifecycle::AccountMesh;
use super::operations::{request_id, Signed};
use super::state::AccountMeshRefusal;
use super::wire::*;

type Outcome<T> = Result<T, AccountMeshRefusal>;

pub(super) struct CredentialStamp {
    epoch: u64,
    generation: u64,
    token_digest: String,
}

impl AccountMesh {
    pub fn credentials_refreshed(&self, session: &crate::Session) -> bool {
        self.with(|inner| {
            if inner.state.signed_in_account()
                != Some(super::account_id(&session.identity.uid).as_str())
            {
                return false;
            }
            let stamp = CredentialStamp {
                epoch: inner.state.account_epoch,
                generation: session.login_generation,
                token_digest: sha256_hex(session.id_token.expose().as_bytes()),
            };
            let rearm = inner.auth_parked.as_ref().is_some_and(|parked| {
                parked.epoch == stamp.epoch
                    && parked.generation == stamp.generation
                    && parked.token_digest != stamp.token_digest
            });
            inner.refreshed_credentials = Some(stamp);
            if !rearm {
                return false;
            }
            inner.auth_parked = None;
            inner.retry = Default::default();
            inner.update(&self.watch, |state| {
                state.last_refusal = None;
                state.retry_at = None;
            });
            true
        })
    }

    pub(super) async fn admission(
        &self,
        signed: &Signed,
        operation: MeshOperation,
    ) -> Outcome<MeshResult> {
        let _guard = self.admission_lock.lock().await;
        if self.with(|inner| inner.auth_parked.is_some()) {
            return Err(AccountMeshRefusal::Unauthenticated);
        }
        let mut current = Signed {
            epoch: signed.epoch,
            session: signed.session.clone(),
            account: signed.account.clone(),
            trust: signed.trust.clone(),
            key: signed.key.clone(),
        };
        if signed.epoch != 0 {
            current.session = self.signed(Some(signed.epoch)).await?.session;
        }
        let result = self.admission_once(&current, operation.clone()).await;
        if result != Err(AccountMeshRefusal::Unauthenticated) {
            return result;
        }
        let session = self
            .call_session_refresh()
            .await?
            .ok_or(AccountMeshRefusal::Unauthenticated)?;
        if session.identity.uid != signed.session.identity.uid
            || session.login_generation != signed.session.login_generation
            || (signed.epoch != 0 && !self.with(|inner| inner.is_current(signed.epoch)))
        {
            return Err(AccountMeshRefusal::Cancelled);
        }
        current.session = session;
        let result = self.admission_once(&current, operation).await;
        if result == Err(AccountMeshRefusal::Unauthenticated) {
            let superseded = self.with(|inner| {
                if !inner.is_current(signed.epoch) {
                    return false;
                }
                let stamp = CredentialStamp {
                    epoch: signed.epoch,
                    generation: current.session.login_generation,
                    token_digest: sha256_hex(current.session.id_token.expose().as_bytes()),
                };
                if inner.refreshed_credentials.as_ref().is_some_and(|latest| {
                    latest.epoch == stamp.epoch
                        && latest.generation == stamp.generation
                        && latest.token_digest != stamp.token_digest
                }) {
                    return true;
                }
                inner.auth_parked = Some(stamp);
                false
            });
            if superseded {
                return Err(AccountMeshRefusal::Unavailable);
            }
        }
        result
    }

    async fn call_session_refresh(&self) -> Outcome<Option<crate::Session>> {
        tokio::time::timeout(self.config.call_deadline, self.ports.sessions.refresh())
            .await
            .map_err(|_| AccountMeshRefusal::Offline)?
    }

    async fn admission_once(
        &self,
        signed: &Signed,
        operation: MeshOperation,
    ) -> Outcome<MeshResult> {
        if matches!(operation, MeshOperation::Recover { target_policy: Some(_), .. })
            && !self.with(|inner| inner.rejoin_window.as_ref().is_some_and(|window| window.live(signed.epoch, self.now()))) {
            return Err(AccountMeshRefusal::ReauthRequired);
        }
        let prepared = self.with(|inner| {
            let window = inner.rejoin_window.as_mut().filter(|window| window.live(signed.epoch, self.now()))?;
            if window.recovery.as_ref().is_some_and(|prepared| !prepared.replayable(self.now())) {
                window.recovery = None;
            }
            window.recovery.as_ref().filter(|prepared| prepared.envelope.request.operation == operation)
                .map(|prepared| prepared.envelope.clone())
        });
        if let Some(prepared) = prepared {
            self.with(|inner| inner.last_request = Some(prepared.request.request_id.clone()));
            return self.submit_admission(signed, &prepared).await;
        }
        let generation = self.generation(signed).await?;
        let mut request = MeshAdmissionBody {
            schema_version: MESH_SCHEMA_VERSION,
            installation_id: self.installation(),
            request_id: request_id(),
            requested_membership_seconds: matches!(
                operation,
                MeshOperation::Enroll { .. }
                    | MeshOperation::Renew { .. }
                    | MeshOperation::Recover { .. }
            )
            .then_some(super::membership::MAX_MEMBERSHIP_SECONDS),
            operation,
        };
        self.with(|inner| {
            if signed.epoch != 0 && !inner.is_current(signed.epoch) {
                return Err(AccountMeshRefusal::Cancelled);
            }
            if matches!(request.operation, MeshOperation::Renew { .. }) {
                if let Some(pending) = &inner.pending_renew {
                    if pending.operation == request.operation {
                        request = pending.clone();
                    }
                }
                inner.pending_renew = Some(request.clone());
            }
            inner.last_request = Some(request.request_id.clone());
            Ok(())
        })?;
        let issued_at = self.now();
        let installation_proof = self
            .prove(signed, ProofEndpoint::MeshAdmission, &request, generation)
            .await?;
        let envelope = SignedEnvelope {
            request,
            installation_proof,
        };
        if matches!(envelope.request.operation, MeshOperation::Recover { target_policy: Some(_), .. }) {
            self.with(|inner| {
                if let Some(window) = inner.rejoin_window.as_mut().filter(|window| window.epoch == signed.epoch) {
                    window.recovery = Some(super::rejoin::PreparedRecovery { envelope: envelope.clone(), issued_at });
                }
            });
        }
        let result = self.submit_admission(signed, &envelope).await;
        if !matches!(
            result,
            Err(AccountMeshRefusal::Offline
                | AccountMeshRefusal::Unavailable
                | AccountMeshRefusal::AuthorityUnavailable
                | AccountMeshRefusal::RateLimited { .. }
                | AccountMeshRefusal::Unauthenticated)
        ) {
            self.with(|inner| {
                if inner.is_current(signed.epoch) {
                    inner.pending_renew = None;
                }
            });
        }
        result
    }

    async fn submit_admission(&self, signed: &Signed, envelope: &SignedEnvelope<MeshAdmissionBody>) -> Outcome<MeshResult> {
        let result = self.call(self.ports.registry.admission(&signed.session, envelope)).await;
        if matches!(result, Err(AccountMeshRefusal::RateLimited { .. }))
            && matches!(envelope.request.operation, MeshOperation::Recover { target_policy: Some(_), .. }) {
            self.with(|inner| {
                if inner.is_current(signed.epoch) {
                    if let Some(window) = inner.rejoin_window.as_mut() { window.recovery = None; }
                }
            });
        }
        result
    }
}

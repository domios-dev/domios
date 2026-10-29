use super::lifecycle::AccountMesh;
use super::operations::{request_id, Signed};
use super::state::AccountMeshRefusal;
use super::wire::*;

type Outcome<T> = Result<T, AccountMeshRefusal>;

impl AccountMesh {
    pub(super) async fn admission(
        &self,
        signed: &Signed,
        operation: MeshOperation,
    ) -> Outcome<MeshResult> {
        let _guard = self.admission_lock.lock().await;
        if self.with(|inner| inner.auth_parked) {
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
        let refreshed = self.call_session_refresh().await;
        if let Ok(Some(session)) = refreshed {
            if session.identity.uid != signed.session.identity.uid
                || session.login_generation != signed.session.login_generation
                || (signed.epoch != 0 && !self.with(|inner| inner.is_current(signed.epoch)))
            {
                return Err(AccountMeshRefusal::Cancelled);
            }
            current.session = session;
            let result = self.admission_once(&current, operation).await;
            if result.is_ok() {
                return result;
            }
        }
        self.with(|inner| {
            if inner.is_current(signed.epoch) {
                inner.auth_parked = true;
            }
        });
        Err(AccountMeshRefusal::Unauthenticated)
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
}

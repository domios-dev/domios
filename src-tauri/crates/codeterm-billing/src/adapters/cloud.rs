use crate::*;
use codeterm_account::{account_mesh::wire::InstallationProof, InstallationId, Session};
use serde::de::DeserializeOwned;
use std::time::Duration;

mod refusal;
use refusal::{callable_status, BillingRefusal};

/// Proof refusal meaning only a fresh sign-in on this installation can restore proofs.
pub const INSTALLATION_SIGN_IN_REQUIRED: &str = "Installation sign-in required";

#[async_trait]
pub trait InstallationProofSource: Send + Sync {
    async fn proof(
        &self,
        session: &Session,
        installation_id: &codeterm_account::InstallationId,
    ) -> Result<InstallationProof>;

    async fn replacement_proof(
        &self,
        _session: &Session,
        _installation_id: &InstallationId,
        _generation: u64,
    ) -> Option<InstallationProof> {
        None
    }
}

#[async_trait]
impl InstallationProofSource for codeterm_account::account_mesh::AccountMesh {
    async fn proof(
        &self,
        session: &Session,
        installation_id: &codeterm_account::InstallationId,
    ) -> Result<InstallationProof> {
        self.entitlements_proof(session, installation_id)
            .await
            .map_err(|refusal| {
                log::debug!("Entitlement installation proof failed: kind={refusal:?}");
                match refusal {
                    codeterm_account::account_mesh::AccountMeshRefusal::ReauthRequired => {
                        INSTALLATION_SIGN_IN_REQUIRED.into()
                    }
                    _ => "Installation proof unavailable".into(),
                }
            })
    }

    async fn replacement_proof(
        &self,
        session: &Session,
        installation_id: &InstallationId,
        generation: u64,
    ) -> Option<InstallationProof> {
        self.replacement_entitlements_proof(session, installation_id, generation)
            .await
    }
}
#[async_trait]
trait CloudTransport: Send + Sync {
    async fn execute(&self, request: reqwest::Request) -> Result<(bool, serde_json::Value)>;
}

struct HttpCloudTransport(reqwest::Client);
#[async_trait]
impl CloudTransport for HttpCloudTransport {
    async fn execute(&self, request: reqwest::Request) -> Result<(bool, serde_json::Value)> {
        let response = self
            .0
            .execute(request)
            .await
            .map_err(|_| "Billing server unreachable")?;
        let successful = response.status().is_success();
        let body = response
            .json()
            .await
            .map_err(|_| "Invalid billing response")?;
        Ok((successful, body))
    }
}

pub struct Cloud {
    base: url::Url,
    client: reqwest::Client,
    transport: std::sync::Arc<dyn CloudTransport>,
    installation_id: codeterm_account::InstallationId,
    installation_proofs: Option<std::sync::Arc<dyn InstallationProofSource>>,
    revocations:
        Option<std::sync::Arc<dyn codeterm_account::session_revocation::SessionRevocationObserver>>,
}

enum CallFailure {
    Other(String),
    Refused {
        status: &'static str,
        reason: BillingRefusal,
        detail: &'static str,
    },
    SessionRevoked,
}

impl CallFailure {
    fn could_be_rekey(&self) -> bool {
        matches!(
            self,
            Self::Refused {
                status: "FAILED_PRECONDITION",
                reason: BillingRefusal::Unknown,
                detail: "unreported" | "generation_mismatch",
            }
        )
    }

    fn message(self) -> String {
        match self {
            Self::Other(message) => message,
            Self::Refused { reason, .. } => reason.message().into(),
            Self::SessionRevoked => codeterm_account::session_revocation::SESSION_REVOKED.into(),
        }
    }
}
impl Cloud {
    pub fn configured(
        override_url: Option<&str>,
        installation_id: codeterm_account::InstallationId,
        installation_proofs: std::sync::Arc<dyn InstallationProofSource>,
        revocations: std::sync::Arc<
            dyn codeterm_account::session_revocation::SessionRevocationObserver,
        >,
    ) -> Result<std::sync::Arc<dyn BillingProvider>> {
        let endpoint = codeterm_account::firebase_endpoints::configured_functions(override_url)?;
        match endpoint {
            Some(endpoint) => Ok(std::sync::Arc::new(
                Self::new(endpoint, installation_id)?
                    .with_installation_proofs(installation_proofs)
                    .with_revocations(revocations),
            )),
            None => Ok(std::sync::Arc::new(super::local::Unavailable)),
        }
    }

    pub fn new(base: &str, installation_id: codeterm_account::InstallationId) -> Result<Self> {
        let base = codeterm_account::firebase_endpoints::configured_functions(Some(base))?
            .ok_or("Invalid billing endpoint")?;
        let base = url::Url::parse(base).map_err(|_| "Invalid billing endpoint")?;
        if !(codeterm_account::firebase_endpoints::allows_url(&base) || dev_issuer(&base))
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || !base.path().ends_with('/')
        {
            return Err("Billing endpoint must be an HTTPS base URL".into());
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "Billing transport unavailable")?;
        Ok(Self {
            base,
            transport: std::sync::Arc::new(HttpCloudTransport(client.clone())),
            client,
            installation_id,
            installation_proofs: None,
            revocations: None,
        })
    }
    #[cfg(test)]
    pub(crate) fn fixture(base: &str, id: codeterm_account::InstallationId) -> Self {
        Self {
            base: url::Url::parse(base).unwrap(),
            client: reqwest::Client::new(),
            transport: std::sync::Arc::new(HttpCloudTransport(reqwest::Client::new())),
            installation_id: id,
            installation_proofs: None,
            revocations: None,
        }
    }
    pub fn with_revocations(
        mut self,
        observer: std::sync::Arc<
            dyn codeterm_account::session_revocation::SessionRevocationObserver,
        >,
    ) -> Self {
        self.revocations = Some(observer);
        self
    }
    pub fn with_installation_proofs(
        mut self,
        proofs: std::sync::Arc<dyn InstallationProofSource>,
    ) -> Self {
        self.installation_proofs = Some(proofs);
        self
    }
    fn request(
        &self,
        session: &Session,
        name: &str,
        data: serde_json::Value,
    ) -> Result<reqwest::RequestBuilder> {
        let url = self
            .base
            .join(name)
            .map_err(|_| "Invalid billing endpoint")?;
        #[cfg(debug_assertions)]
        let local_bearer = self.local_dev_bearer(session, &url);
        #[cfg(debug_assertions)]
        let bearer = local_bearer.as_deref().unwrap_or(session.id_token.expose());
        #[cfg(not(debug_assertions))]
        let bearer = session.id_token.expose();
        Ok(self
            .client
            .post(url)
            .bearer_auth(bearer)
            .json(&serde_json::json!({"data":data})))
    }
    async fn call<T: DeserializeOwned>(
        &self,
        session: &Session,
        name: &str,
        data: serde_json::Value,
    ) -> Result<T> {
        self.call_with_failure(session, name, data)
            .await
            .map_err(CallFailure::message)
    }

    async fn call_with_failure<T: DeserializeOwned>(
        &self,
        session: &Session,
        name: &str,
        data: serde_json::Value,
    ) -> std::result::Result<T, CallFailure> {
        let generation = data
            .pointer("/installation_proof/generation")
            .and_then(|value| value.as_u64());
        let request = self
            .request(session, name, data)
            .map_err(CallFailure::Other)?
            .build()
            .map_err(|_| CallFailure::Other("Invalid billing request".into()))?;
        let (successful, body) = self
            .transport
            .execute(request)
            .await
            .map_err(CallFailure::Other)?;
        if !successful || body.get("error").is_some() {
            static REFUSALS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let trace_id = REFUSALS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let status = callable_status(
                body.pointer("/error/status")
                    .and_then(|value| value.as_str()),
            );
            let reason = refusal_reason(&body);
            let detail = refusal::grant_detail(&body);
            log::debug!(
                "Billing callable refused: status={status} reason={} grant_detail={detail} proof_generation={generation:?} trace_id={trace_id}",
                reason.code()
            );
            if codeterm_account::session_revocation::is_session_revoked(&body) {
                if let Some(observer) = &self.revocations {
                    observer.revoked(session).await;
                }
                return Err(CallFailure::SessionRevoked);
            }
            return Err(CallFailure::Refused {
                status,
                reason,
                detail,
            });
        }
        serde_json::from_value(
            body.get("result")
                .cloned()
                .ok_or_else(|| CallFailure::Other("Invalid callable response".into()))?,
        )
        .map_err(|_| CallFailure::Other("Invalid billing response".into()))
    }
    #[cfg(debug_assertions)]
    fn local_dev_bearer(&self, session: &Session, target: &url::Url) -> Option<String> {
        if !dev_issuer(target)
            || target != &self.base.join("getEntitlements").ok()?
            || session.identity.provider != "local"
            || session.identity.uid != "local-dev-user"
        {
            return None;
        }
        let claims = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("uid", &session.identity.uid)
            .append_pair("installation_id", self.installation_id.as_str())
            .finish();
        Some(format!("domios-local-dev.{claims}"))
    }
    fn local_entitlements(&self, session: &Session) -> bool {
        dev_issuer(&self.base)
            && session.identity.provider == "local"
            && session.identity.uid == "local-dev-user"
    }
    async fn browser_url(
        &self,
        session: &Session,
        name: &str,
        data: serde_json::Value,
    ) -> Result<url::Url> {
        #[derive(serde::Deserialize)]
        struct Response {
            url: String,
        }
        let response: Response = self.call(session, name, data).await?;
        let url = url::Url::parse(&response.url).map_err(|_| "Invalid billing URL")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("Unsafe billing URL".into());
        }
        Ok(url)
    }
}

fn entitlements_request(
    installation_id: &codeterm_account::InstallationId,
    proof: Option<codeterm_account::account_mesh::wire::InstallationProof>,
) -> Result<serde_json::Value> {
    let mut data = serde_json::json!({"installation_id":installation_id});
    match proof {
        Some(proof) => {
            data["installation_proof"] =
                serde_json::to_value(proof).map_err(|_| "Invalid installation proof")?
        }
        None => {}
    }
    Ok(data)
}

fn refusal_reason(body: &serde_json::Value) -> BillingRefusal {
    BillingRefusal::classify(
        body.pointer("/error/details/reason")
            .and_then(|value| value.as_str()),
    )
}

/// DEV QA issuer on loopback; its verdicts still need the pinned TEST signature to count.
fn dev_issuer(url: &url::Url) -> bool {
    cfg!(debug_assertions) && url.scheme() == "http" && url.host_str() == Some("127.0.0.1")
}

#[async_trait]
impl BillingProvider for Cloud {
    async fn plan_operation(
        &self,
        session: &Session,
        operation: PlanOperation,
    ) -> Result<serde_json::Value> {
        let (name, data) = operation.callable();
        self.call(session, name, data).await
    }
    async fn entitlements(&self, session: &Session) -> Result<Reading> {
        let local = cfg!(debug_assertions) && self.local_entitlements(session);
        let (proof, sign_in_required) = match &self.installation_proofs {
            Some(source) if !local => match source.proof(session, &self.installation_id).await {
                Ok(proof) => (Some(proof), false),
                Err(error) => (None, error == INSTALLATION_SIGN_IN_REQUIRED),
            },
            _ => (None, false),
        };
        let generation = proof.as_ref().map(|proof| proof.generation);
        let data = entitlements_request(&self.installation_id, proof)?;
        let mut response = self
            .call_with_failure::<ApplicationEnvelope>(session, "getEntitlements", data)
            .await;
        let refused_generation =
            generation.filter(|_| response.as_ref().is_err_and(CallFailure::could_be_rekey));
        if let (Some(generation), Some(source)) = (refused_generation, &self.installation_proofs) {
            if let Some(proof) = source
                .replacement_proof(session, &self.installation_id, generation)
                .await
                .filter(|proof| proof.generation > generation)
            {
                log::debug!("Retrying signed entitlements after installation rekey: old_generation={generation} new_generation={}", proof.generation);
                let data = entitlements_request(&self.installation_id, Some(proof))?;
                response = self
                    .call_with_failure(session, "getEntitlements", data)
                    .await;
            }
        }
        match response {
            Ok(envelope) => Ok(Reading {
                installation_sign_in_required: sign_in_required,
                ..Reading::from(envelope)
            }),
            Err(
                CallFailure::SessionRevoked
                | CallFailure::Refused {
                    reason: BillingRefusal::SessionRevoked,
                    ..
                },
            ) => Err(CallFailure::SessionRevoked.message()),
            Err(_) if generation.is_some() => self
                .call::<ApplicationEnvelope>(
                    session,
                    "getEntitlements",
                    serde_json::json!({"installation_id": self.installation_id}),
                )
                .await
                .map(Reading::from),
            Err(error) => Err(error.message()),
        }
    }
    async fn checkout_url(&self, session: &Session, plan: &str) -> Result<url::Url> {
        self.browser_url(
            session,
            "createCheckoutSession",
            serde_json::json!({"plan":plan}),
        )
        .await
    }
    async fn portal_url(&self, session: &Session) -> Result<url::Url> {
        self.browser_url(session, "createPortalSession", serde_json::json!({}))
            .await
    }
}

#[cfg(test)]
mod tests;

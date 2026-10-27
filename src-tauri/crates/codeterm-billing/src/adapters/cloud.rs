use crate::*;
use codeterm_account::Session;
use serde::de::DeserializeOwned;
use std::time::Duration;

mod refusal;
use refusal::{callable_status, BillingRefusal};

#[async_trait]
pub trait InstallationProofSource: Send + Sync {
    async fn proof(
        &self,
        session: &Session,
        installation_id: &codeterm_account::InstallationId,
    ) -> Result<codeterm_account::account_mesh::wire::InstallationProof>;
}

#[async_trait]
impl InstallationProofSource for codeterm_account::account_mesh::AccountMesh {
    async fn proof(
        &self,
        session: &Session,
        installation_id: &codeterm_account::InstallationId,
    ) -> Result<codeterm_account::account_mesh::wire::InstallationProof> {
        self.entitlements_proof(session, installation_id)
            .await
            .map_err(|_| "Installation proof unavailable".into())
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
        let request = self
            .request(session, name, data)?
            .build()
            .map_err(|_| "Invalid billing request")?;
        let (successful, body) = self.transport.execute(request).await?;
        if !successful || body.get("error").is_some() {
            static REFUSALS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let trace_id = REFUSALS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            log::debug!(
                "Billing callable refused: status={} reason={} trace_id={trace_id}",
                callable_status(
                    body.pointer("/error/status")
                        .and_then(|value| value.as_str())
                ),
                refusal_reason(&body).code()
            );
        }
        if codeterm_account::session_revocation::is_session_revoked(&body) {
            if let Some(observer) = &self.revocations {
                observer.revoked(session).await;
            }
            return Err(codeterm_account::session_revocation::SESSION_REVOKED.into());
        }
        if !successful || body.get("error").is_some() {
            return Err(refusal_message(&body));
        }
        serde_json::from_value(
            body.get("result")
                .cloned()
                .ok_or("Invalid callable response")?,
        )
        .map_err(|_| "Invalid billing response".into())
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
    local: bool,
) -> Result<serde_json::Value> {
    let mut data = serde_json::json!({"installation_id":installation_id});
    match proof {
        Some(proof) => {
            data["installation_proof"] =
                serde_json::to_value(proof).map_err(|_| "Invalid installation proof")?
        }
        None if local => {}
        None => return Err("Installation proof unavailable".into()),
    }
    Ok(data)
}

fn refusal_reason(body: &serde_json::Value) -> BillingRefusal {
    BillingRefusal::classify(
        body.pointer("/error/details/reason")
            .and_then(|value| value.as_str()),
    )
}

fn refusal_message(body: &serde_json::Value) -> String {
    refusal_reason(body).message().into()
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
        let proof = if local {
            None
        } else {
            Some(
                self.installation_proofs
                    .as_ref()
                    .ok_or("Installation proof unavailable")?
                    .proof(session, &self.installation_id)
                    .await
                    .map_err(|_| "Installation proof unavailable")?,
            )
        };
        let data = entitlements_request(&self.installation_id, proof, local)?;
        self.call::<ApplicationEnvelope>(session, "getEntitlements", data)
            .await
            .map(Reading::from)
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

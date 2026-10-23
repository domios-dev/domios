use crate::*;
use codeterm_account::Session;
use serde::de::DeserializeOwned;
use std::time::Duration;
pub struct Cloud {
    base: url::Url,
    client: reqwest::Client,
    installation_id: codeterm_account::InstallationId,
}
impl Cloud {
    pub fn configured(
        override_url: Option<&str>,
        installation_id: codeterm_account::InstallationId,
    ) -> Result<std::sync::Arc<dyn BillingProvider>> {
        let endpoints = codeterm_account::firebase_endpoints::current();
        if !cfg!(debug_assertions) || endpoints.emulator {
            return Ok(std::sync::Arc::new(Self::new(
                endpoints.functions,
                installation_id,
            )?));
        }
        Self::configured_from(
            override_url,
            crate::build_config::functions_url(),
            installation_id,
        )
    }
    fn configured_from(
        override_url: Option<&str>,
        build_url: Option<&str>,
        installation_id: codeterm_account::InstallationId,
    ) -> Result<std::sync::Arc<dyn BillingProvider>> {
        let endpoint = override_url
            .filter(|value| !value.trim().is_empty())
            .or(build_url);
        match endpoint {
            Some(endpoint) => Ok(std::sync::Arc::new(Self::new(endpoint, installation_id)?)),
            None => Ok(std::sync::Arc::new(super::local::Unavailable)),
        }
    }

    pub fn new(base: &str, installation_id: codeterm_account::InstallationId) -> Result<Self> {
        let endpoints = codeterm_account::firebase_endpoints::current();
        let base = base_for(cfg!(debug_assertions), endpoints, base);
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
            client,
            installation_id,
        })
    }
    #[cfg(test)]
    pub(crate) fn fixture(base: &str, id: codeterm_account::InstallationId) -> Self {
        Self {
            base: url::Url::parse(base).unwrap(),
            client: reqwest::Client::new(),
            installation_id: id,
        }
    }
    async fn call<T: DeserializeOwned>(
        &self,
        session: &Session,
        name: &str,
        data: serde_json::Value,
    ) -> Result<T> {
        let url = self
            .base
            .join(name)
            .map_err(|_| "Invalid billing endpoint")?;
        let response = self
            .client
            .post(url)
            .bearer_auth(session.id_token.expose())
            .json(&serde_json::json!({"data":data}))
            .send()
            .await
            .map_err(|_| "Billing server unreachable")?
            .error_for_status()
            .map_err(|_| "Billing request refused")?;
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|_| "Invalid billing response")?;
        serde_json::from_value(
            body.get("result")
                .cloned()
                .ok_or("Invalid callable response")?,
        )
        .map_err(|_| "Invalid billing response".into())
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

/// DEV QA issuer on loopback; its verdicts still need the pinned TEST signature to count.
fn dev_issuer(url: &url::Url) -> bool {
    cfg!(debug_assertions) && url.scheme() == "http" && url.host_str() == Some("127.0.0.1")
}

fn base_for(
    dev_build: bool,
    endpoints: codeterm_account::firebase_endpoints::Endpoints,
    configured: &str,
) -> &str {
    if !dev_build {
        codeterm_account::firebase_endpoints::PRODUCTION.functions
    } else if endpoints.emulator {
        endpoints.functions
    } else {
        configured
    }
}
#[async_trait]
impl BillingProvider for Cloud {
    async fn entitlements(&self, session: &Session) -> Result<Reading> {
        let data = serde_json::json!({"installation_id":self.installation_id});
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

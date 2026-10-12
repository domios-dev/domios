use crate::*;
use codeterm_account::Session;
use serde::de::DeserializeOwned;
use std::time::Duration;
pub struct Cloud {
    base: url::Url,
    client: reqwest::Client,
}
impl Cloud {
    pub fn configured(override_url: Option<&str>) -> Result<std::sync::Arc<dyn BillingProvider>> {
        Self::configured_from(override_url, crate::build_config::functions_url())
    }
    fn configured_from(
        override_url: Option<&str>,
        build_url: Option<&str>,
    ) -> Result<std::sync::Arc<dyn BillingProvider>> {
        let endpoint = override_url
            .filter(|value| !value.trim().is_empty())
            .or(build_url);
        match endpoint {
            Some(endpoint) => Ok(std::sync::Arc::new(Self::new(endpoint)?)),
            None => Ok(std::sync::Arc::new(super::local::Unavailable)),
        }
    }

    pub fn new(base: &str) -> Result<Self> {
        let base = url::Url::parse(base).map_err(|_| "Invalid billing endpoint")?;
        if base.scheme() != "https"
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
        Ok(Self { base, client })
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
#[async_trait]
impl BillingProvider for Cloud {
    async fn entitlements(&self, session: &Session) -> Result<Entitlements> {
        self.call(session, "getEntitlements", serde_json::json!({}))
            .await
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
mod tests {
    use super::*;
    #[tokio::test]
    async fn build_endpoint_missing_uses_local_free_and_honest_actions() {
        let local = Cloud::configured_from(None, None).unwrap();
        let session = Session {
            identity: codeterm_account::Identity {
                uid: "user".into(),
                email: None,
                display_name: None,
                provider: "local".into(),
            },
            id_token: codeterm_account::Secret::new("test-token"),
        };
        assert_eq!(
            local.entitlements(&session).await.unwrap(),
            Entitlements::default()
        );
        assert!(local
            .portal_url(&session)
            .await
            .unwrap_err()
            .contains("unavailable"));
        assert!(Cloud::configured_from(None, Some("https://example.test/")).is_ok());
        assert!(Cloud::configured_from(Some("https://override.test/"), Some("invalid")).is_ok());
    }
}

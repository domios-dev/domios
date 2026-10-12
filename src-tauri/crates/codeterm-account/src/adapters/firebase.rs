use crate::*;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

pub enum Body {
    Json(Value),
    Form(Vec<(String, String)>),
}
#[async_trait]
pub trait Transport: Send + Sync {
    async fn post(&self, url: &str, body: Body) -> Result<Value>;
}
pub struct HttpTransport {
    client: reqwest::Client,
}
impl HttpTransport {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| Error::Transport)?;
        Ok(Self { client })
    }
}
#[async_trait]
impl Transport for HttpTransport {
    async fn post(&self, url: &str, body: Body) -> Result<Value> {
        let target = url::Url::parse(url).map_err(|_| Error::Configuration)?;
        if target.scheme() != "https"
            || !target.username().is_empty()
            || target.password().is_some()
        {
            return Err(Error::Configuration);
        }
        let request = self.client.post(target);
        let request = match body {
            Body::Json(value) => request.json(&value),
            Body::Form(value) => request.form(&value),
        };
        let response = request.send().await.map_err(|_| Error::Transport)?;
        if !response.status().is_success() {
            return Err(Error::Provider);
        }
        response.json().await.map_err(|_| Error::Provider)
    }
}
pub struct Firebase {
    config: Config,
    transport: Arc<dyn Transport>,
}
impl Firebase {
    pub fn new(config: Config, transport: Arc<dyn Transport>) -> Self {
        Self { config, transport }
    }
    fn endpoint(&self, base: &str) -> Result<String> {
        if self.config.api_key.is_empty() {
            return Err(Error::Configuration);
        }
        let mut url = url::Url::parse(base).map_err(|_| Error::Configuration)?;
        url.query_pairs_mut()
            .append_pair("key", &self.config.api_key);
        Ok(url.into())
    }
    async fn session(&self, id_token: Secret, refresh_token: Secret) -> Result<Tokens> {
        let profile = self
            .transport
            .post(
                &self.endpoint("https://identitytoolkit.googleapis.com/v1/accounts:lookup")?,
                Body::Json(json!({"idToken": id_token.expose()})),
            )
            .await?;
        let user = profile
            .get("users")
            .and_then(Value::as_array)
            .and_then(|users| users.first())
            .ok_or(Error::Provider)?;
        let uid = required(user, "localId")?;
        let optional = |key| user.get(key).and_then(Value::as_str).map(str::to_owned);
        let identity = Identity {
            uid,
            email: optional("email"),
            display_name: optional("displayName"),
            provider: self.id().into(),
        };
        Ok(Tokens {
            session: Session { identity, id_token },
            refresh_token,
        })
    }
}
fn required(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or(Error::Provider)
}
#[async_trait]
impl IdentityProvider for Firebase {
    fn id(&self) -> &str {
        "firebase"
    }
    async fn start_login(&self, flow: &LoginFlow) -> Result<LoginStart> {
        Ok(LoginStart::Browser(
            flow.signin_url(&self.config.signin_url)?,
        ))
    }
    async fn redeem(&self, request: Redemption) -> Result<Tokens> {
        let response = self.transport.post(&self.config.redeem_url, Body::Json(json!({ "code": request.code.expose(), "code_verifier": request.verifier.expose(), "state": request.state, "redirect_uri": request.redirect }))).await?;
        let custom = Secret::new(required(&response, "custom_token")?);
        let response = self
            .transport
            .post(
                &self.endpoint(
                    "https://identitytoolkit.googleapis.com/v1/accounts:signInWithCustomToken",
                )?,
                Body::Json(json!({"token": custom.expose(), "returnSecureToken": true})),
            )
            .await?;
        self.session(
            Secret::new(required(&response, "idToken")?),
            Secret::new(required(&response, "refreshToken")?),
        )
        .await
    }
    async fn refresh(&self, token: &Secret) -> Result<Tokens> {
        let response = self
            .transport
            .post(
                &self.endpoint("https://securetoken.googleapis.com/v1/token")?,
                Body::Form(vec![
                    ("grant_type".into(), "refresh_token".into()),
                    ("refresh_token".into(), token.expose().into()),
                ]),
            )
            .await?;
        self.session(
            Secret::new(required(&response, "id_token")?),
            Secret::new(required(&response, "refresh_token")?),
        )
        .await
    }
    async fn revoke(&self, _: &Secret) -> Result<()> {
        // The client API has no revocation endpoint; logout deletes the desktop credential.
        Ok(())
    }
}

pub fn defaults() -> (String, String, String) {
    (
        "https://domios-d9357.web.app/signin/".into(),
        "https://us-central1-domios-d9357.cloudfunctions.net/redeemDesktopLoginCode".into(),
        "AIzaSyCvASd9et5lzKS4bzU9Doh_VamQxUZVuts".into(),
    )
}

#[cfg(test)]
mod tests;

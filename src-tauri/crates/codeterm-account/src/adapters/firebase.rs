use crate::*;
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub enum Body {
    Json(Value),
    Form(Vec<(String, String)>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestDiagnostic {
    BackendRedeem,
    FirebaseCustomTokenExchange,
}
impl RequestDiagnostic {
    const fn operation(self) -> &'static str {
        match self {
            Self::BackendRedeem => "backend_redeem",
            Self::FirebaseCustomTokenExchange => "firebase_custom_token_exchange",
        }
    }
}
#[async_trait]
pub trait Transport: Send + Sync {
    async fn post(
        &self,
        url: &str,
        body: Body,
        diagnostic: Option<RequestDiagnostic>,
    ) -> Result<Value>;
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
    async fn post(
        &self,
        url: &str,
        body: Body,
        diagnostic: Option<RequestDiagnostic>,
    ) -> Result<Value> {
        let target = match url::Url::parse(url) {
            Ok(target) => target,
            Err(_) => {
                log_request_diagnostic(diagnostic, None, "none", false, false);
                return Err(Error::Configuration);
            }
        };
        if !crate::firebase_endpoints::allows_url(&target) {
            log_request_diagnostic(diagnostic, None, "none", false, false);
            return Err(Error::Configuration);
        }
        let request = self.client.post(target);
        let request = match body {
            Body::Json(value) => request.json(&value),
            Body::Form(value) => request.form(&value),
        };
        let request = match request.build() {
            Ok(request) => request,
            Err(_) => {
                log_request_diagnostic(diagnostic, None, "none", false, false);
                return Err(Error::Transport);
            }
        };
        let app_check_header_present = request.headers().contains_key("x-firebase-appcheck");
        let auth_header_present = request
            .headers()
            .contains_key(reqwest::header::AUTHORIZATION);
        let response = match self.client.execute(request).await {
            Ok(response) => response,
            Err(_) => {
                log_request_diagnostic(
                    diagnostic,
                    None,
                    "none",
                    app_check_header_present,
                    auth_header_present,
                );
                return Err(Error::Transport);
            }
        };
        let status = response.status();
        let status_code = status.as_u16();
        let successful = status.is_success();
        let bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(_) => {
                log_request_diagnostic(
                    diagnostic,
                    Some(status_code),
                    if successful { "none" } else { "unrecognized" },
                    app_check_header_present,
                    auth_header_present,
                );
                return Err(Error::Transport);
            }
        };
        let body = serde_json::from_slice::<Value>(&bytes).ok();
        let error_code = if successful {
            "none"
        } else {
            safe_backend_error_code(body.as_ref())
        };
        log_request_diagnostic(
            diagnostic,
            Some(status_code),
            error_code,
            app_check_header_present,
            auth_header_present,
        );
        if !successful {
            return Err(classify_http_error(status_code, body.as_ref()));
        }
        body.ok_or(Error::Provider)
    }
}

fn classify_http_error(status: u16, body: Option<&Value>) -> Error {
    let credential_rejections = [
        "INVALID_REFRESH_TOKEN",
        "TOKEN_EXPIRED",
        "USER_DISABLED",
        "USER_NOT_FOUND",
    ];
    if status == 400
        && body
            .and_then(|body| body.get("error"))
            .and_then(|error| error.get("message").or(Some(error)))
            .and_then(Value::as_str)
            .is_some_and(|code| credential_rejections.contains(&code))
    {
        Error::Credentials
    } else {
        Error::Transport
    }
}

fn log_request_diagnostic(
    diagnostic: Option<RequestDiagnostic>,
    status: Option<u16>,
    backend_error_code: &str,
    app_check_header_present: bool,
    auth_header_present: bool,
) {
    let Some(diagnostic) = diagnostic else {
        return;
    };
    let status = status.map_or_else(|| "none".to_owned(), |status| status.to_string());
    log::info!(
        target: "codeterm_account::sign_in",
        "sign_in http stage={} operation={} http_status={} backend_error_code={} app_check_header_present={} auth_header_present={}",
        SignInStage::Redemption.name(),
        diagnostic.operation(),
        status,
        backend_error_code,
        app_check_header_present,
        auth_header_present,
    );
}

fn safe_backend_error_code(body: Option<&Value>) -> &str {
    let error = body.and_then(|body| body.get("error"));
    let candidate = error
        .and_then(Value::as_str)
        .or_else(|| {
            error
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            error
                .and_then(|error| error.get("status"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            error
                .and_then(|error| error.get("code"))
                .and_then(Value::as_str)
        });
    candidate
        .filter(|code| {
            (1..=64).contains(&code.len())
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
        })
        .unwrap_or("unrecognized")
}
pub struct Firebase {
    config: Config,
    transport: Arc<dyn Transport>,
}
impl Firebase {
    pub fn new(config: Config, transport: Arc<dyn Transport>) -> Self {
        Self {
            config: crate::firebase_endpoints::account_config(config),
            transport,
        }
    }
    fn endpoint(&self, base: &str) -> Result<String> {
        if self.config.api_key.is_empty() {
            return Err(Error::Configuration);
        }
        let endpoints = crate::firebase_endpoints::current();
        let base = base
            .replace(
                crate::firebase_endpoints::PRODUCTION.identity_toolkit,
                endpoints.identity_toolkit,
            )
            .replace(
                crate::firebase_endpoints::PRODUCTION.secure_token,
                endpoints.secure_token,
            );
        let mut url = url::Url::parse(&base).map_err(|_| Error::Configuration)?;
        url.query_pairs_mut()
            .append_pair("key", &self.config.api_key);
        Ok(url.into())
    }
    async fn session(
        &self,
        id_token: Secret,
        refresh_token: Secret,
        expires_at: u64,
    ) -> Result<Tokens> {
        let profile = self
            .transport
            .post(
                &self.endpoint("https://identitytoolkit.googleapis.com/v1/accounts:lookup")?,
                Body::Json(json!({"idToken": id_token.expose()})),
                None,
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
            photo_url: photo_url(user),
        };
        Ok(Tokens {
            session: Session {
                identity,
                id_token,
                expires_at: Some(expires_at),
            },
            refresh_token,
        })
    }
}
fn token_expiry(response: &Value, key: &str) -> Result<u64> {
    let seconds = required(response, key)?
        .parse::<u64>()
        .map_err(|_| Error::Provider)?;
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Provider)?
        .as_secs()
        .checked_add(seconds)
        .ok_or(Error::Provider)
}
fn photo_url(user: &Value) -> Option<String> {
    let linked = user
        .get("providerUserInfo")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    std::iter::once(user)
        .chain(linked)
        .filter_map(|entry| entry.get("photoUrl").and_then(Value::as_str))
        .find_map(accepted_photo_url)
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
        let response = self
            .transport
            .post(
                &self.config.redeem_url,
                Body::Json(json!({ "code": request.code.expose(), "code_verifier": request.verifier.expose(), "state": request.state, "redirect_uri": request.redirect })),
                Some(RequestDiagnostic::BackendRedeem),
            )
            .await?;
        let custom = Secret::new(required(&response, "custom_token")?);
        let response = self
            .transport
            .post(
                &self.endpoint(
                    "https://identitytoolkit.googleapis.com/v1/accounts:signInWithCustomToken",
                )?,
                Body::Json(json!({"token": custom.expose(), "returnSecureToken": true})),
                Some(RequestDiagnostic::FirebaseCustomTokenExchange),
            )
            .await?;
        let expires_at = token_expiry(&response, "expiresIn")?;
        self.session(
            Secret::new(required(&response, "idToken")?),
            Secret::new(required(&response, "refreshToken")?),
            expires_at,
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
                None,
            )
            .await?;
        let expires_at = token_expiry(&response, "expires_in")?;
        self.session(
            Secret::new(required(&response, "id_token")?),
            Secret::new(required(&response, "refresh_token")?),
            expires_at,
        )
        .await
    }
    async fn revoke(&self, _: &Secret) -> Result<()> {
        // The client API has no revocation endpoint; logout deletes the desktop credential.
        Ok(())
    }
}

pub fn default_config() -> Config {
    crate::firebase_endpoints::account_config(config_from_build(crate::build_config::vendor()))
}
fn config_from_build(vendor: crate::build_config::Vendor) -> Config {
    let present = |value: Option<&'static str>| value.filter(|value| !value.trim().is_empty());
    let available = [
        vendor.api_key,
        vendor.project_id,
        vendor.app_id,
        vendor.redeem_url,
    ]
    .into_iter()
    .all(|value| present(value).is_some());
    Config {
        provider: if available { "firebase" } else { "local" }.into(),
        signin_url: present(vendor.signin_url)
            .unwrap_or(crate::build_config::DEFAULT_SIGNIN_URL)
            .into(),
        redeem_url: present(vendor.redeem_url).unwrap_or_default().into(),
        api_key: present(vendor.api_key).unwrap_or_default().into(),
        login_timeout_secs: 180,
    }
}
pub fn available(config: &Config) -> bool {
    [&config.signin_url, &config.redeem_url, &config.api_key]
        .into_iter()
        .all(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests;

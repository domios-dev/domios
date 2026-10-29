mod account;
pub mod account_mesh;
pub mod adapters;
mod build_config;
pub mod firebase_endpoints;
mod flow;
pub mod session_revocation;
mod sign_in_diagnostic;
pub use sign_in_diagnostic::{SignInFailure, SignInStage};
mod installation;
mod loopback;
pub mod protected_credentials;
pub use installation::{FileInstallationStore, InstallationId, InstallationStore};

pub use account::{Account, Status};
use async_trait::async_trait;
pub use flow::{
    Callback, LoginFlow, LoginHint, LoginMode, LoginProvider, PasteCallbackError, Redemption,
};
pub use loopback::Loopback;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Secret(String);
impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Identity {
    pub uid: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub provider: String,
    #[serde(default)]
    pub photo_url: Option<String>,
}
impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("uid", &self.uid)
            .field("email", &self.email)
            .field("display_name", &self.display_name)
            .field("provider", &self.provider)
            .field("photo_url", &self.photo_url.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}
pub const MAX_PHOTO_URL_LEN: usize = 2048;
pub fn accepted_photo_url(value: &str) -> Option<String> {
    if value.len() > MAX_PHOTO_URL_LEN {
        return None;
    }
    let url = url::Url::parse(value).ok()?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let url: String = url.into();
    (url.len() <= MAX_PHOTO_URL_LEN).then_some(url)
}
#[derive(Debug, Clone)]
pub struct Session {
    pub login_generation: u64,
    pub identity: Identity,
    pub id_token: Secret,
    pub expires_at: Option<u64>,
}

#[async_trait]
pub trait SessionAdmission: Send + Sync {
    async fn admit(&self, session: &Session) -> Result<()>;
}
pub struct Tokens {
    pub session: Session,
    pub refresh_token: Secret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    Configuration,
    InvalidCallback,
    WrongState,
    Consumed,
    Timeout,
    Cancelled,
    Pending,
    Transport,
    Provider,
    Credentials,
    Browser,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Account sign-in failed: {self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

pub enum LoginStart {
    Browser(url::Url),
    Immediate(Tokens),
}
#[async_trait]
pub trait IdentityProvider: Send + Sync {
    fn id(&self) -> &str;
    fn needs_browser_callback(&self) -> bool {
        true
    }
    async fn start_login(&self, flow: &LoginFlow) -> Result<LoginStart>;
    async fn redeem(&self, request: Redemption) -> Result<Tokens>;
    async fn refresh(&self, refresh_token: &Secret) -> Result<Tokens>;
    async fn revoke(&self, refresh_token: &Secret) -> Result<()>;
}
#[async_trait]
pub trait CredentialStore: Send + Sync {
    fn session_only(&self, _provider: &str) -> bool {
        false
    }
    async fn load(&self, provider: &str) -> Result<Option<Secret>>;
    async fn save(&self, provider: &str, token: &Secret) -> Result<()>;
    async fn delete(&self, provider: &str) -> Result<()>;
}
#[async_trait]
pub trait IdentityObserver: Send + Sync {
    async fn changed(&self, identity: Option<&Identity>);
    async fn session_refreshed(&self, _session: &Session) {}
}
#[async_trait]
pub trait Browser: Send + Sync {
    async fn open(&self, url: &url::Url) -> Result<()>;
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub provider: String,
    pub signin_url: String,
    pub redeem_url: String,
    pub api_key: String,
    pub login_timeout_secs: u64,
}
impl Default for Config {
    fn default() -> Self {
        adapters::default_config()
    }
}
#[cfg(test)]
mod build_guard;
pub mod legacy_credentials;
#[cfg(test)]
mod tests;

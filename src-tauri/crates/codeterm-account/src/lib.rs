mod account;
pub mod adapters;
mod flow;
mod loopback;

pub use account::{Account, Status};
use async_trait::async_trait;
pub use flow::{Callback, LoginFlow, Redemption};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Identity {
    pub uid: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub provider: String,
}
#[derive(Debug, Clone)]
pub struct Session {
    pub identity: Identity,
    pub id_token: Secret,
}
pub struct Tokens {
    pub session: Session,
    pub refresh_token: Secret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    async fn load(&self, provider: &str) -> Result<Option<Secret>>;
    async fn save(&self, provider: &str, token: &Secret) -> Result<()>;
    async fn delete(&self, provider: &str) -> Result<()>;
}
#[async_trait]
pub trait IdentityObserver: Send + Sync {
    async fn changed(&self, identity: Option<&Identity>);
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
mod tests;

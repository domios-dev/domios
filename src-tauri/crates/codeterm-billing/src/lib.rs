pub mod adapters;
mod build_config;
mod cache;
mod model;
use async_trait::async_trait;
pub use cache::{Billing, RefreshEvent, Snapshot};
use codeterm_account::Session;
pub use model::{entitlement_allows, Capability, Entitlements, Status, CAPABILITY_PLANS};

pub type Result<T> = std::result::Result<T, String>;
#[async_trait]
pub trait BillingProvider: Send + Sync {
    async fn entitlements(&self, session: &Session) -> Result<Entitlements>;
    async fn checkout_url(&self, session: &Session, plan: &str) -> Result<url::Url>;
    async fn portal_url(&self, session: &Session) -> Result<url::Url>;
}
#[cfg(test)]
mod tests;

pub mod adapters;
mod build_config;
mod cache;
mod model;
use async_trait::async_trait;
pub use cache::{Billing, RefreshEvent, Snapshot};
use codeterm_account::Session;
pub use model::{entitlement_allows, Capability, Entitlements, Plan, Status, CAPABILITY_PLANS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductEvent {
    CheckoutStarted { plan: Plan },
    SubscriptionChanged { plan: Plan, status: Status },
}
pub type ProductObserver = std::sync::Arc<dyn Fn(ProductEvent) + Send + Sync>;

pub type Result<T> = std::result::Result<T, String>;
#[async_trait]
pub trait BillingProvider: Send + Sync {
    async fn entitlements(&self, session: &Session) -> Result<Entitlements>;
    async fn checkout_url(&self, session: &Session, plan: &str) -> Result<url::Url>;
    async fn portal_url(&self, session: &Session) -> Result<url::Url>;
}
#[cfg(test)]
mod tests;

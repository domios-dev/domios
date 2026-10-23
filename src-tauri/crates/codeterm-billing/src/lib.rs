pub mod adapters;
mod build_config;
mod cache;
mod envelope;
mod grant_store;
pub use grant_store::{FileGrantStore, Floors, GrantStore};
mod model;
use async_trait::async_trait;
pub use cache::{Billing, DeadlineTick, RefreshEvent, Snapshot, CHECKING_DEADLINE_SECS};
use codeterm_account::Session;
pub use envelope::{
    Access, ApplicationAccess, ApplicationEnvelope, Authority, BillingInventory, Capabilities,
    Effective, Reading, Version,
};
pub use model::{
    checkout_url_with_theme, Capability, CheckoutKey, CheckoutTheme, Entitlements, Plan, Status,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductEvent {
    CheckoutStarted { plan: Plan },
    SubscriptionChanged { plan: Plan, status: Status },
}
pub type ProductObserver = std::sync::Arc<dyn Fn(ProductEvent) + Send + Sync>;

pub type Result<T> = std::result::Result<T, String>;

/// Checkout refusal for an account that already holds a live subscription; the app offers Manage subscription instead.
pub const ALREADY_SUBSCRIBED: &str = "already_subscribed";
#[async_trait]
pub trait BillingProvider: Send + Sync {
    async fn entitlements(&self, session: &Session) -> Result<Reading>;
    async fn checkout_url(&self, session: &Session, plan: &str) -> Result<url::Url>;
    async fn portal_url(&self, session: &Session) -> Result<url::Url>;
}
#[cfg(test)]
mod tests;

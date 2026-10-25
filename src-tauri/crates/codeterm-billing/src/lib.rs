pub mod adapters;
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

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanOperation {
    Catalog,
    Subscription,
    Change { price_id: String },
}

impl PlanOperation {
    pub fn callable(self) -> (&'static str, serde_json::Value) {
        match self {
            Self::Catalog => ("getAppCatalog", serde_json::json!({})),
            Self::Subscription => ("getSubscriptionPlan", serde_json::json!({})),
            Self::Change { price_id } => (
                "changeSubscriptionPlan",
                serde_json::json!({"price_id":price_id}),
            ),
        }
    }
}

/// Checkout refusal for an account that already holds a live subscription; the app offers Manage subscription instead.
pub const ALREADY_SUBSCRIBED: &str = "already_subscribed";
#[async_trait]
pub trait BillingProvider: Send + Sync {
    async fn plan_operation(
        &self,
        _session: &Session,
        _operation: PlanOperation,
    ) -> Result<serde_json::Value> {
        Err("Plan changes unavailable".into())
    }
    async fn entitlements(&self, session: &Session) -> Result<Reading>;
    async fn checkout_url(&self, session: &Session, plan: &str) -> Result<url::Url>;
    async fn portal_url(&self, session: &Session) -> Result<url::Url>;
}
#[cfg(test)]
mod tests;

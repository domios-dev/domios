use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Trialing,
    PastDue,
    Canceled,
    Revoked,
    Incomplete,
    Unpaid,
    Paused,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entitlements {
    pub plan: String,
    pub status: Status,
    pub current_period_end: u64,
    pub cancel_at_period_end: bool,
}
impl Default for Entitlements {
    fn default() -> Self {
        Self {
            plan: "free".into(),
            status: Status::Active,
            current_period_end: 0,
            cancel_at_period_end: false,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Terminal,
    PaidFeatures,
}
pub const CAPABILITY_PLANS: &[(Capability, &[&str])] = &[
    (Capability::Terminal, &["free", "pro"]),
    (Capability::PaidFeatures, &["pro"]),
];
pub fn entitlement_allows(entitlements: &Entitlements, capability: Capability) -> bool {
    let paid = matches!(entitlements.status, Status::Active | Status::Trialing);
    CAPABILITY_PLANS
        .iter()
        .find(|(key, _)| *key == capability)
        .is_some_and(|(_, plans)| {
            plans.contains(&"free") || (paid && plans.contains(&entitlements.plan.as_str()))
        })
}

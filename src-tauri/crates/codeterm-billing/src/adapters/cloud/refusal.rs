#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BillingRefusal {
    AlreadySubscribed,
    RenewalWindow,
    IntervalChangeUnavailable,
    ChangePending,
    ConfirmationPending,
    SubscriptionUnavailable,
    SameTier,
    UnknownPlan,
    CatalogSelectionUnavailable,
    CatalogSelectionMismatch,
    ReviewedCatalogRequired,
    SelectCatalogPrice,
    TrialAdmissionPending,
    TestBillingRestricted,
    SessionRevoked,
    Unknown,
}

impl BillingRefusal {
    pub(super) fn classify(reason: Option<&str>) -> Self {
        match reason {
            Some("already_subscribed") => Self::AlreadySubscribed,
            Some("renewal_window") => Self::RenewalWindow,
            Some("interval_change_unavailable") => Self::IntervalChangeUnavailable,
            Some("change_pending") => Self::ChangePending,
            Some("confirmation_pending") => Self::ConfirmationPending,
            Some("subscription_unavailable") => Self::SubscriptionUnavailable,
            Some("same_tier") => Self::SameTier,
            Some("unknown_plan") => Self::UnknownPlan,
            Some("catalog_selection_unavailable") => Self::CatalogSelectionUnavailable,
            Some("catalog_selection_mismatch") => Self::CatalogSelectionMismatch,
            Some("reviewed_catalog_required") => Self::ReviewedCatalogRequired,
            Some("select_catalog_price") => Self::SelectCatalogPrice,
            Some("trial_admission_pending") => Self::TrialAdmissionPending,
            Some("test_billing_restricted") => Self::TestBillingRestricted,
            Some("session_revoked") => Self::SessionRevoked,
            _ => Self::Unknown,
        }
    }

    pub(super) fn code(self) -> &'static str {
        match self {
            Self::AlreadySubscribed => "already_subscribed",
            Self::RenewalWindow => "renewal_window",
            Self::IntervalChangeUnavailable => "interval_change_unavailable",
            Self::ChangePending => "change_pending",
            Self::ConfirmationPending => "confirmation_pending",
            Self::SubscriptionUnavailable => "subscription_unavailable",
            Self::SameTier => "same_tier",
            Self::UnknownPlan => "unknown_plan",
            Self::CatalogSelectionUnavailable => "catalog_selection_unavailable",
            Self::CatalogSelectionMismatch => "catalog_selection_mismatch",
            Self::ReviewedCatalogRequired => "reviewed_catalog_required",
            Self::SelectCatalogPrice => "select_catalog_price",
            Self::TrialAdmissionPending => "trial_admission_pending",
            Self::TestBillingRestricted => "test_billing_restricted",
            Self::SessionRevoked => "session_revoked",
            Self::Unknown => "unknown_refusal",
        }
    }

    pub(super) fn message(self) -> &'static str {
        match self {
            Self::AlreadySubscribed => crate::ALREADY_SUBSCRIBED,
            Self::RenewalWindow => "renewal_window: Changes are unavailable immediately before renewal. Try after renewal.",
            Self::IntervalChangeUnavailable => "interval_change_unavailable: Choose a price with your current billing interval.",
            Self::ChangePending => "change_pending: Another plan change is pending. Check its status before changing again.",
            Self::ConfirmationPending => "confirmation_pending: Your change is awaiting confirmation. Do not submit it again.",
            Self::SubscriptionUnavailable => "subscription_unavailable: Plan changes are unavailable for this subscription.",
            Self::SameTier => "same_tier: You already subscribe to this tier.",
            Self::UnknownPlan => "This billing plan is unavailable. Choose a plan from the current catalog.",
            Self::CatalogSelectionUnavailable => "This catalog selection is unavailable. Refresh the catalog and choose again.",
            Self::CatalogSelectionMismatch => "This price does not match the billing catalog. Refresh the catalog and choose again.",
            Self::ReviewedCatalogRequired => "Checkout is unavailable until the billing catalog is reviewed.",
            Self::SelectCatalogPrice => "Select a price from the billing catalog to continue.",
            Self::TrialAdmissionPending => "Trial checkout is not available yet. Choose a plan without a trial or try again later.",
            Self::TestBillingRestricted => "test_billing_restricted: Test billing is limited to Domios staff",
            Self::SessionRevoked => codeterm_account::session_revocation::SESSION_REVOKED,
            Self::Unknown => "Billing request refused",
        }
    }
}

pub(super) fn callable_status(status: Option<&str>) -> &'static str {
    const STATUSES: &[&str] = &[
        "OK",
        "CANCELLED",
        "UNKNOWN",
        "INVALID_ARGUMENT",
        "DEADLINE_EXCEEDED",
        "NOT_FOUND",
        "ALREADY_EXISTS",
        "PERMISSION_DENIED",
        "RESOURCE_EXHAUSTED",
        "FAILED_PRECONDITION",
        "ABORTED",
        "OUT_OF_RANGE",
        "UNIMPLEMENTED",
        "INTERNAL",
        "UNAVAILABLE",
        "DATA_LOSS",
        "UNAUTHENTICATED",
    ];
    STATUSES
        .iter()
        .copied()
        .find(|known| Some(*known) == status)
        .unwrap_or("UNKNOWN")
}

#[cfg(test)]
mod tests;

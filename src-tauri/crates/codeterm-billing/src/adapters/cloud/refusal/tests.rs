use super::{callable_status, BillingRefusal};

#[test]
fn structured_reason_codes_select_typed_refusals() {
    for (code, expected) in [
        ("already_subscribed", BillingRefusal::AlreadySubscribed),
        ("no_billing_customer", BillingRefusal::NoBillingCustomer),
        ("renewal_window", BillingRefusal::RenewalWindow),
        (
            "interval_change_unavailable",
            BillingRefusal::IntervalChangeUnavailable,
        ),
        ("change_pending", BillingRefusal::ChangePending),
        ("confirmation_pending", BillingRefusal::ConfirmationPending),
        (
            "subscription_unavailable",
            BillingRefusal::SubscriptionUnavailable,
        ),
        ("same_tier", BillingRefusal::SameTier),
        ("unknown_plan", BillingRefusal::UnknownPlan),
        (
            "catalog_selection_unavailable",
            BillingRefusal::CatalogSelectionUnavailable,
        ),
        (
            "catalog_selection_mismatch",
            BillingRefusal::CatalogSelectionMismatch,
        ),
        (
            "reviewed_catalog_required",
            BillingRefusal::ReviewedCatalogRequired,
        ),
        ("select_catalog_price", BillingRefusal::SelectCatalogPrice),
        (
            "trial_admission_pending",
            BillingRefusal::TrialAdmissionPending,
        ),
        (
            "test_billing_restricted",
            BillingRefusal::TestBillingRestricted,
        ),
        ("session_revoked", BillingRefusal::SessionRevoked),
    ] {
        let classified = BillingRefusal::classify(Some(code));
        assert_eq!(classified, expected);
        assert_eq!(classified.code(), code);
    }
}

#[test]
fn unknown_reason_inputs_have_one_opaque_classification_and_diagnostic_code() {
    for input in [
        None,
        Some(""),
        Some("fixture-untrusted"),
        Some("Unknown billing plan"),
        Some("unknown_plan\nfixture-untrusted"),
    ] {
        let classified = BillingRefusal::classify(input);
        assert_eq!(classified, BillingRefusal::Unknown);
        assert_eq!(classified.code(), "unknown_refusal");
    }
}

#[test]
fn diagnostics_admit_only_callable_status_codes() {
    for input in [
        None,
        Some(""),
        Some("fixture-untrusted"),
        Some("INTERNAL\nfixture-untrusted"),
    ] {
        assert_eq!(callable_status(input), "UNKNOWN");
    }
    for status in [
        "INVALID_ARGUMENT",
        "FAILED_PRECONDITION",
        "UNAUTHENTICATED",
        "INTERNAL",
        "RESOURCE_EXHAUSTED",
    ] {
        assert_eq!(callable_status(Some(status)), status);
    }
}

#[test]
fn no_billing_customer_explains_non_paid_access() {
    assert_eq!(
        BillingRefusal::classify(Some("no_billing_customer")).message(),
        "There is no paid subscription to manage. Your current access is not billed through Paddle."
    );
}

use super::{reconciliation_needs_owner_notice, IntegrationRepairOutcome};
use crate::agent::provider::ProviderAvailability;
use serde_json::json;

const INSTALLED: ProviderAvailability = ProviderAvailability::Available;

#[test]
fn startup_reconciliation_surfaces_unverified_and_failed_owned_states() {
    let owned = json!({ "integration_owned": true });
    for outcome in [
        IntegrationRepairOutcome::Unreadable,
        IntegrationRepairOutcome::TrustGrantRequired,
        IntegrationRepairOutcome::RepairFailed("grant failed".into()),
    ] {
        assert!(reconciliation_needs_owner_notice(
            &owned, false, INSTALLED, &outcome
        ));
    }
    assert!(reconciliation_needs_owner_notice(
        &owned,
        false,
        INSTALLED,
        &IntegrationRepairOutcome::ConsentMissing
    ));
    assert!(reconciliation_needs_owner_notice(
        &json!({ "integration_state": "absent" }),
        true,
        INSTALLED,
        &IntegrationRepairOutcome::ConsentMissing
    ));
}

#[test]
fn an_unprobed_binary_does_not_hide_an_owned_failure() {
    assert!(reconciliation_needs_owner_notice(
        &json!({ "integration_owned": true }),
        true,
        ProviderAvailability::Unknown,
        &IntegrationRepairOutcome::Unreadable
    ));
}

#[test]
fn a_provider_whose_binary_is_not_installed_raises_no_integration_notice() {
    let leftover_owned_hooks = json!({ "integration_owned": true });
    for outcome in [
        IntegrationRepairOutcome::Unreadable,
        IntegrationRepairOutcome::ConsentMissing,
        IntegrationRepairOutcome::TrustGrantRequired,
        IntegrationRepairOutcome::RepairFailed("spawn failed".into()),
    ] {
        assert!(!reconciliation_needs_owner_notice(
            &leftover_owned_hooks,
            true,
            ProviderAvailability::Unavailable,
            &outcome
        ));
    }
}

#[test]
fn absent_unconsented_provider_stays_silent_across_restarts() {
    let absent_unconsented = json!({
        "integration_state": "absent",
        "integration_owned": false
    });
    for _restart in 0..2 {
        assert!(!reconciliation_needs_owner_notice(
            &absent_unconsented,
            false,
            INSTALLED,
            &IntegrationRepairOutcome::ConsentMissing
        ));
        assert!(!reconciliation_needs_owner_notice(
            &absent_unconsented,
            false,
            INSTALLED,
            &IntegrationRepairOutcome::Unreadable
        ));
    }
}

#[test]
fn successful_or_intentionally_skipped_reconciliation_stays_quiet() {
    for outcome in [
        IntegrationRepairOutcome::Current,
        IntegrationRepairOutcome::ForeignPluginFilePreserved,
        IntegrationRepairOutcome::Repaired(None),
        IntegrationRepairOutcome::Skipped("busy".into()),
    ] {
        assert!(!reconciliation_needs_owner_notice(
            &json!({ "integration_owned": true }),
            false,
            INSTALLED,
            &outcome
        ));
    }
}

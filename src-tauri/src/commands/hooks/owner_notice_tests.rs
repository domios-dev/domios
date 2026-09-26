use super::{reconciliation_needs_owner_notice, IntegrationRepairOutcome};
use serde_json::json;

#[test]
fn startup_reconciliation_surfaces_unverified_and_failed_owned_states() {
    let owned = json!({ "integration_owned": true });
    for outcome in [
        IntegrationRepairOutcome::Unreadable,
        IntegrationRepairOutcome::TrustGrantRequired,
        IntegrationRepairOutcome::RepairFailed("grant failed".into()),
    ] {
        assert!(reconciliation_needs_owner_notice(&owned, false, &outcome));
    }
    assert!(reconciliation_needs_owner_notice(
        &owned,
        false,
        &IntegrationRepairOutcome::ConsentMissing
    ));
    assert!(reconciliation_needs_owner_notice(
        &json!({ "integration_state": "absent" }),
        true,
        &IntegrationRepairOutcome::ConsentMissing
    ));
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
            &IntegrationRepairOutcome::ConsentMissing
        ));
        assert!(!reconciliation_needs_owner_notice(
            &absent_unconsented,
            false,
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
            &outcome
        ));
    }
}

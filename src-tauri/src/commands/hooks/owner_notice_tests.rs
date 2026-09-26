use super::{reconciliation_needs_owner_notice, IntegrationRepairOutcome};

#[test]
fn startup_reconciliation_surfaces_unverified_and_failed_states() {
    for outcome in [
        IntegrationRepairOutcome::ConsentMissing,
        IntegrationRepairOutcome::Unreadable,
        IntegrationRepairOutcome::TrustGrantRequired,
        IntegrationRepairOutcome::RepairFailed("grant failed".into()),
    ] {
        assert!(reconciliation_needs_owner_notice(&outcome));
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
        assert!(!reconciliation_needs_owner_notice(&outcome));
    }
}

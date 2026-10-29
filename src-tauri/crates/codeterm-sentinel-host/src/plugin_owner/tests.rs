use super::*;

#[test]
fn plugin_owner_preview_above_run_budget_is_independent_of_primary_hold_reason() {
    for (amount, remaining, reason, expected) in [
        (5, 3, WaitingReason::OverStepAllowance, true),
        (5, 8, WaitingReason::OverStepAllowance, false),
        (5, 3, WaitingReason::OverRunCeiling, true),
        (5, 5, WaitingReason::OwnerConfirmation, false),
    ] {
        let quote = Estimate {
            amount,
            unit: "usd_micro".into(),
            valid_until_ms: 10000,
        };
        let waiting = Waiting {
            reason,
            estimate: Some(amount),
            remaining,
            unit: "usd_micro".into(),
        };
        assert_eq!(above_run_budget(&quote, &waiting), expected);
    }
}

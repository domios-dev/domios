use super::support::{admin, free, paid, with, LocalSigner, T};
use crate::payload::{Authority, TransitionReason};
use crate::verdict::{compiled_free, TimeState};
use serde_json::json;

#[test]
fn online_windows_are_half_open() {
    let signer = LocalSigner::new();
    for payload in [paid(), admin(), free()] {
        let grant = signer.accept(&payload, T);
        let p = grant.payload();
        assert_eq!(
            (p.refresh_after, p.expires_at, p.grace_until),
            (T + 1_800, T + 3_600, T + 3_600)
        );
        let cases = [
            (T - 1, TimeState::NotYetValid),
            (T, TimeState::Fresh),
            (p.refresh_after - 1, TimeState::Fresh),
            (p.refresh_after, TimeState::Stale),
            (p.expires_at - 1, TimeState::Stale),
            (p.expires_at, TimeState::Expired),
            (u64::MAX, TimeState::Expired),
        ];
        for (now, state) in cases {
            assert_eq!(grant.verdict(now).state, state, "now={now}");
        }
    }
}

#[test]
fn earlier_period_end_ends_paid_access_without_a_tail() {
    let signer = LocalSigner::new();
    let early = signer.accept(&with(paid(), json!({"period_end": T + 900})), T);
    assert_eq!(early.verdict(T + 899).state, TimeState::Fresh);
    assert_eq!(early.verdict(T + 900).state, TimeState::Expired);
    let mid = signer.accept(&with(paid(), json!({"period_end": T + 2_700})), T);
    assert_eq!(mid.verdict(T + 2_699).state, TimeState::Stale);
    assert_eq!(mid.verdict(T + 2_700).state, TimeState::Expired);
    assert_eq!(mid.verdict(T + 2_700).effective(), compiled_free());
}

#[test]
fn effective_access_falls_back_to_compiled_free_outside_retention() {
    let signer = LocalSigner::new();
    let grant = signer.accept(
        &with(paid(), json!({"transition_reason": "period_ended"})),
        T,
    );
    let retained = grant.verdict(grant.payload().expires_at - 1);
    assert_eq!(retained.effective().authority, Authority::Billing);
    assert!(retained.effective().perks.priority_support);
    for now in [T - 1, grant.payload().expires_at] {
        let verdict = grant.verdict(now);
        assert_eq!(verdict.effective(), compiled_free());
        assert_eq!(
            verdict.transition_reason,
            Some(TransitionReason::PeriodEnded)
        );
        assert_eq!(verdict.revision, 10);
    }
}

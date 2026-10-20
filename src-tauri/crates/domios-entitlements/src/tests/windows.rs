use super::support::{admin, paid, with, LocalSigner, PERIOD_END, T};
use crate::activation::{access_source, AccessSource, Phase};
use crate::payload::{Authority, TransitionReason};
use crate::verdict::{compiled_free, TimeState};
use serde_json::json;

#[test]
fn five_windows_are_half_open() {
    let signer = LocalSigner::new();
    let grant = signer.accept(&paid(), T);
    let p = grant.payload();
    let cases = [
        (T - 1, TimeState::NotYetValid),
        (T, TimeState::Fresh),
        (p.refresh_after - 1, TimeState::Fresh),
        (p.refresh_after, TimeState::Stale),
        (p.expires_at - 1, TimeState::Stale),
        (p.expires_at, TimeState::Grace),
        (p.grace_until - 1, TimeState::Grace),
        (p.grace_until, TimeState::PastGrace),
        (u64::MAX, TimeState::PastGrace),
    ];
    for (now, state) in cases {
        assert_eq!(grant.verdict(now).state, state, "now={now}");
    }
    assert_eq!(
        (p.expires_at, p.grace_until),
        (PERIOD_END, PERIOD_END + 604_800)
    );
}

#[test]
fn admin_windows_follow_admin_equations() {
    let signer = LocalSigner::new();
    let grant = signer.accept(&admin(), T);
    assert_eq!(grant.verdict(T + 86_399).state, TimeState::Fresh);
    assert_eq!(grant.verdict(T + 86_400).state, TimeState::Grace);
    assert_eq!(grant.verdict(T + 259_200).state, TimeState::PastGrace);
}

#[test]
fn effective_access_falls_back_to_compiled_free_outside_retention() {
    let signer = LocalSigner::new();
    let grant = signer.accept(
        &with(paid(), json!({"transition_reason": "period_ended"})),
        T,
    );
    let retained = grant.verdict(grant.payload().grace_until - 1);
    assert_eq!(retained.effective().authority, Authority::Billing);
    assert!(retained.effective().perks.priority_support);
    for now in [T - 1, grant.payload().grace_until] {
        let verdict = grant.verdict(now);
        assert_eq!(verdict.effective(), compiled_free());
        assert_eq!(
            verdict.transition_reason,
            Some(TransitionReason::PeriodEnded)
        );
        assert_eq!(verdict.revision, 10);
    }
}

#[test]
fn phases_choose_the_access_source() {
    let signer = LocalSigner::new();
    let verdict = signer.accept(&paid(), T).verdict(T);
    assert_eq!(
        access_source(Phase::A, Some(&verdict)),
        AccessSource::Envelope
    );
    assert_eq!(access_source(Phase::A, None), AccessSource::Envelope);
    assert_eq!(
        access_source(Phase::B, Some(&verdict)),
        AccessSource::Verdict
    );
    assert_eq!(access_source(Phase::B, None), AccessSource::CompiledFree);
}

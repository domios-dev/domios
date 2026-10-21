//! Subscription lifecycle scenarios: one signed grant per state, read at an injected instant,
//! then admitted at and past every free limit with enforcement on.
use super::support::{admin, free, paid, with, LocalSigner, T};
use crate::gate::{admit, LimitGate, LimitKey, LimitRefusal};
use crate::payload::Authority;
use crate::verdict::{compiled_free, EffectiveAccess, TimeState};
use serde_json::json;

const FREE_MAXIMA: [(LimitKey, u64); 3] = [
    (LimitKey::Workspaces, 2),
    (LimitKey::Teams, 6),
    (LimitKey::AgentTabs, 35),
];

fn assert_unlimited(access: &EffectiveAccess) {
    for (limit, max) in FREE_MAXIMA {
        for count in [max, max + 1, 10_000] {
            assert_eq!(
                admit(limit, count, access, true),
                Ok(()),
                "{limit:?} at {count}"
            );
        }
    }
}

fn assert_free_limits(access: &EffectiveAccess) {
    for (limit, max) in FREE_MAXIMA {
        assert_eq!(admit(limit, max - 1, access, true), Ok(()), "{limit:?}");
        for count in [max, max + 3] {
            assert_eq!(
                admit(limit, count, access, true),
                Err(LimitRefusal { limit, count, max })
            );
        }
    }
}

#[test]
fn active_paid_subscription_is_unlimited() {
    let grant = LocalSigner::new().accept(&paid(), T);
    let verdict = grant.verdict(T + 60);
    assert_eq!(verdict.state, TimeState::Fresh);
    assert_eq!(verdict.effective().authority, Authority::Billing);
    assert_unlimited(&verdict.effective());
}

#[test]
fn paid_subscription_awaiting_renewal_stays_unlimited() {
    let grant = LocalSigner::new().accept(&paid(), T);
    for now in [
        grant.payload().refresh_after,
        grant.payload().expires_at - 1,
    ] {
        let verdict = grant.verdict(now);
        assert_eq!(verdict.state, TimeState::Stale);
        assert_unlimited(&verdict.effective());
    }
}

#[test]
fn expired_subscription_returns_to_the_free_limits() {
    let grant = LocalSigner::new().accept(&paid(), T);
    let verdict = grant.verdict(grant.payload().expires_at);
    assert_eq!(verdict.state, TimeState::Expired);
    assert_eq!(verdict.effective(), compiled_free());
    assert_free_limits(&verdict.effective());
}

#[test]
fn cancelled_and_ended_subscription_returns_to_the_free_limits() {
    let signer = LocalSigner::new();
    let cancelled = signer.accept(&with(paid(), json!({"transition_reason": "cancelled"})), T);
    assert_free_limits(
        &cancelled
            .verdict(cancelled.payload().expires_at)
            .effective(),
    );
    let reissued = signer.accept(&free(), T);
    assert_eq!(reissued.verdict(T).effective().authority, Authority::Free);
    assert_free_limits(&reissued.verdict(T).effective());
}

#[test]
fn admin_is_unlimited_without_a_billing_period() {
    let grant = LocalSigner::new().accept(&admin(), T);
    assert_eq!(grant.payload().period_end, None);
    let access = grant.verdict(T + 60).effective();
    assert_eq!(access.authority, Authority::Admin);
    assert_unlimited(&access);
}

#[test]
fn free_installation_meets_the_limits_and_dormant_admits_everything() {
    assert_free_limits(&compiled_free());
    let dormant = LimitGate::dormant();
    for (limit, max) in FREE_MAXIMA {
        assert_eq!(dormant.admit(limit, (max + 10) as usize), Ok(()));
    }
}

use super::*;
use codeterm_types::config::provider_accounts::ProviderAccountConfig;

fn fixture() -> (Switches, ProviderAccountsConfig, Binding) {
    let policy = ProviderAccountsConfig {
        profiles: BTreeMap::from([("account-b".into(), ProviderAccountConfig::default())]),
        order: vec!["default".into(), "account-b".into()],
        ..Default::default()
    };
    let mut switches = Switches::default();
    switches.set_authentication("fixture", "default", Authentication::Ready);
    switches.set_authentication("fixture", "account-b", Authentication::Ready);
    let binding = Binding {
        provider: "fixture".into(),
        account: "default".into(),
        tab: "tab-one".into(),
        nonce: "nonce-a".into(),
        epoch: 3,
        session: "exact-own-session".into(),
        authentication_generation: switches.generation("fixture", "default").unwrap(),
    };
    switches.bind(binding.clone());
    (switches, policy, binding)
}

fn claim(switches: &mut Switches, policy: &ProviderAccountsConfig, binding: &Binding) -> Claim {
    let Decision::Switch(claim) = switches.exhausted(binding, "turn-one", None, policy, true)
    else {
        panic!("expected one switch claim");
    };
    claim
}

fn resumed(claim: &Claim) -> Binding {
    Binding {
        account: claim.target.clone(),
        nonce: "nonce-b".into(),
        epoch: claim.source.epoch + 1,
        authentication_generation: claim.generation,
        ..claim.source.clone()
    }
}

#[test]
fn duplicate_exhaustion_claims_once_and_preserves_second_tab() {
    let (mut switches, policy, binding) = fixture();
    let second = Binding {
        tab: "tab-two".into(),
        nonce: "nonce-two".into(),
        ..binding.clone()
    };
    switches.bind(second.clone());
    let first = claim(&mut switches, &policy, &binding);
    switches.bind(binding.clone());
    assert!(switches.current(&first));
    assert_eq!(
        switches.exhausted(&binding, "turn-one", None, &policy, true),
        Decision::AlreadyHandled
    );
    let other = claim(&mut switches, &policy, &second);
    assert_ne!(first.serial, other.serial);
    assert!(switches.finish(&first, resumed(&first)));
    assert!(switches.current(&other));
    assert!(!switches.finish(&first, resumed(&first)));
    assert_eq!(
        switches.exhausted(&binding, "turn-two", None, &policy, true),
        Decision::Stale
    );
}

#[test]
fn receipt_gate_and_toggle_do_not_create_claims() {
    let (mut switches, mut policy, binding) = fixture();
    assert_eq!(
        switches.exhausted(&binding, "unverified", None, &policy, false),
        Decision::ContinuationUnverified {
            next: Some("account-b".into())
        }
    );
    assert!(switches.claims.is_empty());
    policy.automatic_switch = false;
    assert_eq!(
        switches.exhausted(&binding, "disabled", None, &policy, true),
        Decision::AutomaticDisabled {
            next: Some("account-b".into())
        }
    );
    assert!(switches.claims.is_empty());
}

#[test]
fn removal_waits_for_claim_and_skips_new_selection() {
    let (mut switches, policy, binding) = fixture();
    let pending = claim(&mut switches, &policy, &binding);
    switches.mark_removing("fixture", "account-b");
    assert!(!switches.forget("fixture", "account-b"));
    assert!(switches.current(&pending));
    let second = Binding {
        tab: "tab-two".into(),
        ..binding.clone()
    };
    switches.bind(second.clone());
    assert_eq!(
        switches.exhausted(&second, "turn-two", None, &policy, true),
        Decision::AllExhausted
    );
    assert!(switches.finish(&pending, resumed(&pending)));
    assert!(!switches.forget("fixture", "account-b"));
    switches.close_tab(&binding.tab);
    assert!(switches.forget("fixture", "account-b"));
}

#[test]
fn logout_revokes_pending_claim_even_after_sign_in() {
    let (mut switches, policy, binding) = fixture();
    let pending = claim(&mut switches, &policy, &binding);
    switches.set_authentication("fixture", "account-b", Authentication::SignedOut);
    assert!(!switches.current(&pending));
    assert!(switches.owns(&pending));
    switches.set_authentication("fixture", "account-b", Authentication::Ready);
    assert!(!switches.finish(&pending, resumed(&pending)));
    switches.cancel(&pending);
    assert!(!switches.owns(&pending));
    assert_eq!(
        switches.exhausted(&binding, "turn-one", None, &policy, true),
        Decision::AlreadyHandled
    );
    switches.set_authentication("fixture", "default", Authentication::SignedOut);
    assert_eq!(
        switches.exhausted(&binding, "late-turn", None, &policy, true),
        Decision::Stale
    );
}

#[test]
fn completion_requires_same_session_and_new_binding() {
    let (mut switches, policy, binding) = fixture();
    let pending = claim(&mut switches, &policy, &binding);
    let correct = resumed(&pending);
    let variants = [
        Binding {
            session: "foreign-session".into(),
            ..correct.clone()
        },
        Binding {
            tab: "foreign-tab".into(),
            ..correct.clone()
        },
        Binding {
            nonce: binding.nonce.clone(),
            ..correct.clone()
        },
        Binding {
            epoch: binding.epoch,
            ..correct.clone()
        },
        Binding {
            account: binding.account.clone(),
            ..correct.clone()
        },
        Binding {
            authentication_generation: 999,
            ..correct.clone()
        },
    ];
    for stale in variants {
        assert!(!switches.finish(&pending, stale));
        assert!(switches.current(&pending));
    }
    assert!(switches.finish(&pending, correct));
}

#[test]
fn all_exhausted_has_no_claim_or_retry() {
    let (mut switches, policy, binding) = fixture();
    switches.set_authentication(
        "fixture",
        "account-b",
        Authentication::Exhausted {
            resets_at: Some(500),
        },
    );
    assert_eq!(
        switches.exhausted(&binding, "turn-one", Some(400), &policy, true),
        Decision::AllExhausted
    );
    assert_eq!(
        switches.exhausted(&binding, "turn-one", Some(400), &policy, true),
        Decision::AlreadyHandled
    );
    assert!(switches.claims.is_empty());
    assert_eq!(
        switches.exhausted(&binding, "unverified-turn", None, &policy, false),
        Decision::AllExhausted
    );
}

#[test]
fn source_logout_revokes_an_already_claimed_switch() {
    let (mut switches, policy, binding) = fixture();
    let pending = claim(&mut switches, &policy, &binding);
    switches.set_authentication("fixture", "default", Authentication::SignedOut);
    assert!(!switches.current(&pending));
    assert!(!switches.finish(&pending, resumed(&pending)));
}

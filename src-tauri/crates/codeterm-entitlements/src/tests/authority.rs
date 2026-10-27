use super::support::{binding, paid, with, LocalSigner, EPOCH, KEY_ID, PROJECT, T};
use crate::activation::{Activation, CompiledKey, ACTIVATION, PRODUCTION, QA_PROJECT, TEST};
use crate::authority::{enforcement, enforcing, pinned, EntitlementAuthority, PinnedAuthority};
use crate::payload::{Authority, Enforcement, Mode, COMPILED_FREE_LIMITS};
use crate::policy::is_rfc8032_test_key;
use crate::verdict::compiled_free;
use crate::verify::{DetachedGrant, Refusal, MAX_CLOCK_SKEW_SECS};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashSet;

const ON: Enforcement = Enforcement {
    enabled: true,
    rollback: false,
};

fn leak(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn authority_for(signer: &LocalSigner) -> PinnedAuthority {
    let keys: &'static [CompiledKey] = Box::leak(Box::new([CompiledKey {
        key_id: KEY_ID,
        spki_der_base64: leak(signer.spki()),
    }]));
    PinnedAuthority::new(Activation {
        project: PROJECT,
        mode: Mode::Test,
        policy_version: 1,
        policy_epoch: EPOCH,
        enforcement: ON,
        keys,
    })
}

fn enforced(payload: Value) -> Value {
    with(
        payload,
        json!({"enforcement": {"enabled": true, "rollback": false}}),
    )
}

fn access_of(
    authority: &dyn EntitlementAuthority,
    grant: Option<&DetachedGrant>,
    now: u64,
) -> Authority {
    authority.access(grant, &binding(), now).authority
}

#[test]
fn this_build_enforces() {
    assert!(enforcing());
    assert_eq!(enforcement(), ON);
    assert_eq!(pinned().enforcement(), ON);
    assert_eq!(pinned().activation(), ACTIVATION);
    #[cfg(debug_assertions)]
    assert_eq!(ACTIVATION, TEST);
    #[cfg(not(debug_assertions))]
    assert_eq!(ACTIVATION, PRODUCTION);
    PRODUCTION.policy().expect("production policy validates");
    assert_eq!(TEST.policy().is_ok(), !QA_PROJECT.is_empty());
    for activation in [PRODUCTION, TEST] {
        assert!(activation
            .keys
            .iter()
            .all(|key| !is_rfc8032_test_key(key.spki_der_base64)));
    }
    assert_eq!(PRODUCTION.mode, Mode::Live);
    assert_eq!(TEST.mode, Mode::Test);
}

#[test]
fn release_and_debug_keys_disjoint() {
    let ids = |a: Activation| a.keys.iter().map(|k| k.key_id).collect::<HashSet<_>>();
    let spkis = |a: Activation| {
        a.keys
            .iter()
            .map(|k| k.spki_der_base64)
            .collect::<HashSet<_>>()
    };
    assert!(ids(PRODUCTION).is_disjoint(&ids(TEST)));
    assert!(spkis(PRODUCTION).is_disjoint(&spkis(TEST)));
    assert_ne!(PRODUCTION.project, TEST.project);
    assert_ne!(PRODUCTION.policy_epoch, TEST.policy_epoch);
}

#[test]
fn valid_verdict_is_paid() {
    let signer = LocalSigner::new();
    let grant = signer.sign(&enforced(paid()));
    assert_eq!(
        access_of(&authority_for(&signer), Some(&grant), T),
        Authority::Billing
    );
}

#[test]
fn activation_accepts_policy_one_and_two_without_relaxing_other_pins() {
    let signer = LocalSigner::new();
    let mut activation = authority_for(&signer).activation();
    activation.policy_version = PRODUCTION.policy_version;
    let authority = PinnedAuthority::new(activation);
    for version in [1, 2] {
        let mut payload = enforced(paid());
        payload["policy_version"] = json!(version);
        if version == 2 {
            payload["limits"]["teams"] = json!({"max":null,"scope":"installation"});
            payload["limits"]["agent.tabs"] = json!({"max":null,"scope":"installation"});
        }
        let grant = signer.sign(&payload);
        assert_eq!(access_of(&authority, Some(&grant), T), Authority::Billing);
        for patch in [
            json!({"policy_epoch":"wrong"}),
            json!({"aud":{"project":PROJECT,"env":"live"}}),
            json!({"policy_version":3}),
        ] {
            assert_eq!(
                access_of(
                    &authority,
                    Some(&signer.sign(&with(payload.clone(), patch))),
                    T
                ),
                Authority::Free
            );
        }
        let mut wrong_key = grant;
        wrong_key.key_id = "unknown".into();
        assert_eq!(access_of(&authority, Some(&wrong_key), T), Authority::Free);
    }
    for version in [0, 3, u64::MAX] {
        assert!(!activation.policy().unwrap().accepts_version(version));
    }
}

#[test]
fn flipped_signature_byte_is_free() {
    let signer = LocalSigner::new();
    let mut grant = signer.sign(&enforced(paid()));
    let mut signature = URL_SAFE_NO_PAD.decode(&grant.signature).unwrap();
    signature[0] ^= 1;
    grant.signature = URL_SAFE_NO_PAD.encode(signature);
    let authority = authority_for(&signer);
    assert_eq!(
        authority
            .verify(&grant, &binding(), None, T)
            .unwrap_err()
            .refusal,
        Refusal::Signature
    );
    assert_eq!(
        authority.access(Some(&grant), &binding(), T),
        compiled_free()
    );
}

#[test]
fn unknown_key_id_is_free() {
    let signer = LocalSigner::new();
    let mut grant = signer.sign(&enforced(paid()));
    grant.key_id = "rotated-away".into();
    let authority = authority_for(&signer);
    assert_eq!(
        authority
            .verify(&grant, &binding(), None, T)
            .unwrap_err()
            .refusal,
        Refusal::UnknownKey
    );
    assert_eq!(
        authority.access(Some(&grant), &binding(), T),
        compiled_free()
    );
}

#[test]
fn expired_verdict_is_free() {
    let signer = LocalSigner::new();
    let grant = signer.sign(&enforced(paid()));
    let expires_at = T + 86_400;
    assert_eq!(
        authority_for(&signer).access(Some(&grant), &binding(), expires_at + 1),
        compiled_free()
    );
}

#[test]
fn issued_beyond_clock_skew_is_free() {
    let signer = LocalSigner::new();
    let grant = signer.sign(&enforced(paid()));
    let authority = authority_for(&signer);
    let now = T - MAX_CLOCK_SKEW_SECS - 1;
    assert_eq!(
        authority
            .verify(&grant, &binding(), None, now)
            .unwrap_err()
            .refusal,
        Refusal::IssuedInFuture
    );
    assert_eq!(
        authority.access(Some(&grant), &binding(), now),
        compiled_free()
    );
}

#[test]
fn issued_at_exact_clock_skew_boundary_is_paid() {
    let signer = LocalSigner::new();
    let grant = signer.sign(&enforced(paid()));
    let authority = authority_for(&signer);
    let now = T - MAX_CLOCK_SKEW_SECS;
    let accepted = authority.verify(&grant, &binding(), None, now).unwrap();
    assert_eq!(accepted.verdict.state, crate::verdict::TimeState::Fresh);
    assert_eq!(accepted.verdict.effective().authority, Authority::Billing);
    assert_eq!(access_of(&authority, Some(&grant), now), Authority::Billing);
}

#[test]
fn verdict_for_other_profile_is_free() {
    let signer = LocalSigner::new();
    let grant = signer.sign(&enforced(paid()));
    let other = PinnedAuthority::new(if cfg!(debug_assertions) {
        PRODUCTION
    } else {
        TEST
    });
    assert_eq!(access_of(&other, Some(&grant), T), Authority::Free);
    assert_eq!(
        access_of(pinned().as_ref(), Some(&grant), T),
        Authority::Free
    );
}

#[test]
fn wrong_project_or_epoch_is_free() {
    let signer = LocalSigner::new();
    let authority = authority_for(&signer);
    for fields in [
        json!({"aud": {"project": "elsewhere", "env": "test"}}),
        json!({"policy_epoch": "other-epoch"}),
    ] {
        let grant = signer.sign(&with(enforced(paid()), fields));
        assert_eq!(
            authority.access(Some(&grant), &binding(), T),
            compiled_free()
        );
    }
}

#[test]
fn missing_verdict_while_enforcing_is_compiled_free() {
    let access = pinned().access(None, &binding(), T);
    assert!(pinned().enforcement().enabled);
    assert_eq!(access.authority, Authority::Free);
    assert_eq!(access.limits, COMPILED_FREE_LIMITS);
}

#[test]
fn unset_qa_project_is_plain_free() {
    let signer = LocalSigner::new();
    let grant = signer.sign(&enforced(paid()));
    let authority = authority_for(&signer);
    assert_eq!(access_of(&authority, Some(&grant), T), Authority::Billing);
    let unset = PinnedAuthority::new(Activation {
        project: "",
        ..authority.activation()
    });
    assert_eq!(access_of(&unset, Some(&grant), T), Authority::Free);
}

#[cfg(not(debug_assertions))]
#[test]
fn release_pins_production_whatever_the_build_environment() {
    assert_eq!(ACTIVATION.project, "domios-d9357");
    assert_eq!(QA_PROJECT, "");
}

#[test]
fn invalid_activation_refuses_everything() {
    let signer = LocalSigner::new();
    let grant = signer.sign(&enforced(paid()));
    let broken = PinnedAuthority::new(Activation { keys: &[], ..TEST });
    assert_eq!(access_of(&broken, Some(&grant), T), Authority::Free);
}

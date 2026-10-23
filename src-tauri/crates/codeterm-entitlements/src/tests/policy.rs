use super::support::{spec, LocalSigner};
use crate::activation::{PRODUCTION, TEST};
use crate::payload::Mode;
use crate::policy::{is_rfc8032_test_key, Policy, PolicyError, TrustedKey};

const FIXTURE_SPKI: &str = "MCowBQYDK2VwAyEA11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=";

fn key(id: &str, spki: &str) -> TrustedKey {
    TrustedKey {
        key_id: id.into(),
        spki_der_base64: spki.into(),
    }
}

#[test]
fn production_activation_never_trusts_the_rfc8032_key() {
    assert!(is_rfc8032_test_key(FIXTURE_SPKI));
    for activation in [PRODUCTION, TEST] {
        assert!(activation
            .keys
            .iter()
            .all(|k| !is_rfc8032_test_key(k.spki_der_base64)));
        assert_ne!(activation.project, "demo-domios");
    }
    PRODUCTION.policy().expect("production policy validates");
}

#[test]
fn rfc8032_key_is_confined_to_fixture_projects_in_test_mode() {
    let mut fixture = spec(vec![key("k", FIXTURE_SPKI)]);
    assert_eq!(
        Policy::new(fixture.clone()).unwrap_err(),
        PolicyError::TestKeyOutsideFixture
    );
    fixture.project = "demo-domios".into();
    Policy::new(fixture.clone()).unwrap();
    fixture.project = "demo-domios-admin".into();
    Policy::new(fixture.clone()).unwrap();
    fixture.mode = Mode::Live;
    assert_eq!(
        Policy::new(fixture).unwrap_err(),
        PolicyError::TestKeyOutsideFixture
    );
}

#[test]
fn key_allowlist_shape() {
    let a = LocalSigner::new().spki();
    let b = LocalSigner::new().spki();
    assert_eq!(
        Policy::new(spec(vec![])).unwrap_err(),
        PolicyError::KeyCount
    );
    let three = vec![key("a", &a), key("b", &b), key("c", &a)];
    assert_eq!(Policy::new(spec(three)).unwrap_err(), PolicyError::KeyCount);
    assert_eq!(
        Policy::new(spec(vec![key("a", &a), key("a", &b)])).unwrap_err(),
        PolicyError::DuplicateKeyId
    );
    assert_eq!(
        Policy::new(spec(vec![key("", &a)])).unwrap_err(),
        PolicyError::KeyId
    );
    Policy::new(spec(vec![key("a", &a), key("b", &b)])).unwrap();
}

#[test]
fn spki_must_be_canonical_fixed_prefix_ed25519() {
    let good = LocalSigner::new().spki();
    let unpadded = good.trim_end_matches('=').to_string();
    let url_alphabet = FIXTURE_SPKI.replace('/', "_");
    let wrong_prefix = format!("MCowBQYDK2VxAyEA{}", &FIXTURE_SPKI[16..]);
    let too_long = "A".repeat(260);
    for spki in [
        unpadded.as_str(),
        url_alphabet.as_str(),
        wrong_prefix.as_str(),
        too_long.as_str(),
        "MCowBQ==",
    ] {
        assert_eq!(
            Policy::new(spec(vec![key("k", spki)])).unwrap_err(),
            PolicyError::Spki,
            "{spki}"
        );
    }
}

#[test]
fn policy_scalars_are_validated() {
    let spki = LocalSigner::new().spki();
    let mut bad = spec(vec![key("k", &spki)]);
    bad.project = "a/b".into();
    assert_eq!(Policy::new(bad).unwrap_err(), PolicyError::Project);
    let mut bad = spec(vec![key("k", &spki)]);
    bad.policy_epoch = "x".repeat(129);
    assert_eq!(Policy::new(bad).unwrap_err(), PolicyError::PolicyEpoch);
    let mut bad = spec(vec![key("k", &spki)]);
    bad.policy_version = 0;
    assert_eq!(Policy::new(bad).unwrap_err(), PolicyError::PolicyVersion);
}

use super::*;
use codeterm_entitlements::{verify, Binding, DetachedGrant};

fn dev_grant_accepted(activation: Option<Activation>) -> bool {
    let packet: serde_json::Value = serde_json::from_str(include_str!(
        "../../../codeterm-entitlements/vectors/grant-vectors.json"
    ))
    .unwrap();
    let vector = packet["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == "valid-paid")
        .unwrap();
    let grant: DetachedGrant = serde_json::from_value(vector["response"]["grant"].clone()).unwrap();
    let binding = Binding {
        uid: "fixture-user",
        installation_id: "fixture-installation",
    };
    activation.is_some_and(|activation| {
        verify(
            &grant,
            &activation.policy().unwrap(),
            &binding,
            None,
            1_700_000_000,
        )
        .is_ok()
    })
}

#[test]
fn release_activation_refuses_dev_signed_grant() {
    for opt_in in [false, true] {
        assert!(!dev_grant_accepted(select(false, opt_in)));
    }
    if let Some(activation) = ACTIVATION {
        assert!(activation
            .keys
            .iter()
            .all(|key| key.spki_der_base64 != DEV_ACTIVATION.keys[0].spki_der_base64));
    }
}

#[test]
fn dev_activation_accepts_dev_signed_grant() {
    assert!(dev_grant_accepted(select(true, true)));
    assert!(!dev_grant_accepted(select(true, false)));
}

#[cfg(not(debug_assertions))]
#[test]
fn compiled_release_uses_only_pinned_activation() {
    assert_eq!(current(), ACTIVATION);
    assert!(!dev_grant_accepted(current()));
}

use super::*;
use serde_json::Value;

pub(crate) fn vector() -> Value {
    serde_json::from_str(include_str!("../../test/golden-v2.json")).unwrap()
}

pub(crate) fn keys(encryption: &str, signing: &str) -> PrivateKeys {
    fn seed(text: &str) -> [u8; 32] {
        (0..32)
            .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    }
    PrivateKeys {
        encryption: seed(encryption),
        signing: seed(signing),
    }
}

pub(crate) fn recipient() -> PrivateKeys {
    let v = vector();
    keys(
        v["recipientSeed"].as_str().unwrap(),
        v["seed"].as_str().unwrap(),
    )
}

#[test]
fn golden_v2_matches_every_crypto_boundary() {
    let v = vector();
    let envelope: Envelope = serde_json::from_value(v["envelope"].clone()).unwrap();
    let sender = keys(
        v["senderSeed"].as_str().unwrap(),
        v["seed"].as_str().unwrap(),
    );
    let recipient = recipient();
    assert_eq!(sender.encryption_spki(), v["senderSpki"]);
    assert_eq!(recipient.encryption_spki(), v["recipientSpki"]);
    assert_eq!(sender.signing_spki(), v["spki"]);
    assert_eq!(sender.encryption_key_id(), envelope.enc_key_ids.sender);
    assert_eq!(
        recipient.encryption_key_id(),
        envelope.enc_key_ids.recipient
    );
    let shared = sender
        .shared_key(
            v["recipientSpki"].as_str().unwrap(),
            &envelope.friendship_id,
            &envelope.enc_key_ids,
        )
        .unwrap();
    let expected = (0..32)
        .map(|i| {
            u8::from_str_radix(&v["sharedKey"].as_str().unwrap()[i * 2..i * 2 + 2], 16).unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(shared.as_slice(), expected);
    let mut encrypted = envelope.clone();
    sender
        .encrypt_with_nonce(
            &mut encrypted,
            &recipient.encryption_spki(),
            v["body"].as_str().unwrap(),
            std::array::from_fn(|i| i as u8),
        )
        .unwrap();
    assert_eq!(encrypted, envelope);
    assert_eq!(
        sha256_hex(&envelope.canonical_bytes().unwrap()),
        v["sha256"]
    );
    let signed = sender.sign(envelope.clone()).unwrap();
    assert_eq!(signed.signature, v["signature"]);
    verify(&signed.envelope, &signed.signature, &sender.signing_spki()).unwrap();
    assert_eq!(
        recipient
            .decrypt(&envelope, &sender.encryption_spki())
            .unwrap()
            .as_str(),
        v["body"]
    );
}

#[test]
fn golden_v2_rejects_tampering_and_wrong_keys() {
    let v = vector();
    let envelope: Envelope = serde_json::from_value(v["envelope"].clone()).unwrap();
    let signature = v["signature"].as_str().unwrap();
    let spki = v["spki"].as_str().unwrap();
    for field in ["friendship_id", "message_id", "scope_sha256"] {
        let mut value = v["envelope"].clone();
        let mut bytes = value[field].as_str().unwrap().as_bytes().to_vec();
        bytes[0] = if bytes[0] == b'0' { b'1' } else { b'0' };
        value[field] = String::from_utf8(bytes).unwrap().into();
        let changed: Envelope = serde_json::from_value(value).unwrap();
        assert_eq!(verify(&changed, signature, spki), Err(Error::BadSignature));
    }
    assert_eq!(
        verify(&envelope, &format!("{signature}="), spki),
        Err(Error::BadSignature)
    );
    let mut changed = envelope.clone();
    changed.ciphertext.replace_range(..1, "A");
    assert!(recipient()
        .decrypt(&changed, v["senderSpki"].as_str().unwrap())
        .is_err());
    assert!(recipient()
        .decrypt(&envelope, v["recipientSpki"].as_str().unwrap())
        .is_err());
}

#[test]
fn golden_v2_schema_is_exact_and_bounded() {
    let v = vector();
    for field in v["envelope"].as_object().unwrap().keys() {
        let mut missing = v["envelope"].clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<Envelope>(missing).is_err(),
            "{field}"
        );
    }
    let mut extra = v["envelope"].clone();
    extra["body"] = "plaintext is forbidden".into();
    assert!(serde_json::from_value::<Envelope>(extra).is_err());
    let mut envelope: Envelope = serde_json::from_value(v["envelope"].clone()).unwrap();
    envelope.issued_at = crate::wire::MAX_SAFE_INTEGER + 1;
    assert_eq!(envelope.validate(), Err(Error::InvalidEnvelope));
}

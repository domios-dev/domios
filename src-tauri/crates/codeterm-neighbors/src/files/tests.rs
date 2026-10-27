use super::*;
use rand::{rngs::StdRng, SeedableRng};

fn binding() -> Binding<'static> {
    Binding {
        friendship_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        message_id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        sender_uid: "sender",
        recipient_uid: "recipient",
    }
}

fn sealed(bytes: &[u8]) -> (Manifest, Vec<u8>) {
    seal(&binding(), "cccccccccccccccccccccccccccccccc", "photo.png", "image/png", bytes,
        &mut StdRng::seed_from_u64(77)).unwrap()
}

#[test]
fn file_crypto_round_trip_and_tamper() {
    let (manifest, mut object) = sealed(b"opaque binary\0\xff");
    assert_eq!(&*manifest.open(&binding(), &object).unwrap(), b"opaque binary\0\xff");
    object[20] ^= 1;
    assert!(matches!(manifest.open(&binding(), &object), Err(Error::Decryption)));
    let mut substituted = manifest.clone();
    substituted.ciphertext_sha256 = sha256_hex(&object);
    assert!(matches!(substituted.open(&binding(), &object), Err(Error::Decryption)));
}

#[test]
fn file_crypto_binds_signed_identity_and_message() {
    let (manifest, object) = sealed(b"secret");
    let mut wrong = binding();
    wrong.message_id = "dddddddddddddddddddddddddddddddd";
    assert!(matches!(manifest.open(&wrong, &object), Err(Error::Decryption)));
    wrong = binding();
    wrong.recipient_uid = "other";
    assert!(matches!(manifest.open(&wrong, &object), Err(Error::Decryption)));
}

#[test]
fn file_crypto_size_bound_includes_framing() {
    let (manifest, object) = sealed(&vec![0; MAX_FILE_BYTES - FILE_OVERHEAD]);
    assert_eq!(object.len(), 2_097_152);
    assert_eq!(manifest.open(&binding(), &object).unwrap().len(), 2_097_120);
    assert!(matches!(seal(&binding(), "cccccccccccccccccccccccccccccccc", "x", "", &vec![0; MAX_FILE_BYTES - FILE_OVERHEAD + 1], &mut StdRng::seed_from_u64(1)), Err(Error::MessageTooLarge { limit: 2_097_120 })));
    let (empty, object) = sealed(b"");
    assert_eq!(object.len(), 32);
    assert!(empty.open(&binding(), &object).unwrap().is_empty());
}

#[test]
fn attachment_body_projects_handles_without_manifest_or_key() {
    let (manifest, _) = sealed(b"secret");
    let key = manifest.key_b64u.clone();
    let object = manifest.object_id.clone();
    let body = AttachmentBody::new("hello".into(), vec![manifest]);
    let wire = body.encode(&binding()).unwrap();
    assert!(wire.contains(&key));
    let parsed = AttachmentBody::parse(&wire, &binding()).unwrap().unwrap();
    let text = parsed.agent_text();
    assert_eq!(text, "hello\n[Neighbor attachment: photo.png (6 bytes), handle cccccccccccccccccccccccccccccccc]");
    for private in [key.as_str(), object.as_str(), "key_b64u", "ciphertext_sha256", "plaintext_sha256"] {
        assert!(!text.contains(private));
    }
    assert!(AttachmentBody::parse("ordinary text", &binding()).unwrap().is_none());
    assert!(AttachmentBody::parse(r#"{"attachments":[{"key_b64u":"private"}]}"#, &binding()).is_err());
}

#[test]
fn attachment_body_rejects_duplicates_and_escaped_size_overflow() {
    let (manifest, _) = sealed(b"secret");
    assert!(AttachmentBody::new("".into(), vec![manifest.clone(), manifest.clone()]).encode(&binding()).is_err());
    assert!(matches!(AttachmentBody::new("\"".repeat(20_000), vec![manifest]).encode(&binding()), Err(Error::MessageTooLarge { .. })));
}

#[test]
fn file_names_are_safe_on_windows_and_cannot_traverse() {
    for (input, expected) in [
        ("NUL", "_NUL"), ("con.txt", "_con.txt"), ("COM1.zip", "_COM1.zip"),
        ("lpt9", "_lpt9"), ("LPT¹.txt", "_LPT¹.txt"), ("AUX .txt", "_AUX .txt"),
        ("CONOUT$", "_CONOUT$"), ("..", "attachment"), (" . ", "attachment"),
        ("../../evil.exe", "_.._evil.exe"), ("a:b\\c\n.txt", "a_b_c_.txt"),
        ("normal.pdf", "normal.pdf"), ("readme. ", "readme"),
    ] {
        assert_eq!(safe_name(input), expected, "{input}");
    }
    let name = safe_name(&"🦀".repeat(100));
    assert!(name.len() <= 240);
    assert!(!name.contains(['/', '\\', ':']));
}

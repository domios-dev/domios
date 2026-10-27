use super::*;

fn project(candidates: Vec<Credentials>) -> Vec<(u16, String)> {
    candidates.into_iter().map(|c| (c.port, c.proof)).collect()
}

#[test]
fn macsys_without_app_store_container_uses_system_proof_contents() {
    assert_eq!(
        project(proof_candidates(&[], Some("54285"), Some("a1b2c3\n"))),
        vec![(54285, "a1b2c3".into())]
    );
}

#[test]
fn app_store_proof_remains_first_with_system_extension_fallback() {
    let files = vec!["unrelated".into(), "sameuserproof-12345-abcdef".into()];
    assert_eq!(
        project(proof_candidates(&files, Some("54285"), Some("123abc"))),
        vec![(12345, "abcdef".into()), (54285, "123abc".into())]
    );
    assert_eq!(
        project(proof_candidates(&files, None, None)),
        vec![(12345, "abcdef".into())]
    );
}

#[test]
fn macsys_requires_both_valid_port_and_file_contents() {
    for (port, proof) in [
        (None, None),
        (Some("54285"), None),
        (None, Some("abc123")),
        (Some("0"), Some("abc123")),
        (Some("65536"), Some("abc123")),
        (Some("../54285"), Some("abc123")),
        (Some("54285"), Some("")),
        (Some("54285"), Some("abc\r\nInjected")),
    ] {
        assert!(proof_candidates(&[], port, proof).is_empty());
    }
}

#[test]
fn malformed_app_store_proof_does_not_mask_system_extension() {
    let files = vec![
        "sameuserproof-54285".into(),
        "sameuserproof-0-abcdef".into(),
        "sameuserproof-65536-abcdef".into(),
        "sameuserproof-12345-not-hex".into(),
    ];
    assert_eq!(
        project(proof_candidates(&files, Some("54285"), Some("abc123"))),
        vec![(54285, "abc123".into())]
    );
}

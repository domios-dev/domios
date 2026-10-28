use super::*;

fn fixture() -> VerifiedManifest {
    verify_feed(
        include_bytes!("fixtures/valid.json"),
        include_str!("fixtures/test-public-key.txt").trim(),
        "linux-x86_64",
    )
    .unwrap()
}

#[test]
fn headless_signed_fixture_verifies_with_test_key_only() {
    let offer = fixture();
    offer
        .verify_archive(include_bytes!("fixtures/archive.tar.gz").as_slice())
        .unwrap();
    assert_eq!(offer.target_version("8.0.0").unwrap(), Some("9.0.0"));
    assert!(verify_feed(
        include_bytes!("fixtures/valid.json"),
        &release_public_key().unwrap(),
        "linux-x86_64"
    )
    .is_err());
    assert!(verify_feed(
        include_bytes!("fixtures/valid.json"),
        include_str!("fixtures/test-public-key.txt").trim(),
        "linux-aarch64"
    )
    .is_err());
}

#[test]
fn headless_tampered_and_unsigned_manifests_are_refused() {
    for bytes in [
        include_bytes!("fixtures/tampered.json").as_slice(),
        include_bytes!("fixtures/unsigned.json").as_slice(),
    ] {
        assert_eq!(
            verify_feed(
                bytes,
                include_str!("fixtures/test-public-key.txt").trim(),
                "linux-x86_64"
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidSignature
        );
    }
}

#[test]
fn headless_archive_tampering_truncation_and_appending_are_refused() {
    let offer = fixture();
    let original = include_bytes!("fixtures/archive.tar.gz");
    let mut tampered = original.to_vec();
    tampered[20] ^= 1;
    let mut appended = original.to_vec();
    appended.push(0);
    for bytes in [
        tampered.as_slice(),
        &original[..original.len() - 1],
        appended.as_slice(),
    ] {
        assert_eq!(
            offer.verify_archive(bytes).unwrap_err().code,
            ErrorCode::ArchiveMismatch
        );
    }
    let mut wrong_digest = offer.clone();
    wrong_digest.0.sha256 = "0".repeat(64);
    assert_eq!(
        wrong_digest
            .verify_archive(original.as_slice())
            .unwrap_err()
            .code,
        ErrorCode::ArchiveMismatch
    );
}

#[test]
fn headless_downgrade_requires_signed_authorization() {
    let mut offer = fixture();
    assert_eq!(
        offer.target_version("10.0.0").unwrap_err().code,
        ErrorCode::DowngradeRefused
    );
    assert_eq!(offer.target_version("9.0.0").unwrap(), None);
    offer.0.allow_downgrade = true;
    assert_eq!(offer.target_version("10.0.0").unwrap(), Some("9.0.0"));
}

#[test]
fn headless_unsigned_downgrade_authorization_is_not_trusted() {
    let mut feed: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/valid.json")).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&STANDARD.decode(feed["manifest"].as_str().unwrap()).unwrap())
            .unwrap();
    manifest["allow_downgrade"] = true.into();
    feed["manifest"] = STANDARD
        .encode(serde_json::to_vec(&manifest).unwrap())
        .into();
    assert_eq!(
        verify_feed(
            &serde_json::to_vec(&feed).unwrap(),
            include_str!("fixtures/test-public-key.txt").trim(),
            "linux-x86_64"
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidSignature
    );
    assert_eq!(
        serde_json::to_value(ErrorCode::SignedHeadlessFeedMissing).unwrap(),
        "signed_headless_feed_missing"
    );
}

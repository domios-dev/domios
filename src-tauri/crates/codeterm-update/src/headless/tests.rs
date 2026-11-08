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

mod transport_fallback {
    use super::super::download::{download_with, fetch_feed};
    use super::*;
    use crate::release_transport::test_support::{transport, ScriptedHttp, Step};

    const FEED: &str = "https://github.com/domios-dev/domios/releases/download/v9.0.0/domios-headless-linux-x86_64.json";
    const ARCHIVE: &str = "https://github.com/domios-dev/domios/releases/download/v9.0.0/domios-headless-v9.0.0-linux-x86_64.tar.gz";
    const RELEASE: &str = "https://api.github.com/repos/domios-dev/domios/releases/tags/v9.0.0";
    const ASSET: &str = "https://api.github.com/repos/domios-dev/domios/releases/assets/21";

    fn metadata(name: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "tag_name": "v9.0.0",
            "assets": [{"id": 21, "name": name}],
        }))
        .unwrap()
    }

    fn public_key() -> &'static str {
        include_str!("fixtures/test-public-key.txt").trim()
    }

    async fn feed(
        via: &crate::release_transport::Transport<'_>,
    ) -> Result<VerifiedManifest, Error> {
        let tag = semver::Version::new(9, 0, 0);
        fetch_feed(via, "domios-dev/domios", &tag, "linux-x86_64", public_key()).await
    }

    fn feed_steps(step: Step) -> Vec<(&'static str, Step)> {
        vec![
            (FEED, Step::Status(504)),
            (
                RELEASE,
                Step::Bytes(metadata("domios-headless-linux-x86_64.json")),
            ),
            (ASSET, step),
        ]
    }

    #[tokio::test]
    async fn a_feed_served_by_the_api_after_a_504_is_verified_as_usual() {
        let budget = tempfile::tempdir().unwrap();
        let http = ScriptedHttp::new(feed_steps(Step::Bytes(
            include_bytes!("fixtures/valid.json").to_vec(),
        )));
        let offer = feed(&transport(&http, &budget)).await.unwrap();
        assert_eq!(offer.manifest().version, "9.0.0");
    }

    #[tokio::test]
    async fn a_tampered_feed_from_the_api_is_still_refused() {
        let budget = tempfile::tempdir().unwrap();
        let http = ScriptedHttp::new(feed_steps(Step::Bytes(
            include_bytes!("fixtures/tampered.json").to_vec(),
        )));
        let error = feed(&transport(&http, &budget)).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidSignature);
    }

    #[tokio::test]
    async fn a_missing_feed_stays_missing_without_an_api_request() {
        let budget = tempfile::tempdir().unwrap();
        let http = ScriptedHttp::new(vec![(FEED, Step::Status(404))]);
        let error = feed(&transport(&http, &budget)).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::SignedHeadlessFeedMissing);
        assert_eq!(http.urls().len(), 1);
    }

    fn archive_bytes() -> Vec<u8> {
        include_bytes!("fixtures/archive.tar.gz").to_vec()
    }

    #[tokio::test]
    async fn an_archive_that_breaks_midway_restarts_through_the_api_and_matches_its_hash() {
        let budget = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("archive.tar.gz");
        let bytes = archive_bytes();
        let http = ScriptedHttp::new(vec![
            (
                ARCHIVE,
                Step::BreaksAfter(bytes[..8].to_vec(), "connection reset"),
            ),
            (
                RELEASE,
                Step::Bytes(metadata("domios-headless-v9.0.0-linux-x86_64.tar.gz")),
            ),
            (ASSET, Step::Bytes(bytes.clone())),
        ]);
        download_with(
            &transport(&http, &budget),
            ARCHIVE,
            bytes.len() as u64,
            &path,
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        fixture()
            .verify_archive(std::fs::File::open(&path).unwrap())
            .unwrap();
    }

    #[tokio::test]
    async fn altered_archive_bytes_from_the_api_fail_the_signed_hash() {
        let budget = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("archive.tar.gz");
        let mut bytes = archive_bytes();
        bytes[0] ^= 1;
        let http = ScriptedHttp::new(vec![
            (ARCHIVE, Step::Status(504)),
            (
                RELEASE,
                Step::Bytes(metadata("domios-headless-v9.0.0-linux-x86_64.tar.gz")),
            ),
            (ASSET, Step::Bytes(bytes.clone())),
        ]);
        download_with(
            &transport(&http, &budget),
            ARCHIVE,
            bytes.len() as u64,
            &path,
        )
        .await
        .unwrap();
        assert_eq!(
            fixture()
                .verify_archive(std::fs::File::open(&path).unwrap())
                .unwrap_err()
                .code,
            ErrorCode::ArchiveMismatch
        );
    }

    #[tokio::test]
    async fn a_signed_archive_url_off_github_is_never_rewritten() {
        let budget = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("archive.tar.gz");
        let url = "https://example.invalid/domios-headless-v9.0.0-linux-x86_64.tar.gz";
        let http = ScriptedHttp::new(vec![(url, Step::Status(504))]);
        let error = download_with(&transport(&http, &budget), url, 8, &path)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::DownloadFailed);
        assert_eq!(http.urls(), vec![url.to_string()]);
    }
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

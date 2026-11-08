use std::time::Duration;

use super::test_support::{transport, ScriptedHttp, Step, NOW_MS};
use super::*;

const TIMEOUT: Duration = Duration::from_secs(5);
const URL: &str =
    "https://github.com/domios-dev/domios/releases/download/v1.19.0/domios-latest.json";
const LATEST: &str =
    "https://github.com/domios-dev/domios/releases/latest/download/domios-latest.json";
const TAG_RELEASE: &str = "https://api.github.com/repos/domios-dev/domios/releases/tags/v1.19.0";
const LATEST_RELEASE: &str = "https://api.github.com/repos/domios-dev/domios/releases/latest";
const ASSET: &str = "https://api.github.com/repos/domios-dev/domios/releases/assets/712434969";

fn release_json(tag: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "tag_name": tag,
        "assets": [
            {"id": 1, "name": "other.json"},
            {"id": 712434969_u64, "name": "domios-latest.json"},
        ],
    }))
    .unwrap()
}

#[tokio::test]
async fn a_direct_504_is_served_by_the_tagged_release_asset_with_octet_stream() {
    let root = tempfile::tempdir().unwrap();
    let http = ScriptedHttp::new(vec![
        (URL, Step::Status(504)),
        (TAG_RELEASE, Step::Bytes(release_json("v1.19.0"))),
        (ASSET, Step::Bytes(b"payload".to_vec())),
    ]);
    let fetched = transport(&http, &root)
        .fetch_bytes(Get::direct(URL, TIMEOUT), 1024)
        .await
        .unwrap();
    assert_eq!(fetched.body, b"payload");
    assert_eq!(fetched.served, ServedBy::GithubApi);
    assert_eq!(fetched.etag, None);
    let seen = http.seen();
    assert_eq!(seen[0].route, Route::Direct);
    assert_eq!(seen[1].route, Route::Api);
    assert_eq!(seen[2].route, Route::Api);
    assert_eq!(seen[2].accept.as_deref(), Some("application/octet-stream"));
}

#[tokio::test]
async fn the_latest_endpoint_resolves_through_the_latest_release() {
    let root = tempfile::tempdir().unwrap();
    let http = ScriptedHttp::new(vec![
        (LATEST, Step::Status(504)),
        (LATEST_RELEASE, Step::Bytes(release_json("v9.9.9"))),
        (ASSET, Step::Bytes(b"payload".to_vec())),
    ]);
    let fetched = transport(&http, &root)
        .fetch_bytes(Get::direct(LATEST, TIMEOUT), 1024)
        .await
        .unwrap();
    assert_eq!(fetched.served, ServedBy::GithubApi);
}

#[tokio::test]
async fn timeouts_and_connection_failures_reach_the_api() {
    let root = tempfile::tempdir().unwrap();
    let http = ScriptedHttp::new(vec![
        (URL, Step::Fails("operation timed out")),
        (TAG_RELEASE, Step::Bytes(release_json("v1.19.0"))),
        (ASSET, Step::Bytes(b"payload".to_vec())),
    ]);
    let fetched = transport(&http, &root)
        .fetch_bytes(Get::direct(URL, TIMEOUT), 1024)
        .await
        .unwrap();
    assert_eq!(fetched.served, ServedBy::GithubApi);
}

#[tokio::test]
async fn a_transfer_that_breaks_midway_restarts_cleanly_through_the_api() {
    let root = tempfile::tempdir().unwrap();
    let http = ScriptedHttp::new(vec![
        (
            URL,
            Step::BreaksAfter(b"partial".to_vec(), "connection reset"),
        ),
        (TAG_RELEASE, Step::Bytes(release_json("v1.19.0"))),
        (ASSET, Step::Bytes(b"complete".to_vec())),
    ]);
    let fetched = transport(&http, &root)
        .fetch_bytes(Get::direct(URL, TIMEOUT), 1024)
        .await
        .unwrap();
    assert_eq!(fetched.body, b"complete");
}

#[tokio::test]
async fn a_healthy_or_refused_direct_reply_spends_no_api_request() {
    let root = tempfile::tempdir().unwrap();
    for (step, status) in [
        (Step::Bytes(b"payload".to_vec()), 200),
        (Step::Status(404), 404),
        (Step::Status(403), 403),
    ] {
        let http = ScriptedHttp::new(vec![(URL, step)]);
        let fetched = transport(&http, &root)
            .fetch_bytes(Get::direct(URL, TIMEOUT), 1024)
            .await
            .unwrap();
        assert_eq!(fetched.status, status);
        assert_eq!(fetched.served, ServedBy::Direct);
        assert_eq!(http.urls(), vec![URL.to_string()]);
    }
}

#[tokio::test]
async fn an_oversized_direct_file_is_refused_without_a_fallback() {
    let root = tempfile::tempdir().unwrap();
    let http = ScriptedHttp::new(vec![(URL, Step::Bytes(vec![0; 16]))]);
    assert_eq!(
        transport(&http, &root)
            .fetch_bytes(Get::direct(URL, TIMEOUT), 8)
            .await
            .unwrap_err(),
        FetchError::TooLarge
    );
}

#[tokio::test]
async fn a_url_that_is_not_github_is_never_rewritten() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = "https://example.com/domios-dev/domios/releases/download/v1.19.0/a.json";
    let http = ScriptedHttp::new(vec![(elsewhere, Step::Status(504))]);
    let fetched = transport(&http, &root)
        .fetch_bytes(Get::direct(elsewhere, TIMEOUT), 1024)
        .await
        .unwrap();
    assert_eq!(fetched.status, 504);
    assert_eq!(fetched.served, ServedBy::Direct);
    assert_eq!(http.urls(), vec![elsewhere.to_string()]);

    let http = ScriptedHttp::new(vec![(elsewhere, Step::Fails("offline"))]);
    assert_eq!(
        transport(&http, &root)
            .fetch_bytes(Get::direct(elsewhere, TIMEOUT), 1024)
            .await
            .unwrap_err(),
        FetchError::Failed("offline".into())
    );
    assert_eq!(
        transport(&http, &root)
            .asset_url_after_outage(elsewhere, "HTTP 504", TIMEOUT)
            .await,
        Err("HTTP 504".to_string())
    );
}

#[tokio::test]
async fn both_failures_are_named() {
    let root = tempfile::tempdir().unwrap();
    let http = ScriptedHttp::new(vec![
        (URL, Step::Status(504)),
        (TAG_RELEASE, Step::Status(502)),
    ]);
    let error = transport(&http, &root)
        .fetch_bytes(Get::direct(URL, TIMEOUT), 1024)
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "direct download: HTTP 504; GitHub API fallback: GitHub API returned HTTP 502"
    );
}

#[tokio::test]
async fn a_403_rate_limit_backs_off_until_reset_while_direct_still_runs() {
    let root = tempfile::tempdir().unwrap();
    let http = ScriptedHttp::new(vec![
        (URL, Step::Status(504)),
        (
            TAG_RELEASE,
            Step::RateLimited {
                status: 403,
                reset_unix: NOW_MS / 1000 + 900,
            },
        ),
    ]);
    let first = transport(&http, &root)
        .fetch_bytes(Get::direct(URL, TIMEOUT), 1024)
        .await
        .unwrap_err()
        .to_string();
    assert!(first.contains("GitHub rate limit, retry at"), "{first}");

    // The next attempt, here a fresh transport as after a restart, tries direct and spends no API request.
    let http = ScriptedHttp::new(vec![(URL, Step::Status(504))]);
    let second = transport(&http, &root)
        .fetch_bytes(Get::direct(URL, TIMEOUT), 1024)
        .await
        .unwrap_err()
        .to_string();
    assert!(second.contains("GitHub rate limit, retry at"), "{second}");
    assert_eq!(http.urls(), vec![URL.to_string()]);

    let http = ScriptedHttp::new(vec![
        (URL, Step::Status(504)),
        (TAG_RELEASE, Step::Bytes(release_json("v1.19.0"))),
        (ASSET, Step::Bytes(b"payload".to_vec())),
    ]);
    let mut later = transport(&http, &root);
    later.now = Box::new(|| NOW_MS + 901_000);
    assert!(later
        .fetch_bytes(Get::direct(URL, TIMEOUT), 1024)
        .await
        .is_ok());
}

#[tokio::test]
async fn fallback_requests_count_against_the_hourly_budget() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("budget.json");
    let http = ScriptedHttp::new(vec![
        (URL, Step::Status(504)),
        (TAG_RELEASE, Step::Bytes(release_json("v1.19.0"))),
        (ASSET, Step::Bytes(b"payload".to_vec())),
    ]);
    transport(&http, &root)
        .fetch_bytes(Get::direct(URL, TIMEOUT), 1024)
        .await
        .unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["used"], 2);
}

#[tokio::test]
async fn a_downloader_gets_the_api_address_of_its_own_artifact() {
    let root = tempfile::tempdir().unwrap();
    let http = ScriptedHttp::new(vec![(TAG_RELEASE, Step::Bytes(release_json("v1.19.0")))]);
    assert_eq!(
        transport(&http, &root)
            .asset_url_after_outage(URL, "HTTP 504", TIMEOUT)
            .await
            .unwrap(),
        ASSET
    );
}

#[test]
fn only_a_5xx_status_message_from_the_native_downloader_is_an_outage() {
    for (message, outage) in [
        (
            "Download request failed with status: 504 Gateway Timeout",
            true,
        ),
        (
            "Download request failed with status: 500 Internal Server Error",
            true,
        ),
        ("Download request failed with status: 404 Not Found", false),
        ("Download request failed with status: 403 Forbidden", false),
        ("signature mismatch", false),
    ] {
        assert_eq!(download_status_is_outage(message), outage, "{message}");
    }
}

use super::*;
use std::cell::RefCell;
use std::collections::VecDeque;

const NOW_MS: u64 = 1_800_000_000_000;

struct Scripted {
    replies: RefCell<VecDeque<(&'static str, Result<BlockingReply, String>)>>,
    calls: RefCell<u32>,
}

impl Scripted {
    fn new(replies: Vec<(&'static str, Result<BlockingReply, String>)>) -> Self {
        Self {
            replies: RefCell::new(replies.into()),
            calls: RefCell::new(0),
        }
    }
}

impl BlockingHttp for Scripted {
    fn get(&self, url: &str, _accept: &str, _limit: usize) -> Result<BlockingReply, String> {
        *self.calls.borrow_mut() += 1;
        let (expected, reply) = self
            .replies
            .borrow_mut()
            .pop_front()
            .expect("unexpected request");
        assert_eq!(url, expected);
        reply
    }
}

fn ok(body: &str) -> Result<BlockingReply, String> {
    Ok(BlockingReply {
        status: 200,
        rate: RateHeaders::default(),
        body: body.as_bytes().to_vec(),
    })
}

const URL: &str = "https://github.com/acme/tool/releases/download/channel-latest/core.tar.gz";
const RELEASE: &str = "https://api.github.com/repos/acme/tool/releases/tags/channel-latest";
const ASSET: &str = "https://api.github.com/repos/acme/tool/releases/assets/7";
const RELEASE_JSON: &str = r#"{"tag_name":"channel-latest","assets":[
    {"id":6,"name":"core.tar.gz.sha256","url":"https://evil.example/x"},
    {"id":7,"name":"core.tar.gz","url":"https://evil.example/y"}]}"#;

fn store(root: &tempfile::TempDir) -> ApiBudgetStore {
    ApiBudgetStore::at(root.path().join("budget.json"))
}

#[test]
fn release_urls_map_by_owner_repo_tag_and_name() {
    assert_eq!(
        ReleaseAsset::parse(URL),
        Some(ReleaseAsset {
            owner: "acme".into(),
            repo: "tool".into(),
            selector: ReleaseSelector::Tag("channel-latest".into()),
            name: "core.tar.gz".into(),
        })
    );
    let latest =
        ReleaseAsset::parse("https://github.com/acme/tool/releases/latest/download/feed.json")
            .unwrap();
    assert_eq!(latest.selector, ReleaseSelector::Latest);
    assert_eq!(
        latest.release_api_url(),
        "https://api.github.com/repos/acme/tool/releases/latest"
    );
}

#[test]
fn a_url_that_is_not_an_exact_github_release_download_is_never_rewritten() {
    for url in [
        "https://example.com/acme/tool/releases/download/v1/a.json",
        "https://github.com.evil.example/acme/tool/releases/download/v1/a.json",
        "https://evil.example/github.com/acme/tool/releases/download/v1/a.json",
        "http://github.com/acme/tool/releases/download/v1/a.json",
        "https://github.com:8443/acme/tool/releases/download/v1/a.json",
        "https://user:secret@github.com/acme/tool/releases/download/v1/a.json",
        "https://github.com/acme/tool/releases/download/v1/a.json?x=1",
        "https://github.com/acme/tool/releases/download/v1/a.json#x",
        "https://github.com/acme/tool/releases/download/v1/nested/a.json",
        "https://github.com/acme/tool/archive/v1/a.json",
        "https://github.com/acme/tool/releases/download/v%2F1/a.json",
        "not a url",
    ] {
        assert_eq!(ReleaseAsset::parse(url), None, "{url}");
    }
    let root = tempfile::tempdir().unwrap();
    let http = Scripted::new(Vec::new());
    assert_eq!(
        fetch_after_outage(
            &http,
            &store(&root),
            "https://example.com/acme/tool/releases/download/v1/a.json",
            1024,
            "HTTP 504",
            &|| NOW_MS,
        ),
        Err("HTTP 504".to_string())
    );
    assert_eq!(*http.calls.borrow(), 0);
}

#[test]
fn asset_selection_uses_the_exact_name_and_never_a_url_from_the_response() {
    let asset = ReleaseAsset::parse(URL).unwrap();
    assert_eq!(
        asset.select_asset_url(RELEASE_JSON.as_bytes()).unwrap(),
        ASSET
    );
    let other_tag = r#"{"tag_name":"other","assets":[{"id":7,"name":"core.tar.gz"}]}"#;
    assert!(asset.select_asset_url(other_tag.as_bytes()).is_err());
    let similar =
        r#"{"tag_name":"channel-latest","assets":[{"id":7,"name":"core.tar.gz.sha256"}]}"#;
    assert!(asset.select_asset_url(similar.as_bytes()).is_err());
}

#[test]
fn api_redirects_stay_on_github_hosts() {
    for (url, allowed) in [
        ("https://release-assets.githubusercontent.com/a", true),
        ("https://objects.githubusercontent.com/a", true),
        ("https://api.github.com/a", true),
        ("http://release-assets.githubusercontent.com/a", false),
        ("https://githubusercontent.com.evil.example/a", false),
        ("https://evil.example/a", false),
    ] {
        assert_eq!(
            api_redirect_allowed(&reqwest::Url::parse(url).unwrap()),
            allowed,
            "{url}"
        );
    }
}

#[test]
fn an_outage_serves_the_same_bytes_through_the_api_and_counts_two_requests() {
    let root = tempfile::tempdir().unwrap();
    let http = Scripted::new(vec![(RELEASE, ok(RELEASE_JSON)), (ASSET, ok("archive"))]);
    let bytes =
        fetch_after_outage(&http, &store(&root), URL, 1024, "HTTP 504", &|| NOW_MS).unwrap();
    assert_eq!(bytes, b"archive");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("budget.json")).unwrap()).unwrap();
    assert_eq!(saved["used"], 2);
}

#[test]
fn both_failures_are_named_together() {
    let root = tempfile::tempdir().unwrap();
    let http = Scripted::new(vec![(
        RELEASE,
        Ok(BlockingReply {
            status: 502,
            rate: RateHeaders::default(),
            body: Vec::new(),
        }),
    )]);
    let error =
        fetch_after_outage(&http, &store(&root), URL, 1024, "HTTP 504", &|| NOW_MS).unwrap_err();
    assert_eq!(
        error,
        "direct download: HTTP 504; GitHub API fallback: GitHub API returned HTTP 502"
    );
}

#[test]
fn a_rate_limited_reply_backs_off_until_the_reset_even_after_a_restart() {
    let root = tempfile::tempdir().unwrap();
    let reset_unix = NOW_MS / 1000 + 600;
    let limited = Scripted::new(vec![(
        RELEASE,
        Ok(BlockingReply {
            status: 403,
            rate: RateHeaders {
                remaining: Some(0),
                reset_unix: Some(reset_unix),
                retry_after_secs: None,
            },
            body: Vec::new(),
        }),
    )]);
    let error =
        fetch_after_outage(&limited, &store(&root), URL, 1024, "HTTP 504", &|| NOW_MS).unwrap_err();
    assert!(error.contains("GitHub rate limit, retry at"), "{error}");

    // A new store over the same file stands in for a restart: no request is spent.
    let silent = Scripted::new(Vec::new());
    let error = fetch_after_outage(&silent, &store(&root), URL, 1024, "HTTP 504", &|| {
        NOW_MS + 599_000
    })
    .unwrap_err();
    assert!(error.contains("GitHub rate limit, retry at"), "{error}");
    assert_eq!(*silent.calls.borrow(), 0);

    let recovered = Scripted::new(vec![(RELEASE, ok(RELEASE_JSON)), (ASSET, ok("archive"))]);
    let bytes = fetch_after_outage(&recovered, &store(&root), URL, 1024, "HTTP 504", &|| {
        NOW_MS + 601_000
    })
    .unwrap();
    assert_eq!(bytes, b"archive");
}

#[test]
fn a_secondary_limit_backs_off_for_its_retry_after() {
    let mut budget = ApiBudget::default();
    let rate = RateHeaders {
        remaining: None,
        reset_unix: None,
        retry_after_secs: Some(90),
    };
    assert!(budget.observe(429, &rate, NOW_MS).is_some());
    assert!(budget.admit(NOW_MS + 89_000).is_err());
    assert!(budget.admit(NOW_MS + 91_000).is_ok());
}

#[test]
fn a_forbidden_reply_without_rate_headers_is_only_a_failure() {
    let mut budget = ApiBudget::default();
    assert_eq!(budget.observe(403, &RateHeaders::default(), NOW_MS), None);
    assert!(budget.admit(NOW_MS).is_ok());
}

#[test]
fn the_hourly_budget_is_bounded_and_refills() {
    let mut budget = ApiBudget::default();
    for _ in 0..API_REQUESTS_PER_HOUR {
        budget.admit(NOW_MS).unwrap();
    }
    assert!(budget.admit(NOW_MS + 1000).is_err());
    assert!(budget.admit(NOW_MS + WINDOW_FOR_TEST).is_ok());
}

const WINDOW_FOR_TEST: u64 = 60 * 60 * 1000;

#[test]
fn a_moved_clock_cannot_park_the_fallback_for_long() {
    let mut budget = ApiBudget::default();
    let rate = RateHeaders {
        remaining: Some(0),
        reset_unix: Some(NOW_MS / 1000 + 400 * 24 * 60 * 60),
        retry_after_secs: None,
    };
    assert!(budget.observe(403, &rate, NOW_MS).is_some());
    assert!(budget.admit(NOW_MS + 3 * WINDOW_FOR_TEST).is_ok());
}

#[test]
fn rate_headers_ignore_unusable_values() {
    let rate = RateHeaders::read(|name| match name {
        "x-ratelimit-remaining" => Some("0".into()),
        "x-ratelimit-reset" => Some("soon".into()),
        _ => None,
    });
    assert_eq!(rate.remaining, Some(0));
    assert_eq!(rate.reset_unix, None);
    assert_eq!(rate.retry_after_secs, None);
}

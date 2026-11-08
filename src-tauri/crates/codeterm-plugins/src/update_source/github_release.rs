//! Anonymous GitHub Releases API route to a published release asset.
//! Only the byte transport changes: callers verify what they receive exactly as before.

use serde::Deserialize;

mod budget;
pub use budget::{ApiBudget, ApiBudgetStore, API_REQUESTS_PER_HOUR};

pub const JSON_ACCEPT: &str = "application/vnd.github+json";
pub const ASSET_ACCEPT: &str = "application/octet-stream";
pub const MAX_RELEASE_JSON: usize = 4 * 1024 * 1024;

const API_ORIGIN: &str = "https://api.github.com";
const USER_AGENT: &str = "Domios-Updater";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseSelector {
    Latest,
    Tag(String),
}

/// A file addressed by a github.com release-download URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub owner: String,
    pub repo: String,
    pub selector: ReleaseSelector,
    pub name: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    id: u64,
    name: String,
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
}

impl ReleaseAsset {
    /// Only exact github.com release-download URLs map to the API; everything else stays as it is.
    pub fn parse(url: &str) -> Option<Self> {
        let parsed = reqwest::Url::parse(url).ok()?;
        if parsed.scheme() != "https"
            || parsed.host_str() != Some("github.com")
            || parsed.port().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return None;
        }
        let segments: Vec<&str> = parsed.path_segments()?.collect();
        let (owner, repo, selector, name) = match segments.as_slice() {
            [owner, repo, "releases", "latest", "download", name] => {
                (*owner, *repo, ReleaseSelector::Latest, *name)
            }
            [owner, repo, "releases", "download", tag, name] if safe_component(tag) => {
                (*owner, *repo, ReleaseSelector::Tag((*tag).into()), *name)
            }
            _ => return None,
        };
        if !safe_component(owner) || !safe_component(repo) || !safe_component(name) {
            return None;
        }
        Some(Self {
            owner: owner.into(),
            repo: repo.into(),
            selector,
            name: name.into(),
        })
    }

    pub fn release_api_url(&self) -> String {
        let release = match &self.selector {
            ReleaseSelector::Latest => "latest".to_string(),
            ReleaseSelector::Tag(tag) => format!("tags/{tag}"),
        };
        format!(
            "{API_ORIGIN}/repos/{}/{}/releases/{release}",
            self.owner, self.repo
        )
    }

    /// The asset URL is built from its id; a URL inside the response is never followed.
    pub fn select_asset_url(&self, release_json: &[u8]) -> Result<String, String> {
        let release: Release = serde_json::from_slice(release_json)
            .map_err(|error| format!("invalid release metadata: {error}"))?;
        if let ReleaseSelector::Tag(tag) = &self.selector {
            if &release.tag_name != tag {
                return Err(format!(
                    "release metadata names {} instead of {tag}",
                    release.tag_name
                ));
            }
        }
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == self.name)
            .ok_or_else(|| format!("release {} has no asset {}", release.tag_name, self.name))?;
        Ok(format!(
            "{API_ORIGIN}/repos/{}/{}/releases/assets/{}",
            self.owner, self.repo, asset.id
        ))
    }
}

pub fn is_outage_status(status: u16) -> bool {
    (500..=599).contains(&status)
}

/// 5xx, timeouts and connection failures reach the API; refusals and unsafe redirects do not.
pub fn is_outage_error(error: &reqwest::Error) -> bool {
    match error.status() {
        Some(status) => status.is_server_error(),
        None => error.is_connect() || error.is_timeout() || error.is_request(),
    }
}

pub fn api_redirect_allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.host_str().is_some_and(|host| {
            host == "github.com"
                || host == "api.github.com"
                || host.ends_with(".githubusercontent.com")
        })
}

pub fn both_failed(direct: &str, api: &str) -> String {
    format!("direct download: {direct}; GitHub API fallback: {api}")
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RateHeaders {
    pub remaining: Option<u32>,
    pub reset_unix: Option<u64>,
    pub retry_after_secs: Option<u64>,
}

impl RateHeaders {
    pub fn read(header: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            remaining: header("x-ratelimit-remaining").and_then(|value| value.parse().ok()),
            reset_unix: header("x-ratelimit-reset").and_then(|value| value.parse().ok()),
            retry_after_secs: header("retry-after").and_then(|value| value.parse().ok()),
        }
    }
}

pub struct BlockingReply {
    pub status: u16,
    pub rate: RateHeaders,
    pub body: Vec<u8>,
}

pub trait BlockingHttp {
    fn get(&self, url: &str, accept: &str, limit: usize) -> Result<BlockingReply, String>;
}

fn api_get(
    http: &dyn BlockingHttp,
    budget: &ApiBudgetStore,
    url: &str,
    accept: &str,
    limit: usize,
    now: &dyn Fn() -> u64,
) -> Result<Vec<u8>, String> {
    budget.admit(now())?;
    let reply = http.get(url, accept, limit)?;
    let limited = budget.observe(reply.status, &reply.rate, now());
    if (200..300).contains(&reply.status) {
        return Ok(reply.body);
    }
    Err(limited.unwrap_or_else(|| format!("GitHub API returned HTTP {}", reply.status)))
}

/// Serves the same file through the API after a direct outage; other hosts keep the direct failure.
pub fn fetch_after_outage(
    http: &dyn BlockingHttp,
    budget: &ApiBudgetStore,
    url: &str,
    limit: usize,
    direct: &str,
    now: &dyn Fn() -> u64,
) -> Result<Vec<u8>, String> {
    let Some(asset) = ReleaseAsset::parse(url) else {
        return Err(direct.to_owned());
    };
    log::warn!(
        "[updater] direct download of {} failed ({direct}); trying the GitHub Releases API",
        asset.name
    );
    let serve = || -> Result<Vec<u8>, String> {
        let release = api_get(
            http,
            budget,
            &asset.release_api_url(),
            JSON_ACCEPT,
            MAX_RELEASE_JSON,
            now,
        )?;
        let asset_url = asset.select_asset_url(&release)?;
        api_get(http, budget, &asset_url, ASSET_ACCEPT, limit, now)
    };
    serve().map_err(|api| both_failed(direct, &api))
}

struct OfficialApi {
    client: reqwest::blocking::Client,
}

impl OfficialApi {
    fn new() -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 || !api_redirect_allowed(attempt.url()) {
                    attempt.error("Unsafe update redirect")
                } else {
                    attempt.follow()
                }
            }))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { client })
    }
}

impl BlockingHttp for OfficialApi {
    fn get(&self, url: &str, accept: &str, limit: usize) -> Result<BlockingReply, String> {
        use std::io::Read;
        let response = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT, accept)
            .send()
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let rate = RateHeaders::read(|name| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        });
        let mut body = Vec::new();
        if status.is_success() {
            response
                .take(limit as u64 + 1)
                .read_to_end(&mut body)
                .map_err(|error| error.to_string())?;
            if body.len() > limit {
                return Err("Download exceeds limit".into());
            }
        }
        Ok(BlockingReply {
            status: status.as_u16(),
            rate,
            body,
        })
    }
}

pub fn official_fallback(url: &str, limit: usize, direct: &str) -> Result<Vec<u8>, String> {
    if ReleaseAsset::parse(url).is_none() {
        return Err(direct.to_owned());
    }
    let http = OfficialApi::new().map_err(|api| both_failed(direct, &api))?;
    fetch_after_outage(
        &http,
        &ApiBudgetStore::official(),
        url,
        limit,
        direct,
        &now_ms,
    )
}

#[cfg(test)]
mod tests;

//! Byte transport for release files: the published URL first, the GitHub Releases API after an outage.
//! Callers verify signatures and hashes on whatever arrives, whichever path served it.

use std::time::Duration;

pub use codeterm_plugins::update_source::github_release::both_failed;
use codeterm_plugins::update_source::github_release::{
    self, ApiBudgetStore, RateHeaders, ReleaseAsset,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServedBy {
    Direct,
    GithubApi,
}

impl ServedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::GithubApi => "github_api",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Direct,
    Api,
}

#[derive(Debug, Clone, Copy)]
pub struct Get<'a> {
    pub url: &'a str,
    pub route: Route,
    pub accept: Option<&'a str>,
    pub if_none_match: Option<&'a str>,
    pub no_cache: bool,
    pub timeout: Duration,
}

impl<'a> Get<'a> {
    pub fn direct(url: &'a str, timeout: Duration) -> Self {
        Self {
            url,
            route: Route::Direct,
            accept: None,
            if_none_match: None,
            no_cache: false,
            timeout,
        }
    }

    fn api(url: &'a str, accept: &'a str, timeout: Duration) -> Self {
        Self {
            url,
            route: Route::Api,
            accept: Some(accept),
            if_none_match: None,
            no_cache: false,
            timeout,
        }
    }
}

#[async_trait::async_trait]
pub trait Body: Send {
    async fn chunk(&mut self) -> Result<Option<Vec<u8>>, String>;
}

pub struct Reply {
    pub status: u16,
    pub etag: Option<String>,
    pub retry_after: Option<String>,
    pub rate: RateHeaders,
    pub body: Box<dyn Body>,
}

impl Reply {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

#[async_trait::async_trait]
pub trait Http: Send + Sync {
    async fn get(&self, request: Get<'_>) -> Result<Reply, String>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum FetchError {
    TooLarge,
    Failed(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => write!(f, "Download exceeds limit"),
            Self::Failed(cause) => write!(f, "{cause}"),
        }
    }
}

#[derive(Debug)]
pub struct Fetched {
    pub status: u16,
    pub etag: Option<String>,
    pub retry_after: Option<String>,
    pub body: Vec<u8>,
    pub served: ServedBy,
}

async fn read_body(body: &mut dyn Body, limit: usize) -> Result<Vec<u8>, FetchError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = body.chunk().await.map_err(FetchError::Failed)? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(FetchError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn read_reply(
    mut reply: Reply,
    served: ServedBy,
    limit: usize,
) -> Result<Fetched, FetchError> {
    let body = if reply.is_success() {
        read_body(reply.body.as_mut(), limit).await?
    } else {
        Vec::new()
    };
    Ok(Fetched {
        status: reply.status,
        // A validator from another route must not be replayed against the direct URL.
        etag: reply.etag.filter(|_| served == ServedBy::Direct),
        retry_after: reply.retry_after,
        body,
        served,
    })
}

/// Tauri's updater reports a failed status only as text: `Download request failed with status: 504 ...`.
pub fn download_status_is_outage(message: &str) -> bool {
    message
        .rsplit("status: ")
        .next()
        .and_then(|status| status.split_whitespace().next())
        .and_then(|status| status.parse::<u16>().ok())
        .is_some_and(github_release::is_outage_status)
}

pub struct Transport<'a> {
    pub(crate) http: &'a dyn Http,
    pub(crate) budget: ApiBudgetStore,
    pub(crate) now: Box<dyn Fn() -> u64 + Send + Sync>,
}

impl<'a> Transport<'a> {
    pub fn official(http: &'a dyn Http) -> Self {
        Self {
            http,
            budget: ApiBudgetStore::official(),
            now: Box::new(github_release::now_ms),
        }
    }

    async fn admit(&self) -> Result<(), String> {
        let budget = self.budget.clone();
        let now = (self.now)();
        tokio::task::spawn_blocking(move || budget.admit(now))
            .await
            .map_err(|error| error.to_string())?
    }

    async fn observe(&self, status: u16, rate: RateHeaders) -> Result<Option<String>, String> {
        let budget = self.budget.clone();
        let now = (self.now)();
        tokio::task::spawn_blocking(move || budget.observe(status, &rate, now))
            .await
            .map_err(|error| error.to_string())
    }

    async fn api_get(&self, url: &str, accept: &str, timeout: Duration) -> Result<Reply, String> {
        self.admit().await?;
        let reply = self.http.get(Get::api(url, accept, timeout)).await?;
        let limited = self.observe(reply.status, reply.rate.clone()).await?;
        if reply.is_success() {
            return Ok(reply);
        }
        Err(limited.unwrap_or_else(|| format!("GitHub API returned HTTP {}", reply.status)))
    }

    async fn resolve_asset(
        &self,
        asset: &ReleaseAsset,
        timeout: Duration,
    ) -> Result<String, String> {
        let mut reply = self
            .api_get(
                &asset.release_api_url(),
                github_release::JSON_ACCEPT,
                timeout,
            )
            .await?;
        let release = read_body(reply.body.as_mut(), github_release::MAX_RELEASE_JSON)
            .await
            .map_err(|error| error.to_string())?;
        asset.select_asset_url(&release)
    }

    /// The API address of the file behind `url`; other hosts keep the direct failure.
    pub async fn asset_url_after_outage(
        &self,
        url: &str,
        direct: &str,
        timeout: Duration,
    ) -> Result<String, String> {
        let Some(asset) = ReleaseAsset::parse(url) else {
            return Err(direct.to_owned());
        };
        log::warn!(
            "[updater] direct download of {} failed ({direct}); trying the GitHub Releases API",
            asset.name
        );
        self.resolve_asset(&asset, timeout)
            .await
            .map_err(|api| github_release::both_failed(direct, &api))
    }

    /// Opens the file behind `url` through the API after `direct` failed.
    pub async fn fallback(
        &self,
        url: &str,
        direct: &str,
        timeout: Duration,
    ) -> Result<Reply, String> {
        let asset_url = self.asset_url_after_outage(url, direct, timeout).await?;
        self.api_get(&asset_url, github_release::ASSET_ACCEPT, timeout)
            .await
            .map_err(|api| github_release::both_failed(direct, &api))
    }

    async fn after_direct(
        &self,
        url: &str,
        direct: Result<Reply, String>,
        timeout: Duration,
    ) -> Result<(Reply, ServedBy), String> {
        if ReleaseAsset::parse(url).is_none() {
            return direct.map(|reply| (reply, ServedBy::Direct));
        }
        let failure = match direct {
            Ok(reply) if !github_release::is_outage_status(reply.status) => {
                return Ok((reply, ServedBy::Direct));
            }
            Ok(reply) => format!("HTTP {}", reply.status),
            Err(error) => error,
        };
        let reply = self.fallback(url, &failure, timeout).await?;
        Ok((reply, ServedBy::GithubApi))
    }

    /// Reads a whole file. A non-success status that is not an outage comes back for the caller to judge.
    pub async fn fetch_bytes(&self, request: Get<'_>, limit: usize) -> Result<Fetched, FetchError> {
        let direct = self.http.get(request).await;
        let (reply, served) = self
            .after_direct(request.url, direct, request.timeout)
            .await
            .map_err(FetchError::Failed)?;
        match read_reply(reply, served, limit).await {
            Err(FetchError::Failed(cause))
                if served == ServedBy::Direct && ReleaseAsset::parse(request.url).is_some() =>
            {
                let reply = self
                    .fallback(request.url, &cause, request.timeout)
                    .await
                    .map_err(FetchError::Failed)?;
                read_reply(reply, ServedBy::GithubApi, limit).await
            }
            outcome => outcome,
        }
    }
}

struct ReqwestBody(reqwest::Response);

#[async_trait::async_trait]
impl Body for ReqwestBody {
    async fn chunk(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.0
            .chunk()
            .await
            .map(|chunk| chunk.map(|bytes| bytes.to_vec()))
            .map_err(|error| error.to_string())
    }
}

pub struct ReqwestHttp {
    direct: reqwest::Client,
    api: reqwest::Client,
}

impl ReqwestHttp {
    /// `direct` keeps the caller's own redirect and TLS rules; API requests stay on GitHub hosts.
    pub fn new(direct: reqwest::Client) -> Result<Self, String> {
        let api = reqwest::Client::builder()
            .https_only(true)
            .user_agent("Domios-Updater")
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5
                    || !github_release::api_redirect_allowed(attempt.url())
                {
                    attempt.error("Unsafe update redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { direct, api })
    }
}

#[async_trait::async_trait]
impl Http for ReqwestHttp {
    async fn get(&self, request: Get<'_>) -> Result<Reply, String> {
        let client = match request.route {
            Route::Direct => &self.direct,
            Route::Api => &self.api,
        };
        let mut builder = client.get(request.url).timeout(request.timeout);
        if let Some(accept) = request.accept {
            builder = builder.header(reqwest::header::ACCEPT, accept);
        }
        if let Some(etag) = request.if_none_match {
            builder = builder.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        if request.no_cache {
            builder = builder.header(reqwest::header::CACHE_CONTROL, "no-cache");
        }
        let response = builder.send().await.map_err(|error| error.to_string())?;
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let status = response.status().as_u16();
        let etag = header("etag");
        let retry_after = header("retry-after");
        let rate = RateHeaders::read(header);
        Ok(Reply {
            status,
            etag,
            retry_after,
            rate,
            body: Box::new(ReqwestBody(response)),
        })
    }
}

/// The API address of an update artifact, for a downloader that owns its own transfer.
pub async fn official_asset_url(
    url: &str,
    direct: &str,
    timeout: Duration,
) -> Result<String, String> {
    let http = ReqwestHttp::new(reqwest::Client::new())
        .map_err(|api| github_release::both_failed(direct, &api))?;
    let transport = Transport::official(&http);
    let asset_url = transport
        .asset_url_after_outage(url, direct, timeout)
        .await?;
    Ok(asset_url)
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests;

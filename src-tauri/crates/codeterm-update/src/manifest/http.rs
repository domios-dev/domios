use std::{
    collections::HashMap,
    future::Future,
    sync::Arc,
    time::{Duration, SystemTime},
};

use super::Manifest;
use crate::discovery_policy::{backoff, EVENT_THROTTLE};
use crate::release_transport::{Get, ReqwestHttp, ServedBy, Transport};

const LIMIT: usize = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(60);
type EndpointFlight = Arc<tokio::sync::Mutex<Endpoint>>;

static CACHE: Cache = Cache {
    endpoints: parking_lot::Mutex::new(None),
};

#[derive(Default)]
struct Cache {
    endpoints: parking_lot::Mutex<Option<HashMap<String, EndpointFlight>>>,
}

#[derive(Default)]
struct Endpoint {
    manifest: Option<Manifest>,
    etag: Option<String>,
    failures: u32,
    next: Option<tokio::time::Instant>,
    error: Option<String>,
}

struct Reply {
    status: u16,
    etag: Option<String>,
    retry_after: Option<String>,
    body: Vec<u8>,
    served: ServedBy,
}

pub(super) async fn fetch(url: &str) -> Result<Manifest, String> {
    CACHE.check(url, |etag| send(url, etag)).await
}

pub(super) async fn expire(url: &str) -> Result<(), String> {
    CACHE.expire(url).await
}

impl Cache {
    async fn expire(&self, url: &str) -> Result<(), String> {
        let endpoint = self.endpoint(url)?;
        endpoint.lock().await.next = None;
        Ok(())
    }

    fn endpoint(&self, url: &str) -> Result<EndpointFlight, String> {
        let mut cache = self.endpoints.lock();
        let cache = cache.get_or_insert_with(HashMap::new);
        if !cache.contains_key(url) && cache.len() >= 16 {
            return Err("Too many manifest discovery endpoints".into());
        }
        Ok(Arc::clone(cache.entry(url.into()).or_default()))
    }

    async fn check<F, Fut>(&self, url: &str, send: F) -> Result<Manifest, String>
    where
        F: FnOnce(Option<String>) -> Fut,
        Fut: Future<Output = Result<Reply, String>>,
    {
        let endpoint = self.endpoint(url)?;
        let mut endpoint = endpoint.lock().await;
        endpoint
            .check(tokio::time::Instant::now, SystemTime::now(), send)
            .await
    }
}

impl Endpoint {
    async fn check<F, Fut, Clock>(
        &mut self,
        now: Clock,
        wall: SystemTime,
        send: F,
    ) -> Result<Manifest, String>
    where
        F: FnOnce(Option<String>) -> Fut,
        Fut: Future<Output = Result<Reply, String>>,
        Clock: Fn() -> tokio::time::Instant,
    {
        if self.next.is_some_and(|next| now() < next) {
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
            if let Some(manifest) = &self.manifest {
                return Ok(manifest.clone());
            }
        }
        let reply = send(self.etag.clone()).await;
        let retry = reply.as_ref().ok().and_then(|reply| {
            matches!(reply.status, 403 | 429)
                .then(|| retry_after(reply.retry_after.as_deref(), wall))
                .flatten()
        });
        let served = reply.as_ref().ok().map(|reply| reply.served);
        let result: Result<Manifest, String> = reply.and_then(|reply| match reply.status {
            304 => self
                .manifest
                .clone()
                .ok_or_else(|| "Manifest returned 304 without cached content".into()),
            200..=299 => {
                let manifest: Manifest = serde_json::from_slice(&reply.body)
                    .map_err(|error| format!("manifest: {error}"))?;
                semver::Version::parse(&manifest.version).map_err(|error| error.to_string())?;
                self.etag = reply.etag;
                Ok(manifest)
            }
            status => Err(format!("manifest fetch returned HTTP {status}")),
        });
        let result = result.map(|mut manifest| {
            manifest.served_by = served;
            manifest
        });
        match &result {
            Ok(manifest) => {
                self.manifest = Some(manifest.clone());
                self.failures = 0;
                self.error = None;
                self.next = Some(now() + EVENT_THROTTLE);
            }
            Err(error) => {
                self.failures = self.failures.saturating_add(1);
                self.error = Some(error.clone());
                let now = now();
                self.next = now
                    .checked_add(retry.unwrap_or_else(|| backoff(self.failures)))
                    .or_else(|| now.checked_add(crate::discovery_policy::INTERVAL));
            }
        }
        result
    }
}

fn retry_after(value: Option<&str>, now: SystemTime) -> Option<Duration> {
    let value = value?;
    let delay = value
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
        .or_else(|| {
            httpdate::parse_http_date(value)
                .ok()
                .map(|date| date.duration_since(now).unwrap_or_default())
        })?;
    Some(delay.max(EVENT_THROTTLE))
}

fn allowed(url: &reqwest::Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && url.scheme() != "file"
        && super::validate_url(url.as_str()).is_ok()
}

async fn send(url: &str, etag: Option<String>) -> Result<Reply, String> {
    let parsed = reqwest::Url::parse(url).map_err(|error| error.to_string())?;
    if !allowed(&parsed) {
        return Err("Unsafe update URL".into());
    }
    let client = reqwest::Client::builder()
        .user_agent("Domios-Updater")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !allowed(attempt.url()) {
                attempt.error("Unsafe update redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|error| error.to_string())?;
    let http = ReqwestHttp::new(client)?;
    let transport = Transport::official(&http);
    let reply = send_through(&transport, url, etag).await?;
    Ok(reply)
}

async fn send_through(
    transport: &Transport<'_>,
    url: &str,
    etag: Option<String>,
) -> Result<Reply, String> {
    let request = Get {
        if_none_match: etag.as_deref(),
        no_cache: true,
        ..Get::direct(url, TIMEOUT)
    };
    let fetched = transport
        .fetch_bytes(request, LIMIT)
        .await
        .map_err(|error| format!("manifest fetch: {error}"))?;
    Ok(Reply {
        status: fetched.status,
        etag: fetched.etag,
        retry_after: fetched.retry_after,
        body: fetched.body,
        served: fetched.served,
    })
}

#[cfg(test)]
mod tests;

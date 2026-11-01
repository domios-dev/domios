use std::path::{Path, PathBuf};

pub trait UpdateDiscoverySource: Send + Sync {
    fn fetch(&self, url: &str, limit: usize) -> Result<Vec<u8>, String>;
    fn materialize_channel(&self, name: &str, url: &str, dest: &Path) -> Result<(), String>;
}

pub struct OfficialUpdateSource;

impl UpdateDiscoverySource for OfficialUpdateSource {
    fn fetch(&self, url: &str, limit: usize) -> Result<Vec<u8>, String> {
        use std::io::Read;
        if !discovery_url_allowed(url) {
            return Err("Update discovery requires HTTPS".into());
        }
        let client = reqwest::blocking::Client::builder()
            .user_agent("Domios-Updater")
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 || !discovery_url_allowed(attempt.url().as_str()) {
                    attempt.error("Unsafe update redirect")
                } else {
                    attempt.follow()
                }
            }))
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        let response = client
            .get(url)
            .query(&[("codeterm_cache_bust", uuid::Uuid::new_v4().to_string())])
            .header(reqwest::header::CACHE_CONTROL, "no-cache, no-store")
            .send()
            .map_err(|e| {
                log::warn!("update discovery fetch failed: {e}");
                super::channel_failure::http_failure_cause(&e)
            })?;
        if let Some(error) = rate_limit_error(response.status().as_u16(), response.headers()) {
            return Err(error);
        }
        let response = response
            .error_for_status()
            .map_err(|e| super::channel_failure::http_failure_cause(&e))?;
        let mut bytes = Vec::new();
        response
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > limit {
            return Err("Update discovery exceeds limit".into());
        }
        Ok(bytes)
    }

    fn materialize_channel(&self, _name: &str, url: &str, dest: &Path) -> Result<(), String> {
        let source = super::parse_channel_source(url);
        if super::looks_like_local_path(&source.base) {
            return super::copy_dir_filtered(&source.source_dir(), dest);
        }
        let result = if super::archive_channel::is_archive_url(url) {
            super::archive_channel::materialize(|url, limit| self.fetch(url, limit), url, dest)
        } else {
            super::clone_git_channel(&source, dest)
        };
        result.map_err(|cause| super::channel_failure::channel_unavailable(&cause, url))
    }
}

fn rate_limit_error(status: u16, headers: &reqwest::header::HeaderMap) -> Option<String> {
    if !matches!(status, 403 | 429) || headers.get("x-ratelimit-remaining")?.to_str().ok()? != "0" {
        return None;
    }
    let reset = headers
        .get("x-ratelimit-reset")?
        .to_str()
        .ok()?
        .parse::<i64>()
        .ok()?;
    let reset = chrono::DateTime::from_timestamp(reset, 0)?;
    Some(format!(
        "GitHub rate limit, retry at {}",
        reset.with_timezone(&chrono::Local).format("%H:%M")
    ))
}

#[cfg(debug_assertions)]
pub struct FixtureUpdateSource {
    pub root: PathBuf,
}

#[cfg(debug_assertions)]
impl UpdateDiscoverySource for FixtureUpdateSource {
    fn fetch(&self, url: &str, limit: usize) -> Result<Vec<u8>, String> {
        let name = url
            .split('?')
            .next()
            .unwrap_or(url)
            .rsplit('/')
            .next()
            .unwrap_or_default();
        super::validate_relative_path(name)?;
        let bytes =
            std::fs::read(self.root.join(name)).map_err(|e| format!("fixture {name}: {e}"))?;
        if bytes.len() > limit {
            return Err("Update fixture exceeds limit".into());
        }
        Ok(bytes)
    }

    fn materialize_channel(&self, name: &str, _url: &str, dest: &Path) -> Result<(), String> {
        super::validate_relative_path(name)?;
        super::copy_dir_filtered(&self.root.join("channels").join(name), dest)
    }
}

pub fn discovery_source() -> Box<dyn UpdateDiscoverySource> {
    #[cfg(debug_assertions)]
    if let Some(root) = fixture_root() {
        return Box::new(FixtureUpdateSource { root });
    }
    Box::new(OfficialUpdateSource)
}

pub fn fixture_root() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    {
        std::env::var_os("CODETERM_DEV_UPDATE_FIXTURES").map(PathBuf::from)
    }
    #[cfg(not(debug_assertions))]
    {
        None
    }
}

fn discovery_url_allowed(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    if url.scheme() == "https" {
        return true;
    }
    #[cfg(debug_assertions)]
    if url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        })
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests;

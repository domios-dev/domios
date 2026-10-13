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
            .user_agent("CodeTerm-Updater")
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
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?;
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
            super::copy_dir_filtered(&source.source_dir(), dest)
        } else {
            super::clone_git_channel(&source, dest)
        }
    }
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
mod tests {
    use super::*;
    #[test]
    fn discovery_refuses_untrusted_schemes_and_credentials() {
        assert!(!discovery_url_allowed("file:///tmp/latest.json"));
        assert!(!discovery_url_allowed("http://example.com/latest.json"));
        assert!(!discovery_url_allowed(
            "https://user:secret@example.com/latest.json"
        ));
        assert!(discovery_url_allowed("https://example.com/latest.json"));
    }
    #[cfg(debug_assertions)]
    #[test]
    fn fixture_source_fetches_manifests_without_network_and_enforces_limits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("latest.json"), b"fixture").unwrap();
        let fixture = FixtureUpdateSource {
            root: dir.path().into(),
        };
        let source: &dyn UpdateDiscoverySource = &fixture;
        assert_eq!(
            source
                .fetch("https://example.invalid/latest.json?cache=1", 7)
                .unwrap(),
            b"fixture"
        );
        assert!(source
            .fetch("https://example.invalid/latest.json", 6)
            .is_err());
        assert!(source
            .fetch("https://example.invalid/missing.json", 16)
            .is_err());
    }
    #[cfg(not(debug_assertions))]
    #[test]
    fn release_never_selects_fixtures_or_loopback() {
        assert_eq!(fixture_root(), None);
        assert!(!discovery_url_allowed("http://127.0.0.1/latest.json"));
    }
}

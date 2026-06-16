//! Built-in transcriber: posts an audio file path to `{daemon_url}/transcribe`
//! and parses the returned text. A short-lived per-URL health cache prevents us
//! from pummeling a known-down daemon when many voice messages arrive at once.
//!
//! This is the native engine promoted out of `bridge/voice_transcribe.rs`. It is
//! registered as `builtin-http` so voice notes work with no plugin installed, and
//! it doubles as the parity reference for the JS transcriber.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::Deserialize;

use super::{TranscribeError, Transcriber, TranscriberStatus, Transcript};
use crate::config::schema::VoiceTranscribeConfig;

pub const BUILTIN_HTTP_ID: &str = "builtin-http";

/// The built-in HTTP transcribe engine, configured from `VoiceTranscribeConfig`.
pub struct HttpEngine {
    enabled: bool,
    daemon_url: String,
    timeout_ms: u64,
    language: String,
    health_check_ttl_secs: u64,
}

impl HttpEngine {
    pub fn from_config(cfg: &VoiceTranscribeConfig) -> Self {
        Self {
            enabled: cfg.enabled,
            daemon_url: cfg.daemon_url.clone(),
            timeout_ms: cfg.timeout_ms,
            language: cfg.language.clone(),
            health_check_ttl_secs: cfg.health_check_ttl_secs,
        }
    }
}

#[derive(Deserialize)]
struct TranscribeResponse {
    text: Option<String>,
}

#[derive(Clone, Copy)]
struct HealthEntry {
    healthy: bool,
    checked_at: Instant,
}

fn health_cache() -> &'static Mutex<HashMap<String, HealthEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, HealthEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_health(url: &str, ttl: Duration) -> Option<bool> {
    let map = health_cache().lock();
    map.get(url)
        .filter(|e| e.checked_at.elapsed() < ttl)
        .map(|e| e.healthy)
}

fn store_health(url: &str, healthy: bool) {
    let mut map = health_cache().lock();
    map.insert(
        url.to_string(),
        HealthEntry {
            healthy,
            checked_at: Instant::now(),
        },
    );
}

/// Reset the health cache. Test-only escape hatch — production relies on the TTL.
#[cfg(test)]
pub fn reset_health_cache() {
    health_cache().lock().clear();
}

fn build_client(timeout: Duration) -> Result<reqwest::Client, TranscribeError> {
    reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| TranscribeError::RequestFailed(e.to_string()))
}

async fn check_health(client: &reqwest::Client, daemon_url: &str) -> Result<(), TranscribeError> {
    let url = format!("{}/healthz", daemon_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| TranscribeError::HealthFailed(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(TranscribeError::HealthFailed(format!(
            "HTTP {}",
            resp.status().as_u16()
        )));
    }
    Ok(())
}

fn is_valid_http_url(url: &str) -> bool {
    let lower = url.trim();
    let rest = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"));
    match rest {
        Some(host_and_more) => {
            let host = host_and_more.split('/').next().unwrap_or("").trim();
            !host.is_empty() && !host.contains(' ')
        }
        None => false,
    }
}

fn path_to_string(path: &Path) -> String {
    PathBuf::from(path).to_string_lossy().to_string()
}

#[async_trait]
impl Transcriber for HttpEngine {
    fn id(&self) -> &str {
        BUILTIN_HTTP_ID
    }

    async fn status(&self) -> TranscriberStatus {
        if !self.enabled {
            return TranscriberStatus::Unavailable {
                reason: "disabled".into(),
            };
        }
        if !is_valid_http_url(&self.daemon_url) {
            return TranscriberStatus::Unavailable {
                reason: format!("invalid daemon URL: {}", self.daemon_url),
            };
        }
        let ttl = Duration::from_secs(self.health_check_ttl_secs.max(1));
        match cached_health(&self.daemon_url, ttl) {
            Some(true) => TranscriberStatus::Ready,
            Some(false) => TranscriberStatus::Unavailable {
                reason: "daemon unhealthy".into(),
            },
            None => TranscriberStatus::Ready, // unknown until first transcribe
        }
    }

    async fn transcribe(
        &self,
        audio_path: &Path,
        language: &str,
    ) -> Result<Transcript, TranscribeError> {
        if !self.enabled {
            return Err(TranscribeError::Disabled);
        }
        if audio_path.as_os_str().is_empty() {
            return Err(TranscribeError::EmptyPath);
        }
        if !is_valid_http_url(&self.daemon_url) {
            return Err(TranscribeError::InvalidUrl(self.daemon_url.clone()));
        }

        let timeout = Duration::from_millis(self.timeout_ms.max(1));
        let client = build_client(timeout)?;
        let ttl = Duration::from_secs(self.health_check_ttl_secs.max(1));

        let cached = cached_health(&self.daemon_url, ttl);
        if cached == Some(false) {
            return Err(TranscribeError::HealthFailed("cached unhealthy".into()));
        }
        if cached.is_none() {
            match check_health(&client, &self.daemon_url).await {
                Ok(()) => store_health(&self.daemon_url, true),
                Err(e) => {
                    store_health(&self.daemon_url, false);
                    return Err(e);
                }
            }
        }

        let endpoint = format!("{}/transcribe", self.daemon_url.trim_end_matches('/'));
        // An explicit `language` arg overrides the engine default.
        let lang = if language.trim().is_empty() {
            self.language.clone()
        } else {
            language.to_string()
        };
        let mut body = serde_json::json!({ "path": path_to_string(audio_path) });
        if !lang.trim().is_empty() {
            body["language"] = serde_json::Value::String(lang);
        }

        let resp = client
            .post(&endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| TranscribeError::RequestFailed(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(TranscribeError::HttpStatus(status.as_u16()));
        }
        let parsed: TranscribeResponse = resp
            .json()
            .await
            .map_err(|e| TranscribeError::ParseError(e.to_string()))?;
        let text = parsed.text.unwrap_or_default().trim().to_string();
        if text.is_empty() {
            return Err(TranscribeError::Empty);
        }
        Ok(Transcript { text })
    }
}

#[cfg(test)]
mod tests;

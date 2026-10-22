use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestPlatform {
    pub url: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub pub_date: Option<String>,
    #[serde(default)]
    pub platforms: HashMap<String, ManifestPlatform>,
}

pub fn current_platform_key() -> Option<&'static str> {
    platform_key(
        std::env::consts::OS,
        std::env::consts::ARCH,
        macos_process_is_translated(),
    )
}

fn platform_key(os: &str, arch: &str, macos_translated: bool) -> Option<&'static str> {
    if os == "windows" && arch == "x86_64" {
        Some("windows-x86_64")
    } else if os == "macos" && (arch == "aarch64" || macos_translated) {
        Some("darwin-aarch64")
    } else if os == "macos" && arch == "x86_64" {
        Some("darwin-x86_64")
    } else if os == "linux" && arch == "x86_64" {
        Some("linux-x86_64")
    } else if os == "linux" && arch == "aarch64" {
        Some("linux-aarch64")
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn macos_process_is_translated() -> bool {
    let name = b"sysctl.proc_translated\0";
    let mut value: libc::c_int = 0;
    let mut size = std::mem::size_of_val(&value);
    let result = unsafe {
        libc::sysctlbyname(
            name.as_ptr().cast(),
            (&mut value as *mut libc::c_int).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    result == 0 && value == 1
}

#[cfg(not(target_os = "macos"))]
fn macos_process_is_translated() -> bool {
    false
}

pub fn validate_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|error| error.to_string())?;
    let allowed = parsed.scheme() == "https"
        || (cfg!(feature = "dev-updater-allowlocal")
            && (parsed.scheme() == "file"
                || (parsed.scheme() == "http"
                    && matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))));
    if allowed {
        Ok(())
    } else {
        Err("Updater URLs require HTTPS; local tests require dev-updater-allowlocal".into())
    }
}

pub async fn fetch(url: &str) -> Result<Manifest, String> {
    validate_url(url)?;
    if codeterm_plugins::update_source::fixture_root().is_some() {
        let url = url.to_owned();
        return tokio::task::spawn_blocking(move || {
            let bytes =
                codeterm_plugins::update_source::discovery_source().fetch(&url, 1024 * 1024)?;
            serde_json::from_slice(&bytes).map_err(|e| format!("update fixture: {e}"))
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    let parsed = reqwest::Url::parse(url).map_err(|error| error.to_string())?;
    if parsed.scheme() == "file" {
        return load_file_manifest(url);
    }
    fetch_http(url).await
}

#[cfg(feature = "dev-updater-allowlocal")]
fn load_file_manifest(url: &str) -> Result<Manifest, String> {
    let path = reqwest::Url::parse(url)
        .map_err(|error| error.to_string())?
        .to_file_path()
        .map_err(|_| "invalid local manifest path")?;
    let body = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read manifest at {}: {error}", path.display()))?;
    serde_json::from_str(&body).map_err(|e| format!("failed to parse manifest JSON: {}", e))
}

#[cfg(not(feature = "dev-updater-allowlocal"))]
fn load_file_manifest(url: &str) -> Result<Manifest, String> {
    Err(format!(
        "file:// manifest URLs are disabled in this build: {}",
        url
    ))
}

async fn fetch_http(url: &str) -> Result<Manifest, String> {
    if url.starts_with("https://") {
        let url = url.to_string();
        return tokio::task::spawn_blocking(move || {
            let bytes =
                codeterm_plugins::update_source::discovery_source().fetch(&url, 1024 * 1024)?;
            serde_json::from_slice(&bytes).map_err(|e| format!("manifest: {e}"))
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    let client = reqwest::Client::builder()
        .user_agent("CodeTerm-Updater")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("failed to fetch manifest: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("manifest fetch returned HTTP {}", resp.status()));
    }
    resp.json::<Manifest>()
        .await
        .map_err(|e| format!("failed to parse manifest JSON: {}", e))
}

pub fn diff(local: &str, m: &Manifest) -> Result<Option<String>, String> {
    let local_v = semver::Version::parse(local)
        .map_err(|e| format!("invalid local version '{}': {}", local, e))?;
    let remote_v = semver::Version::parse(&m.version)
        .map_err(|e| format!("invalid manifest version '{}': {}", m.version, e))?;
    if remote_v > local_v {
        Ok(Some(m.version.clone()))
    } else {
        Ok(None)
    }
}

/// A client is only ever offered an artifact that exists for its own platform.
/// `diff` alone answers "is the manifest newer"; this additionally requires the
/// manifest to actually publish an entry for `platform` before treating the
/// newer version as an offer — a manifest whose matrix covers only some
/// platforms (a staggered publish, or a genuinely partial release) must not
/// read as "update available" to a platform it never shipped for.
pub fn offered_version(
    local: &str,
    m: &Manifest,
    platform: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(newer) = diff(local, m)? else {
        return Ok(None);
    };
    match platform {
        Some(key) if m.platforms.contains_key(key) => Ok(Some(newer)),
        _ => Ok(None),
    }
}

#[cfg(test)]
#[path = "manifest/tests.rs"]
mod tests;

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

pub async fn fetch(url: &str) -> Result<Manifest, String> {
    let lower = url.to_ascii_lowercase();

    if lower.starts_with("file://") {
        if !cfg!(feature = "dev-updater-allowlocal") {
            return Err(format!(
                "file:// manifest URLs are disabled in this build (rebuild with --features dev-updater-allowlocal): {}",
                url
            ));
        }
        return load_file_manifest(url);
    }

    if lower.starts_with("http://") {
        if !is_loopback_http(url) {
            return Err(format!(
                "http:// manifest URLs are rejected (must be https or 127.0.0.1): {}",
                url
            ));
        }
        if !cfg!(feature = "dev-updater-allowlocal") {
            return Err(format!(
                "loopback http:// manifest URLs are disabled in this build (rebuild with --features dev-updater-allowlocal): {}",
                url
            ));
        }
        return fetch_http(url).await;
    }

    if lower.starts_with("https://") {
        return fetch_http(url).await;
    }

    Err(format!("unsupported manifest URL scheme: {}", url))
}

fn is_loopback_http(url: &str) -> bool {
    let stripped = match url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("HTTP://"))
    {
        Some(s) => s,
        None => return false,
    };
    let host = stripped.split(['/', '?', '#']).next().unwrap_or("");
    let host_only = host.split(':').next().unwrap_or("");
    host_only == "127.0.0.1" || host_only == "localhost"
}

#[cfg(feature = "dev-updater-allowlocal")]
fn load_file_manifest(url: &str) -> Result<Manifest, String> {
    let path_str = url
        .strip_prefix("file://")
        .or_else(|| url.strip_prefix("FILE://"))
        .ok_or_else(|| format!("invalid file:// URL: {}", url))?;
    let path_str = path_str.trim_start_matches('/');
    let body = std::fs::read_to_string(path_str)
        .map_err(|e| format!("failed to read manifest at {}: {}", path_str, e))?;
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
mod tests {
    use super::*;

    fn example_manifest_json() -> &'static str {
        r#"{
            "version": "1.0.2",
            "notes": "Markdown-formatted release notes",
            "pub_date": "2026-04-25T12:34:56Z",
            "platforms": {
                "windows-x86_64": { "url": "https://example.com/codeterm-1.0.2-x64.msi.zip", "signature": "abc==" },
                "darwin-aarch64": { "url": "https://example.com/codeterm-1.0.2-aarch64.app.tar.gz", "signature": "def==" },
                "darwin-x86_64":  { "url": "https://example.com/codeterm-1.0.2-x64.app.tar.gz", "signature": "ghi==" }
            }
        }"#
    }

    #[test]
    fn parses_example_manifest() {
        let m: Manifest = serde_json::from_str(example_manifest_json()).unwrap();
        assert_eq!(m.version, "1.0.2");
        assert_eq!(m.notes.as_deref(), Some("Markdown-formatted release notes"));
        assert_eq!(m.pub_date.as_deref(), Some("2026-04-25T12:34:56Z"));
        assert_eq!(m.platforms.len(), 3);
        assert_eq!(
            m.platforms["windows-x86_64"].url,
            "https://example.com/codeterm-1.0.2-x64.msi.zip"
        );
        assert_eq!(m.platforms["windows-x86_64"].signature, "abc==");
    }

    #[test]
    fn parses_manifest_with_minimal_fields() {
        let json = r#"{ "version": "0.1.0" }"#;
        let m: Manifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.version, "0.1.0");
        assert!(m.notes.is_none());
        assert!(m.pub_date.is_none());
        assert!(m.platforms.is_empty());
    }

    #[test]
    fn current_platform_key_matches_target() {
        let key = current_platform_key();
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            assert_eq!(key, Some("windows-x86_64"));
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(key, Some("darwin-aarch64"));
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            assert_eq!(key, Some("darwin-x86_64"));
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(key, Some("linux-x86_64"));
        } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            assert_eq!(key, Some("linux-aarch64"));
        } else {
            assert_eq!(key, None);
        }
    }

    #[test]
    fn linux_arches_map_to_updater_platform_keys() {
        assert_eq!(platform_key("linux", "x86_64", false), Some("linux-x86_64"));
        assert_eq!(
            platform_key("linux", "aarch64", false),
            Some("linux-aarch64")
        );
        assert_eq!(platform_key("linux", "arm", false), None);
    }

    #[test]
    fn apple_silicon_maps_to_the_arm_updater_platform() {
        assert_eq!(
            platform_key("macos", "aarch64", false),
            Some("darwin-aarch64")
        );
        assert_eq!(
            platform_key("macos", "x86_64", true),
            Some("darwin-aarch64")
        );
        assert_eq!(
            platform_key("macos", "x86_64", false),
            Some("darwin-x86_64")
        );
    }

    fn manifest_with_version(v: &str) -> Manifest {
        Manifest {
            version: v.to_string(),
            notes: None,
            pub_date: None,
            platforms: HashMap::new(),
        }
    }

    #[test]
    fn diff_newer_returns_version() {
        let m = manifest_with_version("1.2.0");
        assert_eq!(diff("1.0.0", &m).unwrap(), Some("1.2.0".to_string()));
    }

    #[test]
    fn diff_equal_returns_none() {
        let m = manifest_with_version("1.0.0");
        assert_eq!(diff("1.0.0", &m).unwrap(), None);
    }

    #[test]
    fn diff_older_returns_none() {
        let m = manifest_with_version("0.9.0");
        assert_eq!(diff("1.0.0", &m).unwrap(), None);
    }

    #[test]
    fn diff_invalid_version_errors() {
        let m = manifest_with_version("not-a-version");
        assert!(diff("1.0.0", &m).is_err());
        let m = manifest_with_version("1.0.0");
        assert!(diff("not-a-version", &m).is_err());
    }

    fn manifest_for_platforms(v: &str, keys: &[&str]) -> Manifest {
        let mut platforms = HashMap::new();
        for key in keys {
            platforms.insert(
                key.to_string(),
                ManifestPlatform {
                    url: format!("https://example.com/{key}"),
                    signature: "sig".to_string(),
                },
            );
        }
        Manifest {
            version: v.to_string(),
            notes: None,
            pub_date: None,
            platforms,
        }
    }

    // rel#47 (retitled): a client is only ever offered an artifact that exists
    // for its own platform, even when a newer version genuinely exists.
    #[test]
    fn windows_only_manifest_offers_nothing_to_macos_or_linux() {
        let m = manifest_for_platforms("1.2.0", &["windows-x86_64"]);
        assert_eq!(
            offered_version("1.0.0", &m, Some("darwin-aarch64")).unwrap(),
            None
        );
        assert_eq!(
            offered_version("1.0.0", &m, Some("darwin-x86_64")).unwrap(),
            None
        );
        assert_eq!(
            offered_version("1.0.0", &m, Some("linux-x86_64")).unwrap(),
            None
        );
    }

    #[test]
    fn windows_only_manifest_still_offers_windows_the_update() {
        let m = manifest_for_platforms("1.2.0", &["windows-x86_64"]);
        assert_eq!(
            offered_version("1.0.0", &m, Some("windows-x86_64")).unwrap(),
            Some("1.2.0".to_string())
        );
    }

    #[test]
    fn complete_matrix_offers_every_platform_the_update() {
        let m = manifest_for_platforms(
            "1.2.0",
            &[
                "windows-x86_64",
                "darwin-aarch64",
                "darwin-x86_64",
                "linux-x86_64",
            ],
        );
        for key in [
            "windows-x86_64",
            "darwin-aarch64",
            "darwin-x86_64",
            "linux-x86_64",
        ] {
            assert_eq!(
                offered_version("1.0.0", &m, Some(key)).unwrap(),
                Some("1.2.0".to_string()),
                "platform {key} should be offered the update"
            );
        }
    }

    #[test]
    fn no_newer_version_offers_nothing_regardless_of_platform_coverage() {
        let m = manifest_for_platforms("1.0.0", &["windows-x86_64", "darwin-aarch64"]);
        assert_eq!(
            offered_version("1.0.0", &m, Some("windows-x86_64")).unwrap(),
            None
        );
    }

    #[test]
    fn unresolved_client_platform_is_never_offered_an_update() {
        let m = manifest_for_platforms("1.2.0", &["windows-x86_64"]);
        assert_eq!(offered_version("1.0.0", &m, None).unwrap(), None);
    }

    #[test]
    fn loopback_detection() {
        assert!(is_loopback_http("http://127.0.0.1:8080/manifest.json"));
        assert!(is_loopback_http("http://localhost/manifest.json"));
        assert!(!is_loopback_http("http://example.com/manifest.json"));
        assert!(!is_loopback_http("https://127.0.0.1/manifest.json"));
    }

    #[tokio::test]
    async fn rejects_non_loopback_http() {
        let err = fetch("http://example.com/latest.json").await.unwrap_err();
        assert!(err.contains("rejected"), "unexpected error: {}", err);
    }

    #[tokio::test]
    async fn rejects_unknown_scheme() {
        let err = fetch("ftp://example.com/latest.json").await.unwrap_err();
        assert!(err.contains("unsupported"), "unexpected error: {}", err);
    }

    #[cfg(not(feature = "dev-updater-allowlocal"))]
    #[tokio::test]
    async fn file_url_disabled_without_feature() {
        let err = fetch("file:///tmp/x.json").await.unwrap_err();
        assert!(err.contains("disabled"), "unexpected error: {}", err);
    }

    #[cfg(feature = "dev-updater-allowlocal")]
    #[tokio::test]
    async fn file_url_loads_when_feature_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        std::fs::write(&path, example_manifest_json()).unwrap();
        let url = format!("file://{}", path.to_string_lossy().replace('\\', "/"));
        let m = fetch(&url).await.expect("manifest should load");
        assert_eq!(m.version, "1.0.2");
    }
}

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

#[tokio::test]
async fn rejects_non_loopback_http() {
    let err = fetch("http://example.com/latest.json").await.unwrap_err();
    assert!(err.contains("require HTTPS"), "unexpected error: {}", err);
}

#[tokio::test]
async fn rejects_unknown_scheme() {
    let err = fetch("ftp://example.com/latest.json").await.unwrap_err();
    assert!(err.contains("require HTTPS"), "unexpected error: {}", err);
}

#[cfg(not(feature = "dev-updater-allowlocal"))]
#[tokio::test]
async fn file_url_disabled_without_feature() {
    let err = fetch("file:///tmp/x.json").await.unwrap_err();
    assert!(err.contains("require HTTPS"), "unexpected error: {}", err);
}

#[cfg(feature = "dev-updater-allowlocal")]
#[tokio::test]
async fn file_url_loads_when_feature_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    std::fs::write(&path, example_manifest_json()).unwrap();
    let url = reqwest::Url::from_file_path(&path).unwrap().to_string();
    let m = fetch(&url).await.expect("manifest should load");
    assert_eq!(m.version, "1.0.2");
}

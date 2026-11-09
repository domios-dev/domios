use super::*;

#[test]
fn provider_version_empty_and_unresolved_commands_are_never_probed() {
    for binary in ["", " ", "\t", "domios-intentionally-absent-version-probe"] {
        assert_eq!(
            observe(binary),
            VersionObservation::unknown(ProbeStatus::Missing)
        );
    }
    assert_eq!(
        probe(Path::new("")),
        VersionObservation::unknown(ProbeStatus::Missing)
    );
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        probe(root.path()),
        VersionObservation::unknown(ProbeStatus::Missing)
    );
    assert_eq!(
        probe(&root.path().join("absent")),
        VersionObservation::unknown(ProbeStatus::Missing)
    );
    assert!(crate::provider_command::command(Path::new("")).is_err());
}

fn identity(path: &str, revision: u64) -> BinaryIdentity {
    BinaryIdentity {
        path: path.into(),
        len: 42,
        modified: SystemTime::UNIX_EPOCH + Duration::from_secs(revision),
        created: None,
        #[cfg(unix)]
        inode: (1, revision, 0, 0),
    }
}

fn detected() -> VersionObservation {
    VersionObservation {
        version: Some("1.2.3".into()),
        status: ProbeStatus::Detected,
    }
}

#[test]
fn provider_compat_cache_deduplicates_inflight_and_completed_binary_identity() {
    let now = Instant::now();
    let mut cache = VersionCache::default();
    let first = identity("/binary-a", 1);
    assert_eq!(
        cache.reserve(&first, now),
        (VersionObservation::unknown(ProbeStatus::Pending), true)
    );
    assert!(!cache.reserve(&first, now + RETRY_BACKOFF).1);
    cache.settle(first.clone(), detected(), now);
    assert_eq!(cache.reserve(&first, now), (detected(), false));
    assert!(cache.reserve(&identity("/binary-b", 1), now).1);
    assert!(cache.reserve(&identity("/binary-a", 2), now).1);
    assert!(!cache.entries.contains_key(&first));
}

#[test]
fn provider_compat_cache_is_bounded_and_failed_probes_stay_unknown() {
    let now = Instant::now();
    let mut cache = VersionCache::default();
    for index in 0..MAX_CACHE_ENTRIES {
        assert!(
            cache
                .reserve(&identity(&format!("/binary-{index}"), 1), now)
                .1
        );
    }
    assert_eq!(
        cache.reserve(&identity("/overflow", 1), now),
        (VersionObservation::unknown(ProbeStatus::Saturated), false)
    );
    let first = identity("/binary-0", 1);
    let failed = VersionObservation::unknown(ProbeStatus::Timeout);
    cache.settle(first.clone(), failed.clone(), now);
    assert_eq!(cache.reserve(&first, now), (failed, false));
}

#[test]
fn provider_compat_failed_probe_is_retried_after_backoff_while_detected_is_kept() {
    let now = Instant::now();
    let mut cache = VersionCache::default();
    let binary = identity("/binary-shim", 1);
    for status in [
        ProbeStatus::Timeout,
        ProbeStatus::Failed,
        ProbeStatus::Invalid,
    ] {
        let failed = VersionObservation::unknown(status);
        cache.settle(binary.clone(), failed.clone(), now);
        assert_eq!(
            cache.reserve(&binary, now + RETRY_BACKOFF - Duration::from_millis(1)),
            (failed, false),
            "{status:?} must not re-probe before the backoff"
        );
        assert_eq!(
            cache.reserve(&binary, now + RETRY_BACKOFF),
            (VersionObservation::unknown(ProbeStatus::Pending), true),
            "{status:?} must re-probe after the backoff"
        );
        assert!(!cache.reserve(&binary, now + RETRY_BACKOFF * 2).1);
    }
    cache.settle(binary.clone(), detected(), now);
    assert_eq!(
        cache.reserve(&binary, now + RETRY_BACKOFF * 100),
        (detected(), false)
    );
}

#[test]
fn provider_compat_version_output_preserves_prerelease_and_rejects_ambiguity() {
    for (output, expected) in [
        ("claude 2.1.292 (Claude Code)", Some("2.1.292")),
        ("codex-cli 0.160.0\n", Some("0.160.0")),
        ("v2.0.0-beta.1", Some("2.0.0-beta.1")),
        ("1.2.3 4.5.6", None),
        ("unknown", None),
        ("1.2", None),
    ] {
        assert_eq!(parse_version(output).as_deref(), expected);
    }
}

#[test]
fn provider_compat_cli_status_serializes_version_verdict_and_reasons() {
    let mut status = crate::provider_cli::status_from_probe(
        "fixture".into(),
        crate::js_runtime::BinProbe {
            resolution: crate::js_runtime::BinResolution::Found("/fixture".into()),
            searched: Vec::new(),
        },
        None,
        None,
    );
    status.detected_version = Some("1.2.3".into());
    status.version_probe = ProbeStatus::Detected;
    status.compatibility = crate::provider_compat::CompatibilityManifest {
        supported: vec!["=1.2.3".into()],
        ..Default::default()
    }
    .evaluate(status.detected_version.as_deref());
    let json = serde_json::to_value(status).unwrap();
    assert_eq!(json["detected_version"], "1.2.3");
    assert_eq!(json["version_probe"], "detected");
    assert_eq!(json["compatibility"]["verdict"], "supported");
    assert_eq!(json["compatibility"]["reasons"], serde_json::json!([]));
}

#[cfg(unix)]
#[test]
fn provider_compat_env_wrapper_version_probe_survives_minimal_gui_path() {
    let root = tempfile::tempdir().unwrap();
    let wrapper = root.path().join("cli-wrapper");
    std::fs::write(
        &wrapper,
        "#!/usr/bin/env sh\nprintf 'codex-cli 0.162.1\\n'\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let broken = Command::new(&wrapper)
        .env("PATH", "/nonexistent")
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(broken.status.code(), Some(127));
    let mut resolved = crate::provider_command::command(&wrapper).unwrap();
    resolved.env("PATH", "/nonexistent");
    assert_eq!(
        probe_command(resolved),
        VersionObservation {
            version: Some("0.162.1".into()),
            status: ProbeStatus::Detected
        }
    );
}

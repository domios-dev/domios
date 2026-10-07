use super::*;

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

#[test]
fn provider_compat_cache_deduplicates_inflight_and_completed_binary_identity() {
    let mut cache = VersionCache::default();
    let first = identity("/binary-a", 1);
    assert_eq!(
        cache.reserve(&first),
        (VersionObservation::unknown(ProbeStatus::Pending), true)
    );
    assert!(!cache.reserve(&first).1);
    let detected = VersionObservation {
        version: Some("1.2.3".into()),
        status: ProbeStatus::Detected,
    };
    cache.entries.insert(first.clone(), detected.clone());
    assert_eq!(cache.reserve(&first), (detected, false));
    assert!(cache.reserve(&identity("/binary-b", 1)).1);
    assert!(cache.reserve(&identity("/binary-a", 2)).1);
    assert!(!cache.entries.contains_key(&first));
}

#[test]
fn provider_compat_cache_is_bounded_and_failed_probes_stay_unknown() {
    let mut cache = VersionCache::default();
    for index in 0..MAX_CACHE_ENTRIES {
        assert!(cache.reserve(&identity(&format!("/binary-{index}"), 1)).1);
    }
    assert_eq!(
        cache.reserve(&identity("/overflow", 1)),
        (VersionObservation::unknown(ProbeStatus::Saturated), false)
    );
    let first = identity("/binary-0", 1);
    let failed = VersionObservation::unknown(ProbeStatus::Timeout);
    cache.entries.insert(first.clone(), failed.clone());
    assert_eq!(cache.reserve(&first), (failed, false));
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

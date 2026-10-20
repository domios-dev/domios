use super::*;

fn policy() -> CompatibilityManifest {
    CompatibilityManifest {
        supported: vec![">=1.2.3, <2.0.0".into()],
        breaking: vec![BreakingRange {
            range: ">=2.0.0".into(),
            reasons: vec![
                CompatibilityReason::SqliteSchemaConversion,
                CompatibilityReason::EventRouteChanged,
                CompatibilityReason::ServerPasswordRequired,
            ],
        }],
        direct_sqlite: true,
        ..Default::default()
    }
}

#[test]
fn provider_compat_ranges_have_closed_lower_and_open_upper_bounds() {
    for (version, verdict) in [
        ("1.2.2", CompatibilityVerdict::Unknown),
        ("1.2.3", CompatibilityVerdict::Supported),
        ("1.9.9", CompatibilityVerdict::Supported),
        ("2.0.0", CompatibilityVerdict::Breaking),
    ] {
        assert_eq!(policy().evaluate(Some(version)).verdict, verdict);
    }
}

#[test]
fn provider_compat_breaking_precedes_support_and_includes_prereleases() {
    let mut policy = policy();
    policy.supported = vec![">=1.0.0".into()];
    for version in ["2.0.0", "2.0.0-beta.1", "3.0.0"] {
        let evaluation = policy.evaluate(Some(version));
        assert_eq!(evaluation.verdict, CompatibilityVerdict::Breaking);
        assert_eq!(
            evaluation.reasons,
            vec![
                CompatibilityReason::SqliteSchemaConversion,
                CompatibilityReason::EventRouteChanged,
                CompatibilityReason::ServerPasswordRequired
            ]
        );
    }
}

#[test]
fn provider_compat_unknown_and_invalid_never_become_supported() {
    for version in [None, Some(""), Some("bad"), Some("1.3.0-beta.1")] {
        assert_eq!(
            policy().evaluate(version).verdict,
            CompatibilityVerdict::Unknown
        );
    }
    assert_eq!(
        evaluate(None, Some("1.2.3")).verdict,
        CompatibilityVerdict::Unknown
    );
    assert_eq!(
        CompatibilityManifest::default()
            .evaluate(Some("1.2.3"))
            .verdict,
        CompatibilityVerdict::Unknown
    );
    let mut invalid = policy();
    invalid.supported.push("invalid".into());
    assert_eq!(
        invalid.evaluate(Some("1.2.3")).reasons,
        vec![CompatibilityReason::InvalidManifest]
    );
    invalid.supported.clear();
    invalid.breaking[0].reasons.clear();
    assert!(invalid.validate().is_err());
}

#[test]
fn provider_compat_sqlite_best_effort_requires_a_detected_undeclared_version() {
    for (version, allowed) in [
        (None, false),
        (Some("bad"), false),
        (Some("1.2.2"), true),
        (Some("1.2.3"), true),
        (Some("2.0.0"), false),
    ] {
        assert_eq!(policy().evaluate(version).allows_sqlite_read(), allowed);
    }
    assert!(!evaluate(None, Some("1.2.3")).allows_sqlite_read());
    let mut invalid = policy();
    invalid.supported.push("invalid".into());
    assert!(!invalid.evaluate(Some("1.2.3")).allows_sqlite_read());
}

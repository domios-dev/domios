use codeterm_plugin::provider_compat::{CompatibilityManifest, CompatibilityVerdict};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum UpdateDecision {
    Ready,
    Deferred,
    Disabled,
    Incompatible,
    UnboundedUpdater,
}

pub(super) fn may_update_now(
    enabled: bool,
    provider_busy: bool,
    compatibility: Option<&CompatibilityManifest>,
    target: &str,
    updater: &str,
) -> UpdateDecision {
    if !enabled {
        return UpdateDecision::Disabled;
    }
    if let Some(manifest) = compatibility {
        if manifest.validate().is_err()
            || manifest.evaluate(Some(target)).verdict == CompatibilityVerdict::Breaking
        {
            return UpdateDecision::Incompatible;
        }
        if !manifest.breaking.is_empty() {
            if !updater.contains("{version}") {
                return UpdateDecision::UnboundedUpdater;
            }
            if manifest.evaluate(Some(target)).verdict != CompatibilityVerdict::Supported {
                return UpdateDecision::Incompatible;
            }
        }
    }
    if provider_busy {
        UpdateDecision::Deferred
    } else {
        UpdateDecision::Ready
    }
}

pub(crate) fn command_for_target(updater: &str, target: &str) -> Result<String, String> {
    let version = semver::Version::parse(target)
        .map_err(|error| format!("invalid CLI update target: {error}"))?;
    if version.to_string() != target || !version.build.is_empty() {
        return Err("CLI update target is not a canonical executable argument".into());
    }
    Ok(updater.replace("{version}", target))
}

pub(crate) fn compatible_command_for_target(
    updater: &str,
    target: &str,
    compatibility: Option<&CompatibilityManifest>,
) -> Result<String, String> {
    let decision = may_update_now(true, false, compatibility, target, updater);
    if decision != UpdateDecision::Ready {
        return Err(format!("CLI update target was withheld: {decision:?}"));
    }
    command_for_target(updater, target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_plugin::provider_compat::{BreakingRange, CompatibilityReason};

    #[test]
    fn provider_cli_update_command_pins_the_declared_target_and_rejects_shell_input() {
        assert_eq!(
            command_for_target("opaque upgrade {version}", "1.9.4").unwrap(),
            "opaque upgrade 1.9.4"
        );
        assert_eq!(
            command_for_target("opaque update", "1.9.4").unwrap(),
            "opaque update"
        );
        for invalid in ["latest", "1.9.4;evil", "1.9.4 extra", "1.9.4+metadata"] {
            assert!(command_for_target("opaque {version}", invalid).is_err());
        }
    }

    #[test]
    fn provider_cli_update_policy_covers_busy_idle_incompatible_and_opted_out() {
        let supported = CompatibilityManifest {
            supported: vec![">=1.0.0".into()],
            ..Default::default()
        };
        let breaking = CompatibilityManifest {
            breaking: vec![BreakingRange {
                range: ">=2.0.0".into(),
                reasons: vec![CompatibilityReason::EventRouteChanged],
            }],
            supported: vec![">=1.0.0, <2.0.0".into()],
            ..Default::default()
        };
        for (enabled, busy, manifest, target, expected) in [
            (
                true,
                false,
                Some(&supported),
                "1.2.0",
                UpdateDecision::Ready,
            ),
            (
                true,
                true,
                Some(&supported),
                "1.2.0",
                UpdateDecision::Deferred,
            ),
            (
                true,
                false,
                Some(&breaking),
                "2.0.0",
                UpdateDecision::Incompatible,
            ),
            (
                true,
                true,
                Some(&breaking),
                "2.0.0",
                UpdateDecision::Incompatible,
            ),
            (
                false,
                false,
                Some(&supported),
                "1.2.0",
                UpdateDecision::Disabled,
            ),
            (
                false,
                true,
                Some(&supported),
                "1.2.0",
                UpdateDecision::Disabled,
            ),
            (true, false, None, "1.2.0", UpdateDecision::Ready),
            (
                true,
                false,
                Some(&supported),
                "0.5.0",
                UpdateDecision::Ready,
            ),
            (
                true,
                false,
                Some(&breaking),
                "1.5.0",
                UpdateDecision::UnboundedUpdater,
            ),
        ] {
            assert_eq!(
                may_update_now(enabled, busy, manifest, target, "opaque update"),
                expected
            );
        }
        assert_eq!(
            may_update_now(true, false, Some(&breaking), "1.5.0", "opaque {version}"),
            UpdateDecision::Ready
        );
        assert_eq!(
            may_update_now(true, false, Some(&breaking), "3.0.0", "opaque {version}"),
            UpdateDecision::Incompatible
        );
        assert!(
            compatible_command_for_target("opaque {version}", "2.0.0", Some(&breaking)).is_err()
        );
        assert_eq!(
            compatible_command_for_target("opaque {version}", "1.5.0", Some(&breaking)).unwrap(),
            "opaque 1.5.0"
        );
        let invalid = CompatibilityManifest {
            supported: vec!["invalid".into()],
            ..Default::default()
        };
        assert_eq!(
            may_update_now(true, false, Some(&invalid), "1.2.0", "opaque update"),
            UpdateDecision::Incompatible
        );
    }
}

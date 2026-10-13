use super::*;

fn decision(
    local: Option<&str>,
    remote: Option<&str>,
    payload: Option<&str>,
    compatible: bool,
) -> UpdateAvailability {
    update_availability(
        local,
        UpdateSource::Channel("store"),
        Some(UpdateOffer {
            channel: "store",
            version: remote,
            payload_version: payload,
            compatible,
        }),
    )
}

#[test]
fn version_precedence_requires_a_newer_installable_release() {
    for (local, remote, expected) in [
        ("0.1.0", "0.1.0", "same_version"),
        ("0.1.0", "0.2.0", "newer_version"),
        ("0.2.0", "0.1.0", "local_ahead"),
        ("1.0.0-beta.2", "1.0.0-beta.10", "newer_version"),
        ("1.0.0-rc.1", "1.0.0", "newer_version"),
        ("1.0.0", "1.0.0-rc.1", "local_ahead"),
        ("1.0.0+old", "1.0.0+new", "same_version"),
    ] {
        let actual = decision(Some(local), Some(remote), Some(remote), true);
        assert_eq!(actual.reason, expected);
        assert_eq!(actual.available, expected == "newer_version");
    }
}

#[test]
fn provenance_rejects_local_unknown_and_other_channels() {
    for (source, channel, expected) in [
        (UpdateSource::Local, "store", "local_install"),
        (UpdateSource::Unknown, "store", "unknown_source"),
        (
            UpdateSource::Channel("private"),
            "store",
            "different_channel",
        ),
    ] {
        let actual = update_availability(
            Some("0.1.0"),
            source,
            Some(UpdateOffer {
                channel,
                version: Some("1.0.0"),
                payload_version: Some("1.0.0"),
                compatible: true,
            }),
        );
        assert!(!actual.available);
        assert_eq!(actual.reason, expected);
    }
    assert_eq!(
        update_availability(Some("0.1.0"), UpdateSource::Channel("missing"), None).reason,
        "channel_unavailable"
    );
}

#[test]
fn malformed_missing_mismatched_and_incompatible_offers_are_not_updates() {
    for (local, remote, payload, compatible, expected) in [
        (
            Some("0.1.0"),
            Some("bad"),
            Some("bad"),
            true,
            "invalid_version",
        ),
        (None, Some("1.0.0"), Some("1.0.0"), true, "missing_version"),
        (Some("0.1.0"), None, None, true, "missing_version"),
        (Some("0.1.0"), Some("1.0.0"), None, true, "missing_version"),
        (
            Some("0.1.0"),
            Some("1.0.0"),
            Some("0.1.0"),
            true,
            "channel_manifest_mismatch",
        ),
        (
            Some("0.1.0"),
            Some("1.0.0"),
            Some("1.0.0"),
            false,
            "incompatible",
        ),
    ] {
        let actual = decision(local, remote, payload, compatible);
        assert!(!actual.available);
        assert_eq!(actual.reason, expected);
    }
}

#[test]
fn unified_release_states_compare_precedence_and_compatibility() {
    for (installed, latest, compatible, expected) in [
        ("0.1.2", "0.1.3", true, AvailabilityState::UpdateAvailable),
        ("0.1.3", "0.1.3", true, AvailabilityState::UpToDate),
        ("0.1.4", "0.1.3", true, AvailabilityState::UpToDate),
        ("0.1.2", "0.1.3", false, AvailabilityState::Incompatible),
        (
            "1.0.0+local",
            "1.0.0+remote",
            true,
            AvailabilityState::UpToDate,
        ),
    ] {
        for kind in [UpdateKind::Plugin, UpdateKind::App, UpdateKind::Frontend] {
            let item = AvailabilityItem::release("fixture", kind, installed, latest, compatible);
            assert_eq!(item.state, expected);
        }
    }
}

#[test]
fn core_plugins_never_offer_a_user_action_even_when_newer() {
    for latest in ["0.1.2", "0.1.3"] {
        let decision = decision(Some("0.1.2"), Some(latest), Some(latest), true);
        let item = AvailabilityItem::plugin(
            "core",
            Some("0.1.2"),
            Some(latest),
            Some("official"),
            true,
            true,
            &decision,
        );
        assert_eq!(item.state, AvailabilityState::CoreSilent);
    }
}

use super::{availability, offer_availability, rendered, settle, staged, Offer, Rendered};
use crate::{activation::Activation, manifest::Manifest};
use codeterm_plugins::AvailabilityState;

const API: u32 = 3;

fn release(version: &str, min_api: u32, max_api: u32) -> Manifest {
    Manifest {
        version: version.into(),
        min_api_version: min_api,
        max_api_version: max_api,
        sha256: "0".repeat(64),
        url: format!("https://example.test/frontend-v{version}.tar.gz"),
        release_id: format!("frontend-v{version}"),
    }
}

fn app(version: &str) -> semver::Version {
    semver::Version::parse(version).unwrap()
}

fn embedded(version: &str) -> Rendered {
    Rendered {
        version: version.into(),
        release_id: None,
    }
}

#[test]
fn app_upgrade_retires_staged_releases_older_than_the_app_and_renders_its_own_frontend() {
    let mut state = Activation {
        active: Some(release("1.14.6", 3, 3)),
        previous: Some(release("1.14.5", 2, 2)),
        pending: Some(release("1.14.7", 3, 3)),
        ..Activation::default()
    };
    settle(&mut state, &app("1.14.9"), API);
    assert_eq!(state.active, None);
    assert_eq!(state.previous, None);
    assert_eq!(state.pending, None);
    assert_eq!(rendered(&state, &app("1.14.9")), embedded("1.14.9"));
}

#[test]
fn a_release_equal_to_the_app_yields_to_the_embedded_frontend() {
    let mut state = Activation {
        active: Some(release("1.14.9", 3, 3)),
        ..Activation::default()
    };
    settle(&mut state, &app("1.14.9"), API);
    assert_eq!(rendered(&state, &app("1.14.9")), embedded("1.14.9"));
}

#[test]
fn a_newer_compatible_release_stays_rendered_across_the_upgrade() {
    let mut state = Activation {
        active: Some(release("1.15.1", 3, 3)),
        ..Activation::default()
    };
    settle(&mut state, &app("1.14.9"), API);
    assert_eq!(
        rendered(&state, &app("1.14.9")),
        Rendered {
            version: "1.15.1".into(),
            release_id: Some("frontend-v1.15.1".into()),
        }
    );
}

#[test]
fn a_newer_release_incompatible_with_the_app_is_retired() {
    let mut state = Activation {
        active: Some(release("1.16.0", 4, 4)),
        previous: Some(release("1.15.2", 3, 3)),
        pending: Some(release("1.16.1", 2, 2)),
        ..Activation::default()
    };
    settle(&mut state, &app("1.14.9"), API);
    assert_eq!(state.pending, None);
    assert_eq!(
        rendered(&state, &app("1.14.9")).release_id.as_deref(),
        Some("frontend-v1.15.2")
    );
    let mut state = Activation {
        active: Some(release("1.16.0", 4, 4)),
        previous: Some(release("1.14.8", 3, 3)),
        ..Activation::default()
    };
    settle(&mut state, &app("1.14.9"), API);
    assert_eq!(rendered(&state, &app("1.14.9")), embedded("1.14.9"));
}

#[test]
fn an_older_or_incompatible_channel_release_is_never_offered() {
    for (version, compatible, reason) in [
        ("1.14.5", false, "incompatible"),
        ("1.14.5", true, "local_ahead"),
        ("1.14.9", true, "same_version"),
        ("1.15.0", false, "incompatible"),
    ] {
        let item = offer_availability(
            "1.14.9",
            Some(Offer {
                version,
                compatible,
            }),
        );
        assert_eq!(item.state, AvailabilityState::UpToDate, "{version}");
        assert_eq!(item.installed.as_deref(), Some("1.14.9"));
        assert_eq!(item.latest.as_deref(), Some("1.14.9"), "{version}");
        assert!(item.compatible, "{version}");
        assert_eq!(item.reason, reason, "{version}");
    }
    let item = offer_availability("1.14.9", None);
    assert_eq!(item.state, AvailabilityState::UpToDate);
    assert_eq!(item.latest.as_deref(), Some("1.14.9"));
}

#[test]
fn a_newer_compatible_channel_release_is_offered() {
    let item = offer_availability(
        "1.14.9",
        Some(Offer {
            version: "1.15.0",
            compatible: true,
        }),
    );
    assert_eq!(item.state, AvailabilityState::UpdateAvailable);
    assert_eq!(item.latest.as_deref(), Some("1.15.0"));
}

#[test]
fn only_a_newer_compatible_staged_release_is_ready_to_apply() {
    let newer = release("1.15.0", 3, 3);
    let item = availability("1.14.9", Some(&newer), None, API);
    assert_eq!(item.state, AvailabilityState::ReadyToApply);
    assert_eq!(item.latest.as_deref(), Some("1.15.0"));
    for staged in [release("1.14.6", 3, 3), release("1.15.0", 2, 2)] {
        let item = availability("1.14.9", Some(&staged), None, API);
        assert_eq!(item.state, AvailabilityState::UpToDate);
        assert_eq!(item.latest.as_deref(), Some("1.14.9"));
    }
}

#[test]
fn a_loading_or_quarantined_release_is_not_staged() {
    let mut state = Activation {
        pending: Some(release("1.15.0", 3, 3)),
        ..Activation::default()
    };
    assert!(staged(&state).is_some());
    state.quarantined.insert("frontend-v1.15.0".into());
    assert!(staged(&state).is_none());
}

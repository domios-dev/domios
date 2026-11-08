use super::*;

#[test]
fn provider_readiness_aggregates_declared_launch_profiles() {
    for (profiles, expected, availability) in [
        (
            vec![Some(false), Some(true), None],
            Some(true),
            ProviderAvailability::Available,
        ),
        (vec![Some(false), None], None, ProviderAvailability::Unknown),
        (
            vec![Some(false)],
            Some(false),
            ProviderAvailability::Unavailable,
        ),
        (vec![], Some(false), ProviderAvailability::Unavailable),
    ] {
        let profiles = profiles
            .into_iter()
            .enumerate()
            .map(|(index, installed)| ProviderLaunchProfileStatus {
                id: format!("profile-{index}"),
                installed,
            })
            .collect();
        let status = status_from_profiles(profiles);
        assert_eq!(status.installed, expected);
        assert_eq!(status.availability, availability);
        assert!(!status.required);
        assert!(status.binary.is_none());
        assert!(status.installer.is_none());
        assert!(status.updater.is_none());
        assert!(status.launch_profiles.is_some());
    }
}

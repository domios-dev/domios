use super::*;

#[tokio::test]
async fn agent_browser_defaults_to_temporary_and_rejects_missing_profiles() {
    let root = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        BrowserHostConfig::under_data_dir(root.path()),
        "Agent",
        "tab-id",
        Arc::new(tokio::sync::OnceCell::new()),
        SurfaceResume::default(),
    );
    assert_eq!(*runtime.profile_request.lock(), "");
    assert!(runtime.use_profile("Invented").await.is_err());
    assert!(!runtime.layout.profiles_dir().exists());
    assert_eq!(*runtime.profile_request.lock(), "");
    assert_eq!(
        runtime.view_only_methods(),
        ["profiles.create", "profiles.delete", "viewport.claim"]
    );
}

#[tokio::test]
async fn restored_browser_reports_and_applies_creation_profile_before_and_at_launch() {
    let root = tempfile::tempdir().unwrap();
    let config = BrowserHostConfig::under_data_dir(root.path());
    config.layout.create_named("Work").unwrap();
    let runtime = BrowserRuntime::new(
        config,
        "Orla",
        "tab-id",
        Arc::new(tokio::sync::OnceCell::new()),
        SurfaceResume {
            url: Some("https://example.test/page".into()),
            profile: Some("Work".into()),
        },
    );
    // Before first launch, `status` must already report the persisted creation profile, not `clean`.
    assert_eq!(runtime.shared.state().profile, "Work");
    assert!(runtime.status_text().contains("profile: Work"));

    runtime.apply_resume_profile().await;
    assert_eq!(*runtime.profile_request.lock(), "Work");
    assert_eq!(runtime.shared.state().profile, "Work");
    assert_eq!(
        runtime
            .shared
            .subscribe_resume()
            .borrow()
            .profile
            .as_deref(),
        Some("Work")
    );
}

#[tokio::test]
async fn restored_browser_falls_back_to_clean_and_updates_state_when_profile_was_deleted() {
    let root = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        BrowserHostConfig::under_data_dir(root.path()),
        "Orla",
        "tab-id",
        Arc::new(tokio::sync::OnceCell::new()),
        SurfaceResume {
            url: None,
            profile: Some("Deleted".into()),
        },
    );
    let mut notices = runtime.shared.subscribe_notices();
    assert_eq!(runtime.shared.state().profile, "Deleted");

    runtime.apply_resume_profile().await;
    assert_eq!(*runtime.profile_request.lock(), "");
    assert_eq!(runtime.shared.state().profile, "clean");
    assert_eq!(runtime.shared.subscribe_resume().borrow().profile, None);
    assert!(notices
        .try_recv()
        .unwrap()
        .text
        .contains("Profile Deleted no longer exists"));
}

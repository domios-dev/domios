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
        ["profiles.create", "profiles.delete"]
    );
}

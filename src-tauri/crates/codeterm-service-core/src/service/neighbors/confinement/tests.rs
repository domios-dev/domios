use super::*;

#[test]
fn bound_launch_matches_effective_sandbox_and_working_folder() {
    assert!(!bound_launch_matches(
        Some("friend"),
        None,
        MachineAccess::ReadOnly,
        "friend"
    ));
    assert!(bound_launch_matches(
        Some("friend"),
        Some(ExecutionIntent::ReadOnly),
        MachineAccess::ReadOnly,
        "friend"
    ));
    assert!(!bound_launch_matches(
        Some("other"),
        Some(ExecutionIntent::ReadOnly),
        MachineAccess::ReadOnly,
        "friend"
    ));
    assert!(!bound_launch_matches(
        Some("friend"),
        Some(ExecutionIntent::ReadOnly),
        MachineAccess::Folder,
        "friend"
    ));
    assert!(bound_launch_matches(
        Some("friend"),
        Some(ExecutionIntent::Mutating),
        MachineAccess::Folder,
        "friend"
    ));
    assert!(bound_launch_matches(
        Some("friend"),
        None,
        MachineAccess::Full,
        "friend"
    ));
    assert!(!bound_launch_matches(
        Some("friend"),
        None,
        MachineAccess::None,
        "friend"
    ));
}

#[tokio::test]
async fn default_friend_folder_is_account_scoped_and_explicit_folder_must_exist() {
    let temp = tempfile::tempdir().unwrap();
    let metadata = Metadata::new(temp.path().to_owned(), "owner");
    let other = Metadata::new(temp.path().to_owned(), "other");
    let friend = "a".repeat(64);
    assert_ne!(
        metadata.friend_folder(&friend).unwrap(),
        other.friend_folder(&friend).unwrap()
    );
    assert!(metadata.friend_folder("../escape").is_err());
    let mut permission = Permission::default();
    let folder = prepare_folder(&metadata, &friend, &permission)
        .await
        .unwrap();
    assert_eq!(
        std::path::Path::new(&folder).canonicalize().unwrap(),
        metadata
            .friend_folder(&friend)
            .unwrap()
            .canonicalize()
            .unwrap()
    );
    permission.folder = Some(temp.path().join("missing").to_string_lossy().into_owned());
    assert!(prepare_folder(&metadata, &friend, &permission)
        .await
        .is_err());
    permission.folder = Some(temp.path().to_string_lossy().into_owned());
    assert_eq!(
        std::path::Path::new(
            &prepare_folder(&metadata, &friend, &permission)
                .await
                .unwrap()
        )
        .canonicalize()
        .unwrap(),
        temp.path().canonicalize().unwrap()
    );
}

fn capability() -> codeterm_plugin::provider::ExecutionSandboxCapability {
    serde_json::from_value(json!({
        "platforms":["windows","linux","macos"],
        "mutating":{"launchArgs":["--sandbox","workspace-write"],"runtimeMode":"workspace-write"},
        "readOnly":{"launchArgs":["--sandbox","read-only"],"runtimeMode":"read-only"}
    }))
    .unwrap()
}

#[test]
fn read_only_machine_access_uses_supported_sandbox_or_honest_advisory_mode() {
    for platform in ["windows", "linux", "macos"] {
        for runtime in [true, false] {
            assert_eq!(
                sandbox_label(&capability(), platform, runtime, MachineAccess::ReadOnly)
                    .unwrap()
                    .as_deref(),
                Some("read-only")
            );
        }
    }
    assert_eq!(
        sandbox_label(
            &Default::default(),
            "windows",
            true,
            MachineAccess::ReadOnly
        )
        .unwrap(),
        None
    );
}

#[test]
fn folder_machine_access_prefers_workspace_then_read_only_then_advisory() {
    let mut capability = capability();
    assert_eq!(
        sandbox_label(&capability, "windows", true, MachineAccess::Folder)
            .unwrap()
            .as_deref(),
        Some("workspace-write")
    );
    capability.mutating = None;
    assert_eq!(
        sandbox_label(&capability, "windows", true, MachineAccess::Folder)
            .unwrap()
            .as_deref(),
        Some("read-only")
    );
    capability.read_only = None;
    assert_eq!(
        sandbox_label(&capability, "windows", true, MachineAccess::Folder).unwrap(),
        None
    );
}

#[test]
fn full_machine_access_is_explicit_and_none_is_not_claimed_without_a_capability() {
    assert!(
        sandbox_label(&Default::default(), "windows", true, MachineAccess::Full)
            .unwrap()
            .is_none()
    );
    assert!(sandbox_label(&capability(), "windows", true, MachineAccess::None).is_err());
}

#[test]
fn permission_projection_reports_actual_sandbox_and_additional_host_roots() {
    let (mut mux, general) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let gateway = mux.create_tab(&workspace).unwrap();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friend = "a".repeat(64);
    let mut settings = Settings::default();
    settings.bindings.insert(friend.clone(), gateway.clone());
    let permission = Permission::default();
    let projected = fixture
        .service
        .neighbor_permission_projection(&metadata, &friend, &permission, &settings)
        .unwrap();
    assert_eq!(projected["allow_memory"], true);
    assert_eq!(projected["allow_files"], true);
    assert_eq!(projected["folder_enforcement"]["writes"], "advisory");
    let cwd = fixture.temp.path().to_string_lossy().into_owned();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&gateway).unwrap();
        tab.cwd = Some(cwd.clone());
        tab.restorable_launch.execution_sandbox =
            Some(codeterm_types::plan::ExecutionSandboxPolicy {
                intent: ExecutionIntent::Mutating,
                cwd: cwd.clone(),
                launch_args: vec![],
                runtime_mode: Some("workspace-write".into()),
                writable_roots: vec!["own-agent-pad".into()],
                environment: Default::default(),
            });
    }
    let projected = fixture
        .service
        .neighbor_permission_projection(&metadata, &friend, &permission, &settings)
        .unwrap();
    assert_eq!(projected["effective_folder"], cwd);
    assert_eq!(
        projected["folder_enforcement"]["writes"],
        "provider_sandbox"
    );
    assert_eq!(projected["folder_enforcement"]["reads"], "not_confined");
    assert!(projected["folder_enforcement"]["note"]
        .as_str()
        .unwrap()
        .contains("own-agent-pad"));
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&gateway)
        .unwrap()
        .restorable_launch
        .execution_sandbox
        .as_mut()
        .unwrap()
        .intent = ExecutionIntent::ReadOnly;
    let projected = fixture
        .service
        .neighbor_permission_projection(&metadata, &friend, &permission, &settings)
        .unwrap();
    assert_eq!(projected["folder_enforcement"]["writes"], "pad_only");
    assert_eq!(projected["folder_enforcement"]["reads"], "not_confined");
}

use super::*;

#[test]
fn concierge_pty_access_matrix_preserves_explicit_full_grants_and_prepares_pads() {
    use crate::{
        multiplexer::{state::MultiplexerState, types::NeighborAuthority},
        service::{test_fixture::ServiceFixture, SpawnAgentParams},
    };
    for access in [
        MachineAccess::ReadOnly,
        MachineAccess::Folder,
        MachineAccess::Full,
    ] {
        for lifecycle in ["fresh", "resumed", "restored"] {
            let (mut mux, tab_id) = MultiplexerState::new();
            mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("opaque-pty".into());
            mux.get_tab_mut(&tab_id).unwrap().agent_session_id = Some("opaque-session".into());
            mux.get_tab_mut(&tab_id).unwrap().session_binding_nonce =
                Some("opaque-root-nonce".into());
            mux.get_tab_mut(&tab_id).unwrap().neighbor_authority = Some(NeighborAuthority {
                uid: "owner".into(),
                friendship_id: "a".repeat(64),
                root_tab_id: None,
            });
            let fixture = ServiceFixture::new(mux);
            fixture
                .service
                .set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
            let svc = &fixture.service;
            fixture.register_neighbor_advisory_provider("opaque-pty");
            let mut params = SpawnAgentParams {
                resume_session_id: (lifecycle != "fresh").then(|| "opaque-session".into()),
                ..Default::default()
            };
            params.neighbor_authority = svc
                .mux
                .read()
                .get_tab(&tab_id)
                .unwrap()
                .neighbor_authority
                .clone();
            svc.configure_neighbor_launch_access("opaque-pty", access, &mut params)
                .unwrap();
            assert_eq!(
                params.permission_bypass_requested("opaque-pty"),
                access == MachineAccess::Full
            );
            for global in [false, true] {
                fixture.set_agent_full_access(global);
                assert_eq!(
                    crate::service::permission_policy::spawn_permission_bypass_for_launch(
                        svc,
                        "opaque-pty",
                        false,
                        false,
                        Some(&params)
                    )
                    .unwrap()
                    .is_some(),
                    access == MachineAccess::Full
                );
                let mut forged = params.clone();
                forged.skip_permissions = Some(true);
                forged.permission_bypass_authorization = None;
                assert!(
                    crate::service::permission_policy::spawn_permission_bypass_for_launch(
                        svc,
                        "opaque-pty",
                        false,
                        false,
                        Some(&forged)
                    )
                    .unwrap()
                    .is_none()
                );
            }
            fixture.set_agent_full_access(false);
            let pad = svc.ensure_agent_pad(&tab_id).unwrap();
            std::fs::write(pad.path().join("reply.txt"), "opaque reply").unwrap();
            let mut recorded = crate::multiplexer::types::RestorableLaunchParameters::default();
            recorded.captured = true;
            recorded.permission_bypass_requested = params.permission_bypass_requested("opaque-pty");
            svc.remember_neighbor_restore_grant(&tab_id, access);
            svc.mux
                .write()
                .get_tab_mut(&tab_id)
                .unwrap()
                .restorable_launch = recorded.clone();
            let snapshot = svc.mux.read().to_session_snapshot();
            *svc.mux.write() = MultiplexerState::from_session_snapshot(&snapshot);
            assert_eq!(
                svc.mux
                    .read()
                    .get_tab(&tab_id)
                    .unwrap()
                    .agent_session_id
                    .as_deref(),
                Some("opaque-session")
            );
            assert_eq!(
                svc.mux
                    .read()
                    .get_tab(&tab_id)
                    .unwrap()
                    .session_binding_nonce
                    .as_deref(),
                Some("opaque-root-nonce")
            );
            let restored =
                crate::service::restored_launch::restore_permission_bypass_authorization(
                    svc,
                    &svc.agent_registry,
                    &tab_id,
                    "opaque-pty",
                    &recorded,
                )
                .unwrap();
            assert_eq!(restored.is_some(), access == MachineAccess::Full);
            assert!(svc
                .ensure_agent_pad(&tab_id)
                .unwrap()
                .path()
                .join("reply.txt")
                .is_file());
        }
    }
}

#[test]
fn full_launch_without_provider_authorization_is_refused_instead_of_prompting() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("opaque-no-bypass"),
    ));
    let mut params = crate::service::SpawnAgentParams::default();
    assert_eq!(
        fixture
            .service
            .configure_neighbor_launch_access("opaque-no-bypass", MachineAccess::Full, &mut params)
            .unwrap_err(),
        codeterm_neighbors::Error::MachineAccessUnsupported.to_string()
    );
}

#[test]
fn legacy_full_concierge_without_bypass_requires_reconciliation() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("opaque-pty".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
    fixture.register_neighbor_advisory_provider("opaque-pty");
    let cwd = fixture.temp.path().to_string_lossy().into_owned();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .cwd = Some(cwd.clone());
    let permission = Permission {
        machine_access: MachineAccess::Full,
        ..Default::default()
    };
    assert!(fixture
        .service
        .validate_neighbor_bound_launch(&tab_id, &permission, &cwd)
        .is_err());
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .restorable_launch
        .permission_bypass_requested = true;
    fixture
        .service
        .validate_neighbor_bound_launch(&tab_id, &permission, &cwd)
        .unwrap();
}

#[test]
fn restored_full_concierge_without_provider_authorization_refuses_before_relaunch() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().neighbor_authority =
        Some(crate::multiplexer::types::NeighborAuthority {
            uid: "owner".into(),
            friendship_id: "a".repeat(64),
            root_tab_id: None,
        });
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("opaque-no-bypass"),
    ));
    let launch = crate::multiplexer::types::RestorableLaunchParameters {
        captured: true,
        permission_bypass_requested: true,
        ..Default::default()
    };
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .session_binding_nonce = Some("opaque-root-nonce".into());
    fixture
        .service
        .remember_neighbor_restore_grant(&tab_id, MachineAccess::Full);
    for global in [false, true] {
        fixture.set_agent_full_access(global);
        let error = crate::service::restored_launch::restore_permission_bypass_authorization(
            &fixture.service,
            &fixture.service.agent_registry,
            &tab_id,
            "opaque-no-bypass",
            &launch,
        )
        .unwrap_err();
        assert_eq!(
            error,
            codeterm_neighbors::Error::MachineAccessUnsupported.to_string()
        );
    }
}

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
    fixture
        .service
        .set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
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

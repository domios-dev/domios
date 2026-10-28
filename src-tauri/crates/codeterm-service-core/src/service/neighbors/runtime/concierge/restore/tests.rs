use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::test]
async fn older_concierge_is_replaced_in_friend_folder_without_losing_permissions_or_model() {
    for tagged in [false, true] {
        let (mut mux, general) = MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&general).unwrap();
        let old = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&old).unwrap();
        tab.agent_type = Some("advisory-provider".into());
        tab.agent_session_model = Some("opaque-model".into());
        tab.restored_from_snapshot = true;
        tab.cwd = Some("old-general-folder".into());
        let fixture = ServiceFixture::new(mux);
        let service = &fixture.service;
        let friend = "a".repeat(64);
        let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
        let permission = Permission {
            allowed: true,
            scope: "opaque scope".into(),
            allow_memory: false,
            ..Default::default()
        };
        let mut settings = crate::service::neighbors::storage::Settings::default();
        settings.bindings.insert(friend.clone(), old.clone());
        settings
            .permissions
            .insert(friend.clone(), permission.clone());
        metadata.save_settings(settings).await.unwrap();
        if tagged {
            service.tag_neighbor_root("owner", &friend, &old).unwrap();
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let invoked = calls.clone();
        service
            .restore_neighbor_concierge_with(
                &metadata,
                "owner",
                &friend,
                |provider, model, cwd, grant, resume, _| async move {
                    assert!(resume.is_none());
                    invoked.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(provider.as_deref(), Some("advisory-provider"));
                    assert_eq!(model.as_deref(), Some("opaque-model"));
                    assert_eq!(
                        grant.machine_access,
                        codeterm_neighbors::permissions::MachineAccess::ReadOnly
                    );
                    let mut mux = service.mux.write();
                    let id = mux.create_tab(&workspace).unwrap();
                    let tab = mux.get_tab_mut(&id).unwrap();
                    tab.agent_type = provider;
                    tab.agent_session_model = model;
                    tab.cwd = Some(cwd);
                    tab.restorable_launch.identity_instructions =
                        Some(initial_prompt::initial_prompt(&"a".repeat(64), &grant));
                    Ok(id)
                },
            )
            .await
            .unwrap();
        let saved = metadata.settings().await.unwrap();
        let new = &saved.bindings[&friend];
        assert_ne!(new, &old);
        assert_eq!(
            serde_json::to_value(&saved.permissions[&friend]).unwrap(),
            serde_json::to_value(&permission).unwrap()
        );
        let projected = service
            .neighbor_permission_projection(&metadata, &friend, &permission, &saved)
            .unwrap();
        assert_eq!(projected["folder_enforcement"]["writes"], "advisory");
        assert_eq!(
            std::path::Path::new(projected["effective_folder"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            metadata
                .friend_folder(&friend)
                .unwrap()
                .canonicalize()
                .unwrap()
        );
        assert!(service.mux.read().get_tab(&old).is_none());
        assert!(service
            .mux
            .read()
            .get_tab(new)
            .unwrap()
            .neighbor_authority
            .is_some());
        service
            .restore_neighbor_concierge_with(
                &metadata,
                "owner",
                &friend,
                |_, _, _, _, _, _| async { Err("matching launch must not respawn".into()) },
            )
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn failed_replacement_keeps_the_old_binding_and_permissions() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let old = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&old).unwrap().agent_type = Some("advisory-provider".into());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let friend = "b".repeat(64);
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings.bindings.insert(friend.clone(), old.clone());
    settings
        .permissions
        .insert(friend.clone(), Permission::default());
    metadata.save_settings(settings.clone()).await.unwrap();
    assert!(service
        .restore_neighbor_concierge_with(&metadata, "owner", &friend, |_, _, _, _, _, _| async {
            Err("injected launch failure".into())
        })
        .await
        .is_err());
    assert_eq!(
        serde_json::to_value(metadata.settings().await.unwrap()).unwrap(),
        serde_json::to_value(settings).unwrap()
    );
    assert!(service.mux.read().get_tab(&old).is_some());
}

#[tokio::test]
async fn startup_retires_only_superseded_roots_into_recently_closed() {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let friend = "a".repeat(64);
    let mut tabs = Vec::new();
    for (uid, friendship, root_tab_id) in [
        ("owner", friend.clone(), None),
        ("owner", friend.clone(), None),
        ("other-owner", friend.clone(), None),
        ("owner", "b".repeat(64), None),
        ("owner", friend.clone(), Some("current-root".to_string())),
    ] {
        let id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("advisory-provider".into());
        tab.neighbor_authority = Some(crate::multiplexer::types::NeighborAuthority {
            uid: uid.into(),
            friendship_id: friendship,
            root_tab_id,
        });
        tabs.push(id);
    }
    mux.get_tab_mut(&tabs[4])
        .unwrap()
        .neighbor_authority
        .as_mut()
        .unwrap()
        .root_tab_id = Some(tabs[1].clone());
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let permission = Permission::default();
    let cwd =
        crate::service::neighbors::confinement::prepare_folder(&metadata, &friend, &permission)
            .await
            .unwrap();
    service.mux.write().get_tab_mut(&tabs[1]).unwrap().cwd = Some(cwd);
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings.bindings.insert(friend.clone(), tabs[1].clone());
    service
        .mux
        .write()
        .get_tab_mut(&tabs[1])
        .unwrap()
        .restorable_launch
        .identity_instructions = Some(initial_prompt::initial_prompt(&friend, &permission));
    settings.initial_contexts.insert(
        friend.clone(),
        crate::service::neighbors::storage::InitialContext {
            gateway_tab: tabs[1].clone(),
            digest: initial_prompt::digest(&friend, &permission),
            launch_options: None,
        },
    );
    settings.permissions.insert(friend.clone(), permission);
    metadata.save_settings(settings).await.unwrap();
    for _ in 0..2 {
        service
            .restore_neighbor_concierge_with(
                &metadata,
                "owner",
                &friend,
                |_, _, _, _, _, _| async { Err("current launch must not be replaced".into()) },
            )
            .await
            .unwrap();
    }
    let mux = service.mux.read();
    assert!(mux.get_tab(&tabs[0]).is_none());
    for tab in &tabs[1..] {
        assert!(mux.get_tab(tab).is_some());
    }
    assert_eq!(
        mux.list_recently_closed(&workspace)
            .iter()
            .filter(|tab| tab.tab_id == tabs[0])
            .count(),
        1
    );
}

#[tokio::test]
async fn fresh_context_pty_restore_waits_for_local_grant_then_materializes_once() {
    use crate::{
        multiplexer::{state::MultiplexerState, types::NeighborAuthority},
        service::{lazy_restore::test_boundary::Boundary, test_fixture::ServiceFixture},
    };
    use codeterm_neighbors::permissions::MachineAccess;
    use codeterm_types::pty::BootStage;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    for access in [
        MachineAccess::ReadOnly,
        MachineAccess::Folder,
        MachineAccess::Full,
    ] {
        let data_dir = tempfile::tempdir().unwrap();
        let (mut mux, general) = MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&general).unwrap();
        let root = mux.create_tab(&workspace).unwrap();
        let friend = "a".repeat(64);
        let tab = mux.get_tab_mut(&root).unwrap();
        tab.agent_type = Some("opaque-pty".into());
        tab.agent_session_id = Some("same-restored-session".into());
        tab.session_binding_nonce = Some("same-nonce".into());
        tab.neighbor_authority = Some(NeighborAuthority {
            uid: "owner".into(),
            friendship_id: friend.clone(),
            root_tab_id: None,
        });
        tab.restorable_launch.captured = true;
        tab.restorable_launch.permission_bypass_requested = access == MachineAccess::Full;
        let original = ServiceFixture::new_at(mux, data_dir.path());
        let permission = Permission {
            machine_access: access,
            folder: (access == MachineAccess::Folder)
                .then(|| data_dir.path().to_string_lossy().into_owned()),
            ..Default::default()
        };
        original
            .save_neighbor_restore_settings("owner", &friend, &root, permission.clone())
            .await;
        let metadata = Metadata::new(original.service.account_data_root().unwrap(), "owner");
        let cwd =
            crate::service::neighbors::confinement::prepare_folder(&metadata, &friend, &permission)
                .await
                .unwrap();
        original.service.mux.write().get_tab_mut(&root).unwrap().cwd = Some(cwd);
        let mut settings = metadata.settings().await.unwrap();
        settings.initial_contexts.insert(
            friend.clone(),
            crate::service::neighbors::storage::InitialContext {
                gateway_tab: root.clone(),
                digest: initial_prompt::digest(&friend, &permission),
                launch_options: None,
            },
        );
        metadata.save_settings(settings).await.unwrap();
        original.load_neighbor_restore_settings("owner").await;
        assert!(original
            .service
            .require_neighbor_restore_grant(&root, access == MachineAccess::Full)
            .is_ok());
        let snapshot =
            serde_json::to_vec(&original.service.mux.read().to_session_snapshot()).unwrap();
        drop(original);
        let snapshot = serde_json::from_slice(&snapshot).unwrap();
        let restored = ServiceFixture::new_at(
            MultiplexerState::from_session_snapshot(&snapshot),
            data_dir.path(),
        );
        let service = &restored.service;
        service
            .ext::<crate::config::ConfigState>()
            .lock()
            .neighbors
            .enabled = true;
        restored.register_neighbor_advisory_provider("opaque-pty");
        service.mux.write().get_tab_mut(&root).unwrap().frozen = true;
        assert!(service.neighbor_cell().restore_grants.lock().is_empty());
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let target = root.clone();
        let copy = service.clone();
        let boundary = Boundary::install(
            &root,
            Arc::new(move || {
                seen.fetch_add(1, Ordering::SeqCst);
                let launch = copy
                    .mux
                    .read()
                    .get_tab(&target)
                    .unwrap()
                    .restorable_launch
                    .clone();
                let authorization =
                    crate::service::restored_launch::restore_permission_bypass_authorization(
                        &copy,
                        &copy.agent_registry,
                        &target,
                        "opaque-pty",
                        &launch,
                    )?;
                assert_eq!(authorization.is_some(), access == MachineAccess::Full);
                let mut mux = copy.mux.write();
                let tab = mux.get_tab_mut(&target).unwrap();
                tab.boot_stage = Some(BootStage::AgentReady);
                tab.agent_booting = false;
                tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
                Ok(())
            }),
        );
        assert!(service.wake_frozen_tab(&root).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(service.mux.read().get_tab(&root).unwrap().frozen);
        assert_ne!(
            service.mux.read().get_tab(&root).unwrap().boot_stage,
            Some(BootStage::SpawnFailed)
        );
        let metadata = Metadata::new(service.account_data_root().unwrap(), "owner");
        service
            .restore_neighbor_concierge_with(
                &metadata,
                "owner",
                &friend,
                |_, _, _, _, _, _| async {
                    panic!("matching restored grant/digest must not replace the root")
                },
            )
            .await
            .unwrap();
        service.prepare_neighbor_gateway(&root).await.unwrap();
        service.prepare_neighbor_gateway(&root).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(service.neighbor_gateway_ready(&root));
        assert_eq!(
            service
                .mux
                .read()
                .get_tab(&root)
                .unwrap()
                .agent_session_id
                .as_deref(),
            Some("same-restored-session")
        );
        drop(boundary);
        service.mux.write().get_tab_mut(&root).unwrap().frozen = true;
        let failures = Arc::new(AtomicUsize::new(0));
        let seen = failures.clone();
        let _failure = Boundary::install(
            &root,
            Arc::new(move || {
                seen.fetch_add(1, Ordering::SeqCst);
                Err("opaque unrelated or uncertain materialization failure".into())
            }),
        );
        assert!(service.wake_frozen_tab(&root).is_err());
        assert!(!service.mux.read().get_tab(&root).unwrap().frozen);
        assert_eq!(
            service.mux.read().get_tab(&root).unwrap().boot_stage,
            Some(BootStage::SpawnFailed)
        );
        service.wake_frozen_tab(&root).unwrap();
        assert_eq!(failures.load(Ordering::SeqCst), 1);
    }
}

use crate::prelude::*;
use codeterm_service_core::service::agent_runtime::launch_options::test_support::register_neighbor_runtime;
use codeterm_service_core::service::agent_runtime::launch_options::*;
use codeterm_service_core::service::SpawnAgentParams;
use std::path::PathBuf;

#[test]
fn concierge_runtime_restart_carries_only_the_current_root_grant() {
    use crate::{
        multiplexer::{
            state::MultiplexerState,
            types::{NeighborAuthority, TabBacking},
        },
        service::test_fixture::ServiceFixture,
    };
    use codeterm_neighbors::permissions::MachineAccess;

    for (access, sandbox) in [
        (MachineAccess::ReadOnly, Some("read-only")),
        (MachineAccess::Folder, Some("workspace-write")),
        (MachineAccess::Full, None),
    ] {
        let (mut mux, id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("pad-runtime".into());
        tab.session_binding_nonce = Some("old-nonce".into());
        tab.agent_session_id = Some("own-thread".into());
        tab.backing = TabBacking::ChatBackend {
            plugin_id: "pad-runtime".into(),
            session_id: "own-thread".into(),
        };
        tab.neighbor_authority = Some(NeighborAuthority {
            uid: "owner".into(),
            friendship_id: "a".repeat(64),
            root_tab_id: None,
        });
        let fixture = crate::test_support::fixture(mux);
        fixture.install_seeded_storage_desk();
        let svc = &fixture.service;
        register_neighbor_runtime(svc);
        svc.set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
        svc.mux.write().get_tab_mut(&id).unwrap().cwd =
            Some(fixture.temp.path().to_string_lossy().into_owned());
        let mut params = SpawnAgentParams {
            agent: Some("pad-runtime".into()),
            sandbox: sandbox.map(str::to_owned),
            neighbor_authority: svc
                .mux
                .read()
                .get_tab(&id)
                .unwrap()
                .neighbor_authority
                .clone(),
            ..Default::default()
        };
        svc.configure_neighbor_launch_access("pad-runtime", access, &mut params)
            .unwrap();
        svc.prepare_agent_runtime_launch_options(&id, &params)
            .unwrap();
        codeterm_service_core::service::agent_runtime::authority::record_spawn_for_test(
            svc,
            &id,
            "pad-runtime",
            "old-nonce",
        );
        let expected =
            codeterm_service_core::service::agent_runtime::launch_options::restore(svc, &id)
                .unwrap();
        let policy = svc
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .restorable_launch
            .execution_sandbox
            .clone();
        let mut nonce = "old-nonce".to_owned();
        for _ in 0..2 {
            let next = codeterm_service_core::service::agent_runtime::authority::rotate_restart(
                svc,
                &id,
                "pad-runtime",
                &nonce,
            )
            .unwrap();
            assert_ne!(next, nonce);
            assert_eq!(
                codeterm_service_core::service::agent_runtime::launch_options::restore(svc, &id)
                    .unwrap(),
                expected
            );
            let mux = svc.mux.read();
            let tab = mux.get_tab(&id).unwrap();
            assert_eq!(tab.agent_session_id.as_deref(), Some("own-thread"));
            assert_eq!(tab.restorable_launch.execution_sandbox, policy);
            assert_eq!(
                tab.restorable_launch
                    .runtime_launch_options
                    .as_ref()
                    .unwrap()
                    .binding_nonce,
                next
            );
            nonce = next;
        }
        svc.mux
            .write()
            .get_tab_mut(&id)
            .unwrap()
            .session_binding_nonce = Some("foreign-nonce".into());
        assert!(svc
            .neighbors()
            .carry_neighbor_restore_grant(&id, "foreign-nonce", "foreign-nonce")
            .is_err());
        assert!(
            codeterm_service_core::service::agent_runtime::launch_options::restore(svc, &id)
                .is_err()
        );
        svc.mux
            .write()
            .get_tab_mut(&id)
            .unwrap()
            .session_binding_nonce = Some(nonce.clone());
        svc.neighbor_identity_changed();
        assert!(
            codeterm_service_core::service::agent_runtime::authority::rotate_restart(
                svc,
                &id,
                "pad-runtime",
                &nonce
            )
            .is_err()
        );
        assert!(
            codeterm_service_core::service::agent_runtime::launch_options::restore(svc, &id)
                .is_err()
        );
    }
}

#[tokio::test]
async fn concierge_runtime_access_matrix_prepares_own_pad_before_fresh_resume_and_restore() {
    use crate::{
        multiplexer::{state::MultiplexerState, types::NeighborAuthority},
        service::test_fixture::ServiceFixture,
    };
    use codeterm_neighbors::permissions::MachineAccess;
    let shared = crate::test_support::fixture(MultiplexerState::new().0);
    register_neighbor_runtime(&shared.service);
    shared.install_seeded_storage_desk();
    let mut shared = Some(shared);
    for (access, sandbox, mode) in [
        (MachineAccess::ReadOnly, Some("read-only"), "opaque-pad"),
        (
            MachineAccess::Folder,
            Some("workspace-write"),
            "opaque-folder",
        ),
        (MachineAccess::Full, None, "full-access"),
    ] {
        for resumed in [false, true] {
            let (mut mux, tab_id) = MultiplexerState::new();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_type = Some("pad-runtime".into());
            tab.session_binding_nonce = Some("opaque-nonce".into());
            tab.agent_session_id = Some(
                if resumed {
                    "resumed-thread"
                } else {
                    "fresh-thread"
                }
                .into(),
            );
            tab.neighbor_authority = Some(NeighborAuthority {
                uid: "owner".into(),
                friendship_id: "a".repeat(64),
                root_tab_id: None,
            });
            let data_dir = tempfile::tempdir().unwrap();
            let disk_restart = access == MachineAccess::Folder && resumed;
            let fixture = if disk_restart {
                let fixture = crate::test_support::fixture_at(mux, data_dir.path());
                register_neighbor_runtime(&fixture.service);
                fixture.install_seeded_storage_desk();
                fixture
            } else {
                let fixture = shared.take().unwrap();
                *fixture.service.mux.write() = mux;
                fixture
            };
            fixture
                .service
                .set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
            let svc = &fixture.service;
            let cwd = data_dir.path().join("friend");
            std::fs::create_dir(&cwd).unwrap();
            svc.mux.write().get_tab_mut(&tab_id).unwrap().cwd =
                Some(cwd.to_string_lossy().into_owned());
            let mut params = SpawnAgentParams {
                agent: Some("pad-runtime".into()),
                cwd: Some(cwd.to_string_lossy().into_owned()),
                sandbox: sandbox.map(str::to_owned),
                resume_session_id: resumed.then(|| "resumed-thread".into()),
                ..Default::default()
            };
            params.neighbor_authority = svc
                .mux
                .read()
                .get_tab(&tab_id)
                .unwrap()
                .neighbor_authority
                .clone();
            svc.configure_neighbor_launch_access("pad-runtime", access, &mut params)
                .unwrap();
            svc.prepare_agent_runtime_launch_options(&tab_id, &params)
                .unwrap();
            let pad = svc.ensure_agent_pad(&tab_id).unwrap();
            let expected = validate_host_directories(vec![pad.path().to_path_buf()]).unwrap();
            std::fs::write(pad.path().join("reply.txt"), "opaque reply").unwrap();
            let saved = svc
                .mux
                .read()
                .get_tab(&tab_id)
                .unwrap()
                .restorable_launch
                .runtime_launch_options
                .clone()
                .unwrap();
            assert_eq!(saved.mode, mode);
            assert_eq!(saved.additional_directories, expected);
            assert_eq!(
                saved.network_access,
                (access != MachineAccess::Full).then_some(false)
            );
            assert_eq!(
                PathBuf::from(
                    svc.mux
                        .read()
                        .get_tab(&tab_id)
                        .unwrap()
                        .cwd
                        .as_ref()
                        .unwrap()
                )
                .canonicalize()
                .unwrap(),
                PathBuf::from(params.cwd.as_ref().unwrap())
                    .canonicalize()
                    .unwrap()
            );
            let snapshot = serde_json::to_vec(&svc.mux.read().to_session_snapshot()).unwrap();
            let snapshot = serde_json::from_slice(&snapshot).unwrap();
            let restored_mux = MultiplexerState::from_session_snapshot(&snapshot);
            let fixture = if disk_restart {
                fixture
                    .save_neighbor_restore_settings(
                        "owner",
                        &"a".repeat(64),
                        &tab_id,
                        codeterm_neighbors::permissions::Permission {
                            machine_access: access,
                            folder: Some(cwd.to_string_lossy().into_owned()),
                            ..Default::default()
                        },
                    )
                    .await;
                drop(fixture);
                let fixture = crate::test_support::fixture_at(restored_mux, data_dir.path());
                register_neighbor_runtime(&fixture.service);
                fixture.install_seeded_storage_desk();
                fixture.service.set_neighbor_friend_receiver_for_test(
                    &"a".repeat(64),
                    Some(true),
                    1,
                );
                assert_eq!(
                    codeterm_service_core::service::agent_runtime::launch_options::restore(
                        &fixture.service,
                        &tab_id
                    )
                    .unwrap_err(),
                    codeterm_neighbors::Error::RestoreGrantRequired.to_string()
                );
                fixture.load_neighbor_restore_settings("owner").await;
                fixture
            } else {
                *fixture.service.mux.write() = restored_mux;
                fixture
                    .service
                    .mux
                    .write()
                    .get_tab_mut(&tab_id)
                    .unwrap()
                    .session_binding_nonce = Some("restored-nonce".into());
                assert_eq!(
                    codeterm_service_core::service::agent_runtime::launch_options::restore(
                        &fixture.service,
                        &tab_id
                    )
                    .unwrap_err(),
                    codeterm_neighbors::Error::RestoreGrantRequired.to_string()
                );
                fixture
                    .service
                    .neighbors()
                    .prepare_neighbor_restore_grant(&tab_id, "pad-runtime", &params)
                    .unwrap();
                fixture
            };
            let svc = &fixture.service;
            let options = codeterm_service_core::service::agent_runtime::launch_options::restore(
                svc, &tab_id,
            )
            .unwrap();
            assert_eq!(options.mode.as_deref(), Some(mode));
            let pad = svc.ensure_agent_pad(&tab_id).unwrap();
            assert_eq!(
                options.additional_directories,
                validate_host_directories(vec![pad.path().to_path_buf()]).unwrap()
            );
            std::fs::write(pad.path().join("restored-reply.txt"), "opaque reply").unwrap();
            assert_eq!(
                svc.mux
                    .read()
                    .get_tab(&tab_id)
                    .unwrap()
                    .agent_session_id
                    .as_deref(),
                Some(if resumed {
                    "resumed-thread"
                } else {
                    "fresh-thread"
                })
            );
            if !disk_restart {
                shared = Some(fixture);
            }
        }
    }
}

#[test]
fn contradictory_neighbor_bypass_records_require_current_root_grant_for_both_restore_adapters() {
    use crate::{
        multiplexer::{state::MultiplexerState, types::NeighborAuthority},
        service::test_fixture::ServiceFixture,
    };
    use codeterm_neighbors::permissions::MachineAccess;
    for access in [MachineAccess::ReadOnly, MachineAccess::Folder] {
        for global in [false, true] {
            let (mut mux, root) = MultiplexerState::new();
            let workspace = mux.find_workspace_for_tab(&root).unwrap();
            let child = mux.create_tab(&workspace).unwrap();
            let friend = "a".repeat(64);
            for (id, parent, nonce) in [
                (&root, None, "root-nonce"),
                (&child, Some(root.clone()), "child-nonce"),
            ] {
                let tab = mux.get_tab_mut(id).unwrap();
                tab.agent_type = Some("pad-runtime".into());
                tab.session_binding_nonce = Some(nonce.into());
                tab.neighbor_authority = Some(NeighborAuthority {
                    uid: "owner".into(),
                    friendship_id: friend.clone(),
                    root_tab_id: parent,
                });
                tab.restorable_launch.captured = true;
                tab.restorable_launch.permission_bypass_requested = true;
            }
            let fixture = crate::test_support::fixture(mux);
            fixture
                .service
                .set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
            fixture.set_agent_full_access(global);
            register_neighbor_runtime(&fixture.service);
            let svc = &fixture.service;
            let mut params = SpawnAgentParams {
                neighbor_authority: svc
                    .mux
                    .read()
                    .get_tab(&root)
                    .unwrap()
                    .neighbor_authority
                    .clone(),
                ..Default::default()
            };
            svc.configure_neighbor_launch_access("pad-runtime", access, &mut params)
                .unwrap();
            svc.neighbors()
                .prepare_neighbor_restore_grant(&root, "pad-runtime", &params)
                .unwrap();
            for id in [&root, &child] {
                let launch = svc
                    .mux
                    .read()
                    .get_tab(id)
                    .unwrap()
                    .restorable_launch
                    .clone();
                assert_eq!(
                    crate::service::restored_launch::restore_permission_bypass_authorization(
                        svc,
                        &svc.agent_registry,
                        id,
                        "pad-runtime",
                        &launch
                    )
                    .unwrap_err(),
                    codeterm_neighbors::Error::RestoreGrantRequired.to_string()
                );
                assert_eq!(
                    codeterm_service_core::service::agent_runtime::launch_options::restore(svc, id)
                        .unwrap_err(),
                    codeterm_neighbors::Error::RestoreGrantRequired.to_string()
                );
                assert!(svc
                    .mux
                    .read()
                    .get_tab(id)
                    .unwrap()
                    .restorable_launch
                    .runtime_launch_options
                    .is_none());
            }
            for id in [&root, &child] {
                svc.mux
                    .write()
                    .get_tab_mut(id)
                    .unwrap()
                    .restorable_launch
                    .permission_bypass_requested = false;
                assert!(svc
                    .neighbors()
                    .require_neighbor_restore_grant(id, false)
                    .is_ok());
            }
            svc.neighbor_identity_changed();
            assert!(svc
                .neighbors()
                .require_neighbor_restore_grant(&root, false)
                .is_err());
        }
    }
}

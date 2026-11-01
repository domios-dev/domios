use super::*;

#[test]
fn permission_mode_never_broadens_an_explicit_false_or_an_unapproved_request() {
    assert_eq!(
        authorized_mode(Some(false), true).unwrap(),
        "auto-accept-edits"
    );
    assert_eq!(authorized_mode(Some(true), true).unwrap(), "full-access");
    assert!(authorized_mode(Some(true), false).is_err());
    assert!(authorized_mode(None, false).is_err());
}

#[test]
fn writable_roots_require_existing_directories_and_deduplicate_canonical_authority() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
    let validated = validate_host_directories(vec![root.clone(), root.join(".")]).unwrap();
    assert_eq!(validated.len(), 1);
    assert_eq!(
        validated[0],
        codeterm_paths::strip_verbatim_prefix(root.canonicalize().unwrap().to_str().unwrap())
    );
    assert!(validate_host_directories(vec![root.join("missing")]).is_err());
    let file = root.join("file");
    std::fs::write(&file, "opaque").unwrap();
    assert!(validate_host_directories(vec![file]).is_err());
    assert!(validate_host_directories(vec![PathBuf::from("relative")]).is_err());
}

#[test]
fn saved_options_round_trip_preserves_directories_and_explicit_network_denial() {
    let options = RestorableRuntimeLaunchOptions {
        provider_id: "fixture-provider".into(),
        binding_nonce: "fixture-nonce".into(),
        mode: "auto-accept-edits".into(),
        additional_directories: vec![std::env::current_dir().unwrap().to_str().unwrap().into()],
        network_access: Some(false),
    };
    let saved = serde_json::to_vec(&options).unwrap();
    let restored: RestorableRuntimeLaunchOptions = serde_json::from_slice(&saved).unwrap();
    assert_eq!(restored, options);
    assert_eq!(runtime_settings(&restored).network_access, Some(false));
    assert_eq!(
        runtime_settings(&restored).additional_directories,
        options.additional_directories
    );
}

fn register_neighbor_runtime(svc: &ServiceContext) {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    let manifest = PluginManifest::from_json_str(&serde_json::json!({
        "id":"pad-runtime","version":"1.0.0","displayName":"Fixture",
        "commands":{"autoApproveFlags":["opaque-full"]},
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtimeModes":[{"id":"full-access","displayName":"Full"}],
                "runtime":"stdio-jsonrpc","command":{"binary":"fixture-cli","args":[]},
                "requestMethods":{"initialize":["handshake"]},"notificationMethods":[]}},
        "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},"taskDelivery":"pty_input","verifySubmit":{"kind":"none"},
            "executionSandbox":{"platforms":["windows","linux","macos"],
                "readOnly":{"launchArgs":[],"runtimeMode":"opaque-read"},
                "runtimeReadOnlyWritableRootsMode":"opaque-pad",
                "mutating":{"launchArgs":[],"runtimeMode":"opaque-folder"}}},
        "permissions":{"subprocess":{"allow":["fixture-cli"]}}
    }).to_string()).unwrap();
    let vm = std::sync::Arc::new(
        PluginVm::from_source("pad-runtime", "module.exports={}".into()).unwrap(),
    );
    svc.agent_registry
        .register_provider(Box::new(JsProvider::new(manifest, vm)));
}

#[tokio::test]
async fn concierge_runtime_access_matrix_prepares_own_pad_before_fresh_resume_and_restore() {
    use crate::{
        multiplexer::{state::MultiplexerState, types::NeighborAuthority},
        service::test_fixture::ServiceFixture,
    };
    use codeterm_neighbors::permissions::MachineAccess;
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
            let fixture = ServiceFixture::new_at(mux, data_dir.path());
            fixture
                .service
                .set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
            let svc = &fixture.service;
            register_neighbor_runtime(svc);
            let cwd = data_dir.path().join("friend");
            std::fs::create_dir(&cwd).unwrap();
            svc.mux.write().get_tab_mut(&tab_id).unwrap().cwd =
                Some(cwd.to_string_lossy().into_owned());
            fixture.install_seeded_storage_desk();
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
                svc.mux.read().get_tab(&tab_id).unwrap().cwd.as_deref(),
                params.cwd.as_deref()
            );
            let snapshot = serde_json::to_vec(&svc.mux.read().to_session_snapshot()).unwrap();
            let snapshot = serde_json::from_slice(&snapshot).unwrap();
            fixture
                .save_neighbor_restore_settings(
                    "owner",
                    &"a".repeat(64),
                    &tab_id,
                    codeterm_neighbors::permissions::Permission {
                        machine_access: access,
                        folder: (access == MachineAccess::Folder)
                            .then(|| cwd.to_string_lossy().into_owned()),
                        ..Default::default()
                    },
                )
                .await;
            drop(fixture);
            let fixture = ServiceFixture::new_at(
                MultiplexerState::from_session_snapshot(&snapshot),
                data_dir.path(),
            );
            let svc = &fixture.service;
            register_neighbor_runtime(svc);
            fixture.install_seeded_storage_desk();
            svc.set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
            assert_eq!(
                super::restore(svc, &tab_id).unwrap_err(),
                codeterm_neighbors::Error::MachineAccessUnsupported.to_string()
            );
            fixture.load_neighbor_restore_settings("owner").await;
            let options = super::restore(svc, &tab_id).unwrap();
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
            let fixture = ServiceFixture::new(mux);
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
            svc.prepare_neighbor_restore_grant(&root, "pad-runtime", &params)
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
                    codeterm_neighbors::Error::MachineAccessUnsupported.to_string()
                );
                assert_eq!(
                    super::restore(svc, id).unwrap_err(),
                    codeterm_neighbors::Error::MachineAccessUnsupported.to_string()
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
                assert!(svc.require_neighbor_restore_grant(id, false).is_ok());
            }
            svc.neighbor_identity_changed();
            assert!(svc.require_neighbor_restore_grant(&root, false).is_err());
        }
    }
}

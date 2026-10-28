use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use codeterm_neighbors::permissions::MachineAccess;

async fn fixture(access: MachineAccess) -> (ServiceFixture, String, String) {
    let (mut mux, general) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&general).unwrap();
    let root = mux.create_tab(&workspace).unwrap();
    let project = mux.create_workspace("opaque-project".into(), None).id;
    let tab = mux.get_tab_mut(&root).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("own-session".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    let fixture = ServiceFixture::new(mux);
    let manifest = codeterm_plugin::js_runtime::PluginManifest::from_json_str(&serde_json::json!({
        "id":"opaque-provider","version":"1.0.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true},
        "commands":{"autoApproveFlags":["opaque-full"]},
        "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},"taskDelivery":"pty_input","verifySubmit":{"kind":"none"},
            "executionSandbox":{"platforms":["windows","macos","linux"],
                "readOnly":{"launchArgs":["opaque-read"],"runtimeMode":"opaque-read"},
                "mutating":{"launchArgs":["opaque-folder"],"runtimeMode":"opaque-folder"}}}
    }).to_string()).unwrap();
    let vm = std::sync::Arc::new(
        codeterm_plugin::js_runtime::PluginVm::from_source(
            "opaque-provider",
            "module.exports={}".into(),
        )
        .unwrap(),
    );
    fixture.service.agent_registry.register_provider(Box::new(
        codeterm_plugin::js_runtime::JsProvider::new(manifest, vm),
    ));
    fixture
        .seed_neighbor_concierge(
            &root,
            &"a".repeat(64),
            Permission {
                allowed: true,
                scope: "opaque".into(),
                allow_tab_requests: true,
                workspaces: Some(vec![project.clone()]),
                machine_access: access,
                folder: (access == MachineAccess::Folder)
                    .then(|| fixture.temp.path().to_string_lossy().into_owned()),
                ..Default::default()
            },
        )
        .await;
    (fixture, root, project)
}

#[tokio::test]
async fn descendants_inherit_each_machine_level_and_keep_the_original_root_authority() {
    for global in [false, true] {
        for (access, expected) in [
            (MachineAccess::ReadOnly, Some("read-only")),
            (MachineAccess::Folder, Some("workspace-write")),
            (MachineAccess::Full, None),
        ] {
            let (fixture, root, project) = fixture(access).await;
            fixture.set_agent_full_access(global);
            let service = &fixture.service;
            let mut params = SpawnAgentParams {
                spawned_by_tab_id: Some(root.clone()),
                workspace_id: Some(project.clone()),
                sandbox: Some("workspace-write".into()),
                ..Default::default()
            };
            assert!(service
                .inherit_neighbor_agent_spawn(&mut params)
                .await
                .unwrap());
            let authority = params.neighbor_authority.clone().unwrap();
            assert_eq!(authority.friendship_id, "a".repeat(64));
            assert_eq!(authority.root_tab_id.as_deref(), Some(root.as_str()));
            assert_eq!(params.sandbox.as_deref(), expected);
            assert_eq!(params.skip_permissions, Some(access == MachineAccess::Full));
            assert_eq!(
                params.permission_bypass_authorization.is_some(),
                access == MachineAccess::Full
            );
            assert_eq!(
                params.permission_bypass_requested("opaque-provider"),
                access == MachineAccess::Full
            );
            assert!(params
                .cwd
                .as_ref()
                .is_some_and(|cwd| std::path::Path::new(cwd).is_absolute()));
            let child = {
                let mut mux = service.mux.write();
                let child = mux.create_tab(&project).unwrap();
                let tab = mux.get_tab_mut(&child).unwrap();
                tab.agent_type = Some("opaque-provider".into());
                tab.agent_session_id = Some("child-session".into());
                tab.session_binding_nonce = Some("child-nonce".into());
                tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
                tab.neighbor_authority = Some(authority.clone());
                child
            };
            let mut grandchild = SpawnAgentParams {
                creator_tab_id: Some(child.clone()),
                workspace_id: Some(project.clone()),
                ..Default::default()
            };
            assert!(service
                .inherit_neighbor_agent_spawn(&mut grandchild)
                .await
                .unwrap());
            assert_eq!(grandchild.neighbor_authority, Some(authority));
            assert_eq!(grandchild.parent_tab_id.as_deref(), Some(child.as_str()));
            assert_eq!(
                grandchild.skip_permissions,
                Some(access == MachineAccess::Full)
            );
            assert_eq!(
                grandchild.permission_bypass_authorization.is_some(),
                access == MachineAccess::Full
            );
            assert_eq!(
                grandchild.permission_bypass_requested("opaque-provider"),
                access == MachineAccess::Full
            );
            let token = service
                .frontend_pty_env(Some(&child))
                .remove("CODETERM_API_TOKEN")
                .unwrap();
            for (friendship, allowed) in [("a".repeat(64), true), ("b".repeat(64), false)] {
                let command = crate::service::neighbors::runtime::NeighborRuntimeCommand::History {
                    friendship_id: friendship,
                    before: None,
                    limit: Some(1),
                };
                assert_eq!(
                    service
                        .neighbor_runtime(command, Some(&child), Some("child-nonce"))
                        .await
                        .is_ok(),
                    allowed
                );
            }
            assert!(service
                .authorize_neighbor_rpc_credential(&token, "mem.search", None)
                .await
                .is_ok());
            for method in [
                "mem.secret.get",
                "mem.save",
                "plugin:opaque.execute",
                "notification.create",
                "config.set",
                "mesh.relay",
            ] {
                assert!(
                    service
                        .authorize_neighbor_rpc_credential(&token, method, None)
                        .await
                        .is_err(),
                    "{method}"
                );
            }
            for candidate in [params, grandchild.clone()] {
                let copy = service.clone();
                let spec = crate::service::spawn::SpawnTabSpec::from_agent_params(candidate);
                let prepared = tokio::task::spawn_blocking(move || {
                    let mut spec = spec;
                    let _gate = copy.neighbor_tab_launch_guard(&spec).unwrap();
                    copy.prepare_neighbor_tab_spawn(&mut spec).unwrap();
                    spec
                })
                .await
                .unwrap();
                assert_eq!(
                    prepared.to_agent_params().unwrap().neighbor_authority,
                    grandchild.neighbor_authority
                );
                assert_eq!(prepared.cwd, grandchild.cwd);
                let prepared_params = &prepared.chatbackend_launch.as_ref().unwrap().params;
                assert_eq!(
                    prepared_params.skip_permissions,
                    Some(access == MachineAccess::Full)
                );
                assert_eq!(
                    prepared_params.permission_bypass_authorization.is_some(),
                    access == MachineAccess::Full
                );
                assert_eq!(
                    prepared_params.permission_bypass_requested("opaque-provider"),
                    access == MachineAccess::Full
                );
            }
            let replacement = service.mux.write().create_tab(&project).unwrap();
            fixture
                .seed_neighbor_concierge(
                    &replacement,
                    &"a".repeat(64),
                    Permission {
                        allowed: true,
                        scope: "opaque replacement".into(),
                        ..Default::default()
                    },
                )
                .await;
            assert!(service
                .authorize_neighbor_rpc_credential(&token, "mem.search", None)
                .await
                .is_err());
        }
    }
}

#[tokio::test]
async fn missing_workspaces_unsafe_launch_options_and_mutating_friendships_create_no_child() {
    let (fixture, root, project) = fixture(MachineAccess::ReadOnly).await;
    let service = &fixture.service;
    let before = service.mux.read().snapshot().tabs.len();
    let base = SpawnAgentParams {
        spawned_by_tab_id: Some(root),
        workspace_id: Some(project),
        ..Default::default()
    };
    let mut cases = Vec::new();
    let mut missing = base.clone();
    missing.workspace_id = None;
    cases.push(missing);
    let mut foreign = base.clone();
    foreign.workspace_id = Some("missing".into());
    cases.push(foreign);
    let mut flags = base.clone();
    flags.args = Some(vec!["opaque-widen".into()]);
    cases.push(flags);
    let mut resume = base.clone();
    resume.resume_session_id = Some("foreign-session".into());
    cases.push(resume);
    let mut plan = base.clone();
    plan.plan_id = Some("foreign-plan".into());
    cases.push(plan);
    let mut forged = base.clone();
    forged.neighbor_authority = Some(NeighborAuthority {
        uid: "forged".into(),
        friendship_id: "b".repeat(64),
        root_tab_id: None,
    });
    cases.push(forged);
    for mut params in cases {
        assert!(service
            .inherit_neighbor_agent_spawn(&mut params)
            .await
            .is_err());
    }
    let mutation = service
        .begin_neighbor_mutation(&"a".repeat(64))
        .await
        .unwrap();
    let nested = service
        .begin_neighbor_mutation(&"a".repeat(64))
        .await
        .unwrap();
    let mut params = base.clone();
    assert!(service
        .inherit_neighbor_agent_spawn(&mut params)
        .await
        .is_err());
    drop(nested);
    assert!(service
        .inherit_neighbor_agent_spawn(&mut params)
        .await
        .is_err());
    drop(mutation);
    assert!(service
        .inherit_neighbor_agent_spawn(&mut params)
        .await
        .unwrap());
    assert_eq!(service.mux.read().snapshot().tabs.len(), before);
}

#[tokio::test]
async fn closing_a_concierge_retires_its_descendants_into_their_own_recently_closed() {
    let (fixture, root, project) = fixture(MachineAccess::Full).await;
    let service = &fixture.service;
    let child = {
        let mut mux = service.mux.write();
        let child = mux.create_tab(&project).unwrap();
        let mut authority = mux
            .get_tab(&root)
            .unwrap()
            .neighbor_authority
            .clone()
            .unwrap();
        authority.root_tab_id = Some(root.clone());
        mux.get_tab_mut(&child).unwrap().neighbor_authority = Some(authority);
        child
    };
    fixture.install_seeded_storage_desk();
    let uid = service
        .mux
        .read()
        .get_tab(&root)
        .unwrap()
        .neighbor_authority
        .as_ref()
        .unwrap()
        .uid
        .clone();
    let metadata = Metadata::new(service.account_data_root().unwrap(), &uid);
    service.observe_neighbor_pause(&uid, &"a".repeat(64), true);
    service
        .settle_neighbor_pause(&metadata, &"a".repeat(64))
        .await
        .unwrap();
    assert!(service.mux.read().get_tab(&child).unwrap().frozen);
    service.observe_neighbor_pause(&uid, &"a".repeat(64), false);
    assert!(!service.mux.read().get_tab(&child).unwrap().frozen);
    service.close_tab_async(&root).await.unwrap();
    let mux = service.mux.read();
    assert!(mux.get_tab(&root).is_none());
    assert!(mux.get_tab(&child).is_none());
    assert!(mux
        .list_recently_closed(&project)
        .iter()
        .any(|entry| entry.tab_id == child));
}

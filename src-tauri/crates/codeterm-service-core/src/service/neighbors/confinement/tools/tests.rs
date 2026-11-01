use super::*;
use crate::{
    multiplexer::{state::MultiplexerState, types::NeighborAuthority},
    service::{test_fixture::ServiceFixture, SpawnAgentParams},
};
use std::sync::Arc;

fn register(svc: &ServiceContext) {
    let permissions = json!({"allowRuleArgsFormat":["opaque-allow","{rule}"],"commandRuleFormat":"OpaqueShell({command}:*)",
        "scopedFilesystem":{"noPromptArgs":["opaque-mode","opaque-no-prompt"],"readAllRules":["OpaqueRead","OpaqueGlob","OpaqueGrep"],
            "readRootRule":"OpaqueRead({path}/**)","writeRootRule":"OpaqueEdit({path}/**)","additionalDirectoryArgs":["opaque-dir","{path}"]}});
    let manifest = codeterm_plugin::js_runtime::PluginManifest::from_json_str(&json!({
        "id":"scoped-pty","version":"1.0.0","displayName":"Fixture","capabilities":{"agentProvider":true},
        "commands":{"autoApproveFlags":["opaque-full"]},
        "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},"taskDelivery":"pty_input","verifySubmit":{"kind":"none"},"launchPermissions":permissions}
    }).to_string()).unwrap();
    let vm=codeterm_plugin::js_runtime::PluginVm::from_source("scoped-pty", r#"module.exports={buildResumeCommand:()=> 'opaque-resume',buildLaunchSpec:p=>{const q=p.resume||p;return {program:'opaque-cli',args:[...(q.args||[]),...(q.skipPermissions?['opaque-full']:[])],envSet:{},envUnset:[]}}}"#.into()).unwrap();
    svc.agent_registry
        .register_provider(Box::new(codeterm_plugin::js_runtime::JsProvider::new(
            manifest,
            Arc::new(vm),
        )));
}

fn assert_policy(args: &[String], access: MachineAccess) {
    assert_eq!(
        args.iter().any(|arg| arg == "opaque-full"),
        access == MachineAccess::Full
    );
    assert_eq!(
        args.windows(2)
            .any(|pair| pair == ["opaque-mode", "opaque-no-prompt"]),
        access != MachineAccess::Full
    );
    assert_eq!(
        args.iter()
            .filter(|arg| arg.starts_with("OpaqueEdit("))
            .count(),
        match access {
            MachineAccess::ReadOnly => 1,
            MachineAccess::Folder => 2,
            _ => 0,
        }
    );
    for read in ["OpaqueRead", "OpaqueGlob", "OpaqueGrep"] {
        assert_eq!(
            args.iter().any(|arg| arg == read),
            access == MachineAccess::ReadOnly
        );
    }
    assert!(!args.iter().any(
        |arg| ["OpaqueWrite", "OpaqueEdit", "opaque-accept-all-edits"].contains(&arg.as_str())
    ));
    if access != MachineAccess::Full {
        assert!(args.iter().any(|arg| arg == "OpaqueShell(domios:*)"));
    }
}

#[tokio::test]
async fn advisory_policy_matrix_reaches_fresh_resume_and_cold_restore_launch_specs() {
    for access in [
        MachineAccess::ReadOnly,
        MachineAccess::Folder,
        MachineAccess::Full,
    ] {
        for resumed in [false, true] {
            let data = tempfile::tempdir().unwrap();
            let (mut mux, tab_id) = MultiplexerState::new();
            let root = mux.get_tab_mut(&tab_id).unwrap();
            root.agent_type = Some("scoped-pty".into());
            root.agent_session_id = Some("opaque-own-session".into());
            root.session_binding_nonce = Some("opaque-own-nonce".into());
            root.neighbor_authority = Some(NeighborAuthority {
                uid: "owner".into(),
                friendship_id: "a".repeat(64),
                root_tab_id: None,
            });
            let fixture = ServiceFixture::new_at(mux, data.path());
            register(&fixture.service);
            fixture
                .service
                .set_neighbor_friend_receiver_for_test(&"a".repeat(64), Some(true), 1);
            let folder = data.path().join("friend folder");
            std::fs::create_dir_all(&folder).unwrap();
            let folder = codeterm_paths::strip_verbatim_prefix(
                folder.canonicalize().unwrap().to_str().unwrap(),
            );
            fixture
                .service
                .mux
                .write()
                .get_tab_mut(&tab_id)
                .unwrap()
                .cwd = Some(folder.clone());
            fixture.install_seeded_storage_desk();
            let svc = &fixture.service;
            let mut params = SpawnAgentParams {
                agent: Some("scoped-pty".into()),
                cwd: Some(folder.clone()),
                resume_session_id: resumed.then(|| "opaque-own-session".into()),
                neighbor_authority: svc
                    .mux
                    .read()
                    .get_tab(&tab_id)
                    .unwrap()
                    .neighbor_authority
                    .clone(),
                ..Default::default()
            };
            svc.configure_neighbor_launch_access("scoped-pty", access, &mut params)
                .unwrap();
            let command = crate::service::orchestrator::prepare_agent_command_for_service(
                svc,
                &tab_id,
                "scoped-pty",
                &params,
                None,
                None,
                &svc.agent_registry,
                None,
            );
            let args = command.direct.unwrap().args;
            assert_policy(&args, access);
            if access != MachineAccess::Full {
                let mut custom = params.clone();
                custom.args = Some(vec!["opaque-permission-override".into()]);
                assert!(svc
                    .neighbor_scoped_tool_launch_args(&tab_id, "scoped-pty", &custom)
                    .is_err());
            }
            let expected = svc
                .neighbor_scoped_tool_launch_args(&tab_id, "scoped-pty", &params)
                .unwrap();
            for argument in &expected {
                assert!(args.contains(argument));
            }
            {
                let mut mux = svc.mux.write();
                let tab = mux.get_tab_mut(&tab_id).unwrap();
                tab.restorable_launch.captured = true;
                tab.restorable_launch.permission_bypass_requested = access == MachineAccess::Full;
            }
            fixture
                .save_neighbor_restore_settings(
                    "owner",
                    &"a".repeat(64),
                    &tab_id,
                    Permission {
                        machine_access: access,
                        folder: (access == MachineAccess::Folder).then_some(folder.clone()),
                        ..Default::default()
                    },
                )
                .await;
            let snapshot = serde_json::to_vec(&svc.mux.read().to_session_snapshot()).unwrap();
            drop(fixture);
            let fixture = ServiceFixture::new_at(
                MultiplexerState::from_session_snapshot(
                    &serde_json::from_slice(&snapshot).unwrap(),
                ),
                data.path(),
            );
            register(&fixture.service);
            fixture.install_seeded_storage_desk();
            fixture.load_neighbor_restore_settings("owner").await;
            let svc = &fixture.service;
            let launch = svc
                .mux
                .read()
                .get_tab(&tab_id)
                .unwrap()
                .restorable_launch
                .clone();
            let authorization =
                crate::service::restored_launch::restore_permission_bypass_authorization(
                    svc,
                    &svc.agent_registry,
                    &tab_id,
                    "scoped-pty",
                    &launch,
                )
                .unwrap();
            let params = SpawnAgentParams {
                agent: Some("scoped-pty".into()),
                cwd: Some(folder),
                resume_session_id: Some("opaque-own-session".into()),
                skip_permissions: Some(authorization.is_some()),
                permission_bypass_authorization: authorization,
                ..Default::default()
            };
            let input = crate::service::restored_launch::restored_fresh_launch_input(
                &svc.agent_registry,
                params,
                launch,
            );
            assert!(input.params.neighbor_machine_access.is_none());
            let restored = input
                .prepared_command(svc, &tab_id)
                .unwrap()
                .direct
                .unwrap()
                .args;
            assert_policy(&restored, access);
            assert_eq!(
                svc.neighbor_scoped_tool_launch_args(&tab_id, "scoped-pty", &input.params)
                    .unwrap(),
                expected
            );
        }
    }
}

#[test]
fn caller_payload_cannot_set_host_machine_access_or_neighbor_authority() {
    let params: SpawnAgentParams=serde_json::from_value(json!({"neighbor_machine_access":"full","neighbor_authority":{"uid":"forged","friendship_id":"a".repeat(64),"root_tab_id":null}})).unwrap();
    assert!(params.neighbor_machine_access.is_none());
    assert!(params.neighbor_authority.is_none());
}

use crate::prelude::*;
use crate::service::{neighbors::storage::Metadata, SpawnAgentParams};
use codeterm_neighbors::permissions::{MachineAccess, Permission};
use codeterm_service_core::service::agent_runtime::{self, process::test_ports};
use serde_json::{json, Value};
use std::sync::Arc;

const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    const method = {initialize:'fixture/initialize',openThread:'fixture/open',readHistory:'fixture/history'}[op.kind];
    return {action:{kind:'request',call:{method,params:{settings:op.settings,policy:ctx.restrictedCli}}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    if (op.kind === 'initialize') return {kind:'complete',result:{kind:'initialized'}};
    if (op.kind === 'openThread') return {kind:'complete',result:{kind:'threadLoaded',threadId:'fresh-thread',settings:reply.value.settings}};
    return {kind:'complete',result:{kind:'historyPage',threadId:op.threadId,items:[],openItems:[],order:op.order}};
  },
  runtimeReduce() { return {state:null,events:[]}; }
};
"#;

#[tokio::test]
async fn owner_permission_review_launches_confined_runtime_with_bound_cli_policy() {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    for access in [MachineAccess::ReadOnly, MachineAccess::Folder] {
        let (mux, general) = crate::multiplexer::state::MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&general).unwrap();
        let fixture = crate::test_support::fixture(mux);
        let service = &fixture.service;
        let _storage = fixture.install_seeded_storage_desk();
        let executable = fixture.temp.path().join("trusted-cli.exe");
        std::fs::write(&executable, b"trusted-cli").unwrap();
        service
            .set_ext(agent_runtime::cli_exception::tests::Executable(executable))
            .unwrap();
        let binary = std::env::current_exe().unwrap();
        let manifest = PluginManifest::from_json_str(&json!({
            "id":"confined-runtime","version":"1.0.0","displayName":"Fixture",
            "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
                "agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":binary,"args":[]},
                    "restrictedCli":{"kind":"execpolicy-service-operations","metadataDirectory":".runtime-policy"},
                    "requestMethods":{"initialize":["fixture/initialize"],"openThread":["fixture/open"],"readHistory":["fixture/history"]},"notificationMethods":[]}},
            "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},"taskDelivery":"pty_input","verifySubmit":{"kind":"none"},
                "executionSandbox":{"platforms":["windows","linux","macos"],"runtimeReadOnlyWritableRootsMode":"read-only-with-writable-roots",
                    "readOnly":{"launchArgs":[],"runtimeMode":"read-only"},"mutating":{"launchArgs":[],"runtimeMode":"workspace-write"}}},
            "permissions":{"subprocess":{"allow":[binary]}}
        }).to_string()).unwrap();
        service
            .agent_registry
            .register_provider(Box::new(JsProvider::new(
                manifest,
                Arc::new(PluginVm::from_source("confined-runtime", SOURCE.into()).unwrap()),
            )));
        let friendship = "a".repeat(64);
        service.set_neighbor_friend_receiver_for_test(&friendship, Some(true), 1);
        let metadata = Metadata::new(fixture.temp.path().into(), "owner");
        let permission = Permission {
            machine_access: access,
            allowed: true,
            ..Default::default()
        };
        let registrations = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let captured = registrations.clone();
        let result = service.apply_neighbor_permission_with(&metadata, "owner", &friendship, permission.clone(),
            service.neighbor_history_generation(), |_, _, cwd, grant, resume, _| async move {
                assert!(resume.is_none());
                let id = service.mux.write().create_tab(&workspace).unwrap();
                {
                    let mut mux = service.mux.write();
                    let tab = mux.get_tab_mut(&id).unwrap();
                    tab.agent_type = Some("confined-runtime".into());
                    tab.cwd = Some(cwd.clone());
                    tab.neighbor_authority = Some(crate::multiplexer::types::NeighborAuthority {
                        uid:"owner".into(),friendship_id:"a".repeat(64),root_tab_id:None,
                    });
                }
                service.ensure_session_binding_nonce(&id);
                let mut params = SpawnAgentParams { agent:Some("confined-runtime".into()),cwd:Some(cwd),
                    sandbox:Some(if grant.machine_access == MachineAccess::ReadOnly {"read-only"} else {"workspace-write"}.into()),
                    neighbor_authority:service.mux.read().get_tab(&id).unwrap().neighbor_authority.clone(), ..Default::default() };
                service.configure_neighbor_launch_access("confined-runtime", grant.machine_access, &mut params)?;
                service.prepare_agent_runtime_launch_options(&id, &params)?;
                let bound = id.clone();
                captured.lock().push(test_ports::register(&id, move || {
                    let (process, peer) = test_ports::connected();
                    let expected = bound.clone();
                    crate::async_runtime::spawn(async move {
                        let (reader, mut writer) = tokio::io::split(peer);
                        let mut lines = BufReader::new(reader).lines();
                        while let Some(line) = lines.next_line().await.unwrap() {
                            let call: Value = serde_json::from_str(&line).unwrap();
                            let policy = &call["params"]["policy"];
                            assert_eq!(policy["tabId"], expected);
                            assert_eq!(policy["metadataDirectory"], ".runtime-policy");
                            let reply = json!({"jsonrpc":"2.0","id":call["id"],"result":{"settings":call["params"]["settings"]}});
                            if writer.write_all(format!("{reply}\n").as_bytes()).await.is_err() { break; }
                        }
                    });
                    process
                }));
                let owner = service.clone();
                let target = id.clone();
                tokio::task::spawn_blocking(move || agent_runtime::transport::launch(&owner, "confined-runtime", &target, json!({})))
                    .await.unwrap()?;
                Ok(id)
            }).await.unwrap();
        let id = result["gateway_tab"].as_str().unwrap();
        let tab = service.mux.read().get_tab(id).unwrap().clone();
        assert_eq!(tab.agent_session_id.as_deref(), Some("fresh-thread"));
        assert!(service.neighbor_gateway_ready(id));
        assert!(service.require_neighbor_restore_grant(id, false).is_ok());
        let session = agent_runtime::engine::for_tab(id).unwrap();
        let context = session.context.lock().clone();
        assert!(context.restricted_cli.is_some());
        assert_eq!(context.settings.network_access, Some(false));
        assert_eq!(
            context.settings.additional_directories,
            tab.restorable_launch
                .execution_sandbox
                .unwrap()
                .writable_roots
        );
        assert_eq!(
            serde_json::to_value(
                metadata.settings().await.unwrap().permissions[&friendship].clone()
            )
            .unwrap(),
            serde_json::to_value(permission).unwrap()
        );
        let owner = service.clone();
        tokio::task::spawn_blocking(move || agent_runtime::engine::shutdown_blocking(&owner))
            .await
            .unwrap()
            .unwrap();
    }
}

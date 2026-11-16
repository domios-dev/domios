use crate::prelude::*;
use codeterm_plugin::agent_runtime::RuntimeSettings;
use codeterm_service_core::service::agent_runtime::engine::{sync_fixture as sync_send, *};
use codeterm_service_core::service::agent_runtime::model::RuntimePhase;
use serde_json::Value;
use std::{sync::Arc, time::Duration};

#[test]
fn folder_concierge_launch_preserves_policy_until_an_explicit_reported_mode_conflict() {
    use crate::multiplexer::types::NeighborAuthority;
    use codeterm_neighbors::permissions::MachineAccess;
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    const method = {initialize:'fixture/initialize',openThread:'fixture/open',readHistory:'fixture/history'}[op.kind];
    return {action:{kind:'request',call:{method,params:op}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    if (op.kind === 'initialize') return {kind:'complete',result:{kind:'initialized'}};
    if (op.kind === 'openThread') return {kind:'complete',result:{kind:'threadLoaded',threadId:'own-thread',settings:reply.value.settings}};
    return {kind:'complete',result:{kind:'historyPage',threadId:op.threadId,items:[],openItems:[],order:op.order}};
  },
  runtimeReduce(ctx, state, frames) { return {state:null,events:[]}; }
};
"#;
    crate::register_neighbors();
    for reported_mode in ["workspace-write", "read-only"] {
        let (fixture, tab_id, peer, _cleanup) = sync_send::runtime_fixture_with(SOURCE);
        let svc = &fixture.service;
        let _storage = fixture.install_seeded_storage_desk();
        let session = for_tab(&tab_id).unwrap();
        let provider = session.provider.clone();
        let manifest = PluginManifest::from_json_str(&serde_json::json!({
            "id":provider,"version":"1.0.0","displayName":"Fixture",
            "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
                "agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":"fixture-cli","args":[]},
                    "requestMethods":{"initialize":["fixture/initialize"],"openThread":["fixture/open"],"readHistory":["fixture/history"]},"notificationMethods":[]}},
            "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},"taskDelivery":"pty_input","verifySubmit":{"kind":"none"},
                "executionSandbox":{"platforms":["windows","linux","macos"],
                "mutating":{"launchArgs":[],"runtimeMode":"workspace-write"}}},
            "permissions":{"subprocess":{"allow":["fixture-cli"]}}
        }).to_string()).unwrap();
        svc.agent_registry
            .register_provider(Box::new(JsProvider::new(
                manifest,
                Arc::new(PluginVm::from_source(&provider, SOURCE.into()).unwrap()),
            )));
        *session.adapter.lock() = svc.agent_registry.provider_arc(&provider);
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("friend");
        std::fs::create_dir(&folder).unwrap();
        let cwd =
            codeterm_paths::strip_verbatim_prefix(folder.canonicalize().unwrap().to_str().unwrap());
        let authority = NeighborAuthority {
            uid: "owner".into(),
            friendship_id: "a".repeat(64),
            root_tab_id: None,
        };
        svc.set_neighbor_friend_receiver_for_test(&authority.friendship_id, Some(true), 1);
        {
            let mut mux = svc.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.cwd = Some(cwd.clone());
            tab.neighbor_authority = Some(authority.clone());
            tab.agent_session_id = None;
            tab.reported_session_binding_nonce = None;
            tab.agent_booting = true;
            if let crate::multiplexer::types::TabBacking::ChatBackend { session_id, .. } =
                &mut tab.backing
            {
                session_id.clear();
            }
        }
        let mut params = crate::service::SpawnAgentParams {
            agent: Some(provider.clone()),
            cwd: Some(cwd.clone()),
            sandbox: Some("workspace-write".into()),
            neighbor_authority: Some(authority.clone()),
            ..Default::default()
        };
        svc.configure_neighbor_launch_access(&provider, MachineAccess::Folder, &mut params)
            .unwrap();
        svc.prepare_agent_runtime_launch_options(&tab_id, &params)
            .unwrap();
        let saved = svc.mux.read().get_tab(&tab_id).unwrap().clone();
        let policy = saved.restorable_launch.execution_sandbox.clone().unwrap();
        let options = saved
            .restorable_launch
            .runtime_launch_options
            .clone()
            .unwrap();
        let pad = svc.ensure_agent_pad(&tab_id).unwrap();
        assert_eq!(policy.cwd, cwd);
        assert_eq!(policy.runtime_mode.as_deref(), Some("workspace-write"));
        assert_eq!(
            policy.writable_roots,
            vec![codeterm_paths::strip_verbatim_prefix(
                pad.path().canonicalize().unwrap().to_str().unwrap(),
            )]
        );
        assert_eq!(options.additional_directories, policy.writable_roots);
        assert_eq!(options.network_access, Some(false));
        assert_eq!(
            options.binding_nonce,
            saved.session_binding_nonce.clone().unwrap()
        );
        let restored =
            codeterm_service_core::service::agent_runtime::launch_options::restore(svc, &tab_id)
                .unwrap();
        crate::service::execution_sandbox::validate_runtime_settings(&policy, &restored, &cwd)
            .unwrap();
        session
            .thread_loaded
            .store(false, std::sync::atomic::Ordering::Release);
        session.state.lock().phase = RuntimePhase::Loading;
        session.context.lock().thread_id = None;
        let epoch = session.context.lock().epoch;
        let expected = restored.clone();
        let transport = crate::async_runtime::spawn(async move {
            let mut peer = BufReader::new(peer);
            let methods: &[&str] = if reported_mode == "workspace-write" {
                &["fixture/initialize", "fixture/open", "fixture/history"]
            } else {
                &["fixture/initialize", "fixture/open"]
            };
            for method in methods {
                let mut line = String::new();
                tokio::time::timeout(Duration::from_secs(2), peer.read_line(&mut line))
                    .await
                    .unwrap()
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], *method);
                let value = if *method == "fixture/open" {
                    let requested: RuntimeSettings =
                        serde_json::from_value(request["params"]["settings"].clone()).unwrap();
                    assert_eq!(requested, expected);
                    serde_json::json!({"settings":RuntimeSettings { mode:Some(reported_mode.into()), ..requested }})
                } else {
                    serde_json::json!({})
                };
                peer.get_mut()
                    .write_all(
                        format!(
                            "{}\n",
                            serde_json::json!({"jsonrpc":"2.0","id":request["id"],"result":value})
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
            peer
        });
        let result = crate::async_runtime::block_on(async {
            let result = ensure_locked(
                svc,
                &tab_id,
                &provider,
                &serde_json::json!({}),
                &session,
                None,
            )
            .await;
            let _peer = transport.await.unwrap();
            result
        });
        if reported_mode == "workspace-write" {
            result.unwrap();
            assert!(session
                .thread_loaded
                .load(std::sync::atomic::Ordering::Acquire));
        } else {
            assert_eq!(
                result.unwrap_err().to_string(),
                "Provider refused: Runtime settings would override the immutable execution sandbox"
            );
            assert!(!session
                .thread_loaded
                .load(std::sync::atomic::Ordering::Acquire));
        }
        let context = session.context.lock().clone();
        assert_eq!(context.settings.mode.as_deref(), Some(reported_mode));
        assert_eq!(
            context.settings.additional_directories,
            policy.writable_roots
        );
        assert_eq!(context.settings.network_access, Some(false));
        assert_eq!(context.cwd, cwd);
        assert_eq!(context.binding_nonce, options.binding_nonce);
        assert_eq!(context.epoch, epoch);
        assert_eq!(context.thread_id.as_deref(), Some("own-thread"));
        let after = svc.mux.read().get_tab(&tab_id).unwrap().clone();
        assert_eq!(after.restorable_launch.execution_sandbox, Some(policy));
        assert_eq!(after.neighbor_authority, Some(authority));
        assert_eq!(
            after.agent_runtime_config.as_ref().unwrap()["mode"],
            "workspace-write"
        );
        assert_eq!(
            after.structured_runtime_mode.as_deref(),
            Some(reported_mode)
        );
        assert!(svc
            .neighbors()
            .require_neighbor_restore_grant(&tab_id, false)
            .is_ok());
        svc.mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .session_binding_nonce = Some("foreign-nonce".into());
        assert!(svc
            .neighbors()
            .require_neighbor_restore_grant(&tab_id, false)
            .is_err());
        svc.mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .session_binding_nonce = after.session_binding_nonce;
        assert!(svc
            .neighbors()
            .require_neighbor_restore_grant(&tab_id, false)
            .is_ok());
        svc.neighbor_identity_changed();
        assert!(svc
            .neighbors()
            .require_neighbor_restore_grant(&tab_id, false)
            .is_err());
    }
}

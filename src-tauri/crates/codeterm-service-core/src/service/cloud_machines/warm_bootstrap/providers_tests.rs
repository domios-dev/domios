use std::sync::Arc;

use codeterm_plugin::agent_runtime::RuntimeLoginMethod;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{TabBacking, TabType};
use crate::service::agent_runtime::{
    authority as runtime_authority,
    engine::test_support::signed_out_runtime,
    login::{self as runtime_login, LoginBinding, LoginRequest},
};
use crate::service::test_fixture::ServiceFixture;

use super::providers::prepare_provider_signin_tabs;
use super::types::{
    PrepareProviderSignInRequest, PrepareProviderSignInResponse, ProviderSignInState,
};

// Production calls prepare from spawn_blocking; tests leave the async worker the same way.
fn prepare_signin_blocking(
    service: &crate::service::ServiceContext,
    provider: &str,
    request: &PrepareProviderSignInRequest,
) -> PrepareProviderSignInResponse {
    tokio::task::block_in_place(|| {
        prepare_provider_signin_tabs(
            service,
            &[provider.into()],
            &[],
            request,
            Some("peer-cloud-1"),
        )
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prepare_provider_signin_tabs_binds_runtime_tab_for_opaque_provider_and_accepts_login() {
    const PROVIDER: &str = "opaque-runtime-login-fixture";
    const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    if (op.kind === 'control' && op.action === 'login') {
      return { action: { kind: 'request', call: { method: 'opaque/control', params: op.value } }, state: null };
    }
    return { action: { kind: 'request', call: { method: 'opaque/init', params: op } }, state: null };
  },
  runtimeResponse(ctx, op, state, reply) {
    if (op.kind === 'control' && op.action === 'login') {
      return {
        kind: 'complete',
        result: { kind: 'done' },
        events: [{
          kind: 'authLogin',
          login: {
            status: 'pending',
            method: 'device',
            loginId: reply.value.loginId,
            url: reply.value.url,
            userCode: reply.value.userCode,
            expiresAtMs: reply.value.expiresAtMs,
          },
        }],
      };
    }
    return { kind: 'complete', result: { kind: 'done' } };
  },
  runtimeReduce(ctx, state, frames) { return { state: null, events: [] }; }
};
"#;

    let (mut mux, _initial) = MultiplexerState::new();
    let ws_id = mux.list_workspaces()[0].id.clone();
    // Create an unbound raw Choice tab to ensure prepare_provider_signin_tabs ignores it.
    let decoy_tab_id = mux.create_tab(&ws_id).expect("decoy tab");
    if let Some(decoy) = mux.get_tab_mut(&decoy_tab_id) {
        decoy.tab_type = TabType::Choice;
        decoy.agent_type = Some(PROVIDER.into());
    }
    let fixture = ServiceFixture::new(mux);

    let manifest = PluginManifest::from_json_str(
        &serde_json::json!({
            "id": PROVIDER,
            "version": "1.0.0",
            "displayName": "Opaque Runtime",
            "capabilities": {
                "agentProvider": true,
                "chatBackend": { "spawnable": true, "sessionConfig": true, "agentTasks": true },
                "agentRuntime": {
                    "runtime": "stdio-jsonrpc",
                    "command": { "binary": "opaque-cli", "args": [] },
                    "login": { "methods": ["browser", "device"] },
                    "requestMethods": {
                        "control": ["opaque/control"],
                        "initialize": ["opaque/init"],
                        "openThread": ["opaque/open"],
                        "resumeThread": ["opaque/resume"],
                        "submitTurn": ["opaque/submit"]
                    },
                    "notificationMethods": []
                }
            },
            "permissions": { "subprocess": { "allow": ["opaque-cli"] } }
        })
        .to_string(),
    )
    .expect("opaque runtime manifest");
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(PROVIDER, SOURCE.into()).expect("opaque vm")),
        )));

    let request = PrepareProviderSignInRequest {
        provider_id: Some(PROVIDER.into()),
        profile_id: None,
    };
    let prepared = prepare_signin_blocking(&fixture.service, PROVIDER, &request);
    assert_eq!(prepared.providers.len(), 1);
    let item = &prepared.providers[0];
    assert!(item.card_ready);
    assert_eq!(item.state, ProviderSignInState::SignInRequired);
    let tab_id = item.tab_id.clone().expect("prepared runtime login tab_id");
    assert_ne!(tab_id, decoy_tab_id);
    assert_eq!(
        item.remote_tab_id.as_deref(),
        Some(format!("mesh:peer-cloud-1/{tab_id}").as_str())
    );

    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .expect("prepared tab in mux")
        .clone();
    assert!(tab.hidden);
    assert!(!tab.show_in_tabbar);
    assert!(tab.skip_snapshot);
    assert!(tab.chat_view);
    assert!(matches!(
        &tab.backing,
        TabBacking::ChatBackend { plugin_id, .. } if plugin_id == PROVIDER
    ));
    assert!(tab.cwd.as_deref().is_some_and(|dir| !dir.trim().is_empty()));
    let nonce = tab
        .session_binding_nonce
        .clone()
        .expect("session binding nonce");
    assert!(runtime_authority::verifies(
        &fixture.service,
        &tab_id,
        PROVIDER,
        &nonce
    ));
    let prepared_json = serde_json::to_string(&prepared).expect("serialize prepared");
    assert!(!prepared_json.contains(&nonce));

    // Repeat call reuses the exact same bound runtime tab.
    let repeat = prepare_signin_blocking(&fixture.service, PROVIDER, &request);
    assert_eq!(repeat.providers[0].tab_id.as_deref(), Some(tab_id.as_str()));

    let adapter = fixture
        .service
        .agent_registry
        .provider_arc(PROVIDER)
        .expect("registered opaque adapter");
    let (peer, _guard) = signed_out_runtime(&tab, adapter);
    let (peer_read, mut peer_write) = tokio::io::split(peer);
    let responder = tokio::spawn(async move {
        let mut lines = BufReader::new(peer_read).lines();
        let line = lines
            .next_line()
            .await
            .expect("read rpc request")
            .expect("rpc request line");
        let req: serde_json::Value = serde_json::from_str(&line).expect("parse rpc request");
        assert_eq!(req["method"], "opaque/control");
        assert_eq!(req["params"]["action"], "start");
        assert_eq!(req["params"]["method"], "device");
        let reply = serde_json::json!({
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": {
                "loginId": "opaque-login-1",
                "url": "https://example.invalid/opaque-device",
                "userCode": "OPAQ-9942",
                "expiresAtMs": 4_102_444_800_000_u64
            }
        });
        peer_write
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .expect("write rpc reply");
        peer_write.flush().await.expect("flush rpc reply");
    });

    let reply = runtime_login::operate(
        &fixture.service,
        &tab_id,
        LoginRequest {
            binding: None,
            login_id: None,
            action: None,
            owner_confirmed: false,
            provider_id: Some(PROVIDER.into()),
            account: None,
            code: None,
            method_id: None,
            method: Some(RuntimeLoginMethod::Device),
            api_key: None,
        },
    )
    .await
    .expect("device login must succeed on prepared runtime tab");
    responder.await.expect("responder joined");

    assert_eq!(reply["login"]["status"], "pending");
    assert_eq!(reply["login"]["userCode"], "OPAQ-9942");
    assert_eq!(
        reply["login"]["url"],
        "https://example.invalid/opaque-device"
    );
    assert_eq!(reply["loginBinding"]["nonce"], nonce);
    assert_eq!(reply["loginBinding"]["tabId"], tab_id);
    assert_eq!(reply["loginBinding"]["providerId"], PROVIDER);

    // Keystone flow-bound LoginBinding security: mismatched nonce is rejected.
    let bad_binding = runtime_login::operate(
        &fixture.service,
        &tab_id,
        LoginRequest {
            binding: Some(LoginBinding {
                installation_id: None,
                tab_id: Some(tab_id.clone()),
                provider_id: Some(PROVIDER.into()),
                session_id: None,
                nonce: "foreign-binding-nonce".into(),
                epoch: 1,
            }),
            login_id: Some("opaque-login-1".into()),
            action: Some("cancel".into()),
            owner_confirmed: false,
            provider_id: Some(PROVIDER.into()),
            account: None,
            code: None,
            method_id: None,
            method: None,
            api_key: None,
        },
    )
    .await;
    assert!(bad_binding.is_err());
    drop(_guard);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prepare_stop_prepare_reuses_single_dedicated_signin_tab() {
    const PROVIDER: &str = "opaque-runtime-stop-fixture";
    const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    return { action: { kind: 'request', call: { method: 'opaque/init', params: op } }, state: null };
  },
  runtimeResponse(ctx, op, state, reply) {
    return { kind: 'complete', result: { kind: 'done' } };
  },
  runtimeReduce(ctx, state, frames) { return { state: null, events: [] }; }
};
"#;

    let (mux, _initial) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let manifest = PluginManifest::from_json_str(
        &serde_json::json!({
            "id": PROVIDER,
            "version": "1.0.0",
            "displayName": "Opaque Stop Fixture",
            "capabilities": {
                "agentProvider": true,
                "chatBackend": { "spawnable": true, "sessionConfig": true, "agentTasks": true },
                "agentRuntime": {
                    "runtime": "stdio-jsonrpc",
                    "command": { "binary": "opaque-cli", "args": [] },
                    "login": { "methods": ["browser", "device"] },
                    "requestMethods": {
                        "control": ["opaque/control"],
                        "initialize": ["opaque/init"],
                        "openThread": ["opaque/open"],
                        "resumeThread": ["opaque/resume"],
                        "submitTurn": ["opaque/submit"]
                    },
                    "notificationMethods": []
                }
            },
            "permissions": { "subprocess": { "allow": ["opaque-cli"] } }
        })
        .to_string(),
    )
    .expect("manifest");
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(PROVIDER, SOURCE.into()).expect("vm")),
        )));

    let request = PrepareProviderSignInRequest {
        provider_id: Some(PROVIDER.into()),
        profile_id: None,
    };
    let first = prepare_signin_blocking(&fixture.service, PROVIDER, &request);
    let tab_id_1 = first.providers[0].tab_id.clone().expect("first tab_id");
    let tab_1 = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id_1)
        .expect("first tab")
        .clone();
    let nonce_1 = tab_1.session_binding_nonce.clone().expect("first nonce");
    assert!(runtime_authority::verifies(
        &fixture.service,
        &tab_id_1,
        PROVIDER,
        &nonce_1
    ));

    // Attach a live runtime and then stop it so authority is revoked.
    let adapter = fixture
        .service
        .agent_registry
        .provider_arc(PROVIDER)
        .expect("adapter");
    let (_peer, _guard) = signed_out_runtime(&tab_1, adapter);
    crate::service::agent_runtime::engine::stop(&fixture.service, &tab_id_1)
        .await
        .expect("stop runtime");
    assert!(!runtime_authority::verifies(
        &fixture.service,
        &tab_id_1,
        PROVIDER,
        &nonce_1
    ));

    // Re-preparing must reuse the single dedicated sign-in tab with a rotated nonce and fresh authority.
    let second = prepare_signin_blocking(&fixture.service, PROVIDER, &request);
    let tab_id_2 = second.providers[0].tab_id.clone().expect("second tab_id");
    assert_eq!(tab_id_2, tab_id_1);

    let (dedicated_count, nonce_2) = {
        let mux = fixture.service.mux.read();
        let count = mux
            .all_tabs()
            .filter(|tab| tab.description.as_deref() == Some("domios:cloud-provider-sign-in"))
            .count();
        let nonce = mux
            .get_tab(&tab_id_2)
            .and_then(|tab| tab.session_binding_nonce.clone())
            .expect("rotated nonce");
        (count, nonce)
    };
    assert_eq!(dedicated_count, 1);
    assert_ne!(nonce_2, nonce_1);
    assert!(runtime_authority::verifies(
        &fixture.service,
        &tab_id_2,
        PROVIDER,
        &nonce_2
    ));
}

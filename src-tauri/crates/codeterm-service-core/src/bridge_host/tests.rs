use super::*;
use crate::agent::provider::AgentProvider;
use crate::multiplexer::{state::MultiplexerState, types::TabBacking};
use crate::service::agent_runtime::engine::test_support::loaded_runtime;
use crate::service::chat_send_transaction::ChatSendState;
use crate::service::test_fixture::ServiceFixture;
use codeterm_bridge::bridge::traits::{InboundMessage, ReplyContext};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn fixture() -> (ServiceFixture, String) {
    let provider = runtime_adapter().id().to_string();
    let mut mux = MultiplexerState::empty();
    general_agent::ensure_general_workspace(&mut mux, false);
    let id = mux.resolve_tab_id(general_agent::GENERAL_ALIAS).unwrap();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some(provider.clone());
    tab.agent_session_id = Some("exact-session".into());
    tab.session_binding_nonce = Some("exact-nonce".into());
    tab.reported_session_binding_nonce = Some("exact-nonce".into());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: provider.clone(),
        session_id: "exact-session".into(),
    };
    tab.cwd = Some(
        crate::test_support::fixture_manifest_dir()
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    let fixture = ServiceFixture::new(mux);
    fixture.set_agent_full_access(false);
    (fixture, id)
}

fn runtime_adapter() -> codeterm_plugin::js_runtime::JsProvider {
    let root = crate::test_support::fixture_manifest_dir().join("../../..");
    let fixture: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("tests/fixtures/agent-runtime/bridge-provider.json"))
            .unwrap(),
    )
    .unwrap();
    codeterm_plugin::js_runtime::load_one(&root.join(fixture["manifest"].as_str().unwrap()))
        .expect("genuine bundled runtime adapter")
}

fn register_runtime(svc: &ServiceContext) {
    let adapter = runtime_adapter();
    let provider = adapter.id().to_string();
    svc.agent_registry.register_provider(Box::new(adapter));
    let launched: Vec<_> = svc
        .mux
        .read()
        .all_tabs()
        .filter(|tab| tab.agent_type.as_deref() == Some(provider.as_str()))
        .filter_map(|tab| Some((tab.id.clone(), tab.session_binding_nonce.clone()?)))
        .collect();
    for (tab, nonce) in launched {
        crate::service::agent_runtime::authority::record_spawn_for_test(svc, &tab, &provider, &nonce);
    }
}
fn message() -> InboundMessage {
    InboundMessage {
        service: "telegram".into(),
        user_id: 101,
        user_name: "fixture".into(),
        text: "opaque-bridge-body".into(),
        attachments: Vec::new(),
        message_id: Some(202),
        callback_data: None,
        reply_ctx: ReplyContext::Telegram {
            chat_id: 101,
            thread_id: None,
            reply_to_message_id: None,
        },
    }
}

#[test]
fn bridge_main_and_local_send_share_refreshed_runtime_exact_session() {
    let (fixture, id) = fixture();
    let provider = fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .agent_type
        .clone()
        .unwrap();
    register_runtime(&fixture.service);
    let before = fixture
        .service
        .agent_registry
        .provider_arc(&provider)
        .unwrap();
    register_runtime(&fixture.service);
    let adapter = fixture
        .service
        .agent_registry
        .provider_arc(&provider)
        .unwrap();
    assert!(!Arc::ptr_eq(&before, &adapter));
    assert!(fixture
        .service
        .chatbackend_registry
        .get(&provider)
        .is_none());
    let tab = fixture.service.mux.read().get_tab(&id).unwrap().clone();
    let mut runtime = loaded_runtime(&tab, Some(adapter));
    let session = crate::service::agent_runtime::engine::for_tab(&id).unwrap();
    codeterm_plugin::agent_runtime::validate_runtime_context(&session.context.lock()).unwrap();
    let port: Arc<dyn BridgeTabPort> = Arc::new(ServiceBridgeTabPort(fixture.service.clone()));
    crate::async_runtime::block_on(async {
        let mut peer = BufReader::new(&mut runtime.peer);
        let receive = async {
            let mut ids = Vec::new();
            for expected in ["opaque-bridge-body", "opaque-local-body"] {
                let mut line = String::new();
                tokio::time::timeout(std::time::Duration::from_secs(5), peer.read_line(&mut line))
                    .await
                    .unwrap()
                    .unwrap();
                let request: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], "turn/start");
                assert_eq!(request["params"]["threadId"], "exact-session");
                assert!(request["params"]["input"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains(expected));
                let send = request["params"]["clientUserMessageId"]
                    .as_str()
                    .unwrap()
                    .to_string();
                assert!(!ids.contains(&send));
                ids.push(send);
                let response = format!("{}\n", json!({"id":request["id"],"result":{}}));
                peer.get_mut().write_all(response.as_bytes()).await.unwrap();
            }
            ids
        };
        let send = async {
            codeterm_bridge::bridge::manager::inject_message(
                &port,
                &message(),
                &VoiceTranscribeConfig::default(),
                &TextHooksConfig::default(),
            )
            .await
            .unwrap();
            let svc = fixture.service.clone();
            let target = id.clone();
            tokio::task::spawn_blocking(move || {
                svc.chatbackend_tab_send(&target, "opaque-local-body")
            })
            .await
            .unwrap()
            .unwrap();
        };
        let (ids, ()) = tokio::join!(receive, send);
        for send in ids {
            let ledger = &fixture.service.chat_send_ledger;
            assert_eq!(ledger.status(&send).unwrap().state, ChatSendState::Written);
            assert!(ledger
                .confirm_rpc_send(&id, "wrong-session", "exact-nonce", &send, Some("item"))
                .is_none());
            assert!(ledger
                .confirm_rpc_send(&id, "exact-session", "exact-nonce", &send, Some(&send))
                .is_some());
            assert_eq!(ledger.status(&send).unwrap().state, ChatSendState::Sent);
        }
    });
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .pty_id
        .is_none());
}

#[test]
fn bridge_removed_provider_is_typed_and_never_uses_a_pty() {
    let (fixture, id) = fixture();
    let provider = fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .agent_type
        .clone()
        .unwrap();
    register_runtime(&fixture.service);
    assert!(fixture
        .service
        .agent_registry
        .unregister_provider(&provider));
    let port: Arc<dyn BridgeTabPort> = Arc::new(ServiceBridgeTabPort(fixture.service.clone()));
    let error = crate::async_runtime::block_on(codeterm_bridge::bridge::manager::inject_message(
        &port,
        &message(),
        &VoiceTranscribeConfig::default(),
        &TextHooksConfig::default(),
    ))
    .unwrap_err();
    let error: Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["code"], "chat_backend_plugin_removed");
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .pty_id
        .is_none());
}

#[test]
fn bridge_runtime_rpc_failure_retains_uncertainty_without_replay_or_pty() {
    use std::future::Future;
    use std::task::Poll;

    let (fixture, id) = fixture();
    register_runtime(&fixture.service);
    let tab = fixture.service.mux.read().get_tab(&id).unwrap().clone();
    let adapter = fixture
        .service
        .agent_registry
        .provider_arc(tab.agent_type.as_deref().unwrap())
        .unwrap();
    let mut runtime = loaded_runtime(&tab, Some(adapter));
    let port: Arc<dyn BridgeTabPort> = Arc::new(ServiceBridgeTabPort(fixture.service.clone()));
    crate::async_runtime::block_on(async {
        let mut peer = BufReader::new(&mut runtime.peer);
        let receive = async {
            let mut line = String::new();
            tokio::time::timeout(std::time::Duration::from_secs(5), peer.read_line(&mut line))
                .await
                .unwrap()
                .unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "turn/start");
            assert_eq!(request["params"]["threadId"], "exact-session");
            let send_id = request["params"]["clientUserMessageId"]
                .as_str()
                .unwrap()
                .to_owned();
            let response =
                json!({"id":request["id"],"error":{"code":-32603,"message":"opaque-failure"}});
            peer.get_mut()
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
            send_id
        };
        let voice = VoiceTranscribeConfig::default();
        let hooks = TextHooksConfig::default();
        let inbound = message();
        let inject =
            codeterm_bridge::bridge::manager::inject_message(&port, &inbound, &voice, &hooks);
        let (send_id, result) = tokio::join!(receive, inject);
        let error =
            crate::service::delivery_error::DeliveryError::from_opaque(&id, result.unwrap_err());
        assert_eq!(error.code(), "submit_unconfirmed");
        assert!(error.bytes_written());
        let ledger = &fixture.service.chat_send_ledger;
        assert_eq!(
            ledger.status(&send_id).unwrap().state,
            ChatSendState::Unknown
        );
        assert!(ledger.has_rpc_handoff(&send_id));
        assert_eq!(ledger.reconcile(&id).len(), 1);
        assert!(codeterm_bridge::bridge::inbound_buffer::drain(&id).is_empty());
        let mut extra = String::new();
        let mut read = std::pin::pin!(peer.read_line(&mut extra));
        let additional_rpc = std::future::poll_fn(|cx| {
            Poll::Ready(matches!(read.as_mut().poll(cx), Poll::Ready(_)))
        })
        .await;
        assert!(!additional_rpc);
    });
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .pty_id
        .is_none());
}

#[test]
fn bridge_disappeared_target_is_typed_instead_of_buffered() {
    let (fixture, _) = fixture();
    let port = ServiceBridgeTabPort(fixture.service.clone());
    let error = port
        .deliver_inbound("closed-tab", "opaque-body", None)
        .unwrap_err();
    let error: Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["code"], "bridge_target_not_found");
    assert!(codeterm_bridge::bridge::inbound_buffer::drain("closed-tab").is_empty());
}

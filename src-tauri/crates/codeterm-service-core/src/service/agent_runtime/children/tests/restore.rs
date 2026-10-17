use super::*;
use crate::multiplexer::{state::MultiplexerState, types::TabBacking};
use crate::service::test_fixture::ServiceFixture;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[test]
fn restored_epoch_reads_child_from_own_parent_history() {
    let provider_id = format!("child-restore-{}", uuid::Uuid::new_v4());
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some(provider_id.clone());
    tab.agent_session_id = Some("root".into());
    tab.session_binding_nonce = Some("restored".into());
    tab.reported_session_binding_nonce = Some("restored".into());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: provider_id.clone(),
        session_id: "root".into(),
    };
    let tab = tab.clone();
    let fixture = ServiceFixture::new(mux);
    let manifest = PluginManifest::from_json_str(&json!({
        "id":provider_id,"version":"1.0.0","displayName":"Fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
        "agentRuntime":{"runtime":"stdio-jsonrpc","command":{"binary":"fixture-cli","args":[]},
        "requestMethods":{"readHistory":["fixture/history"]},"notificationMethods":[]}},
        "permissions":{"subprocess":{"allow":["fixture-cli"]}}
    }).to_string()).unwrap();
    let source = r#"
module.exports = {
  runtimePlan(ctx, op) {
    return {action:{kind:'request',call:{method:'fixture/history',params:op,target:{threadId:op.threadId}}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) { return {kind:'complete',result:reply.value}; },
  runtimeReduce(ctx, state, frames) { return {state:null,events:[]}; }
};
"#;
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(&provider_id, source.into()).unwrap()),
        )));
    let adapter = fixture
        .service
        .agent_registry
        .provider_arc(&provider_id)
        .unwrap();
    let mut runtime = engine::test_support::loaded_runtime(&tab, Some(adapter));
    let session = engine::for_tab(&tab_id).unwrap();
    let epoch = session.state.lock().begin();
    assert_eq!(epoch, 2);
    assert!(session.state.lock().load(epoch, "root"));
    session.context.lock().epoch = epoch;
    assert!(session.maps.lock().child(epoch, "child").is_none());
    let page = crate::async_runtime::block_on(async {
        let peer = async {
            let (reader, mut writer) = tokio::io::split(&mut runtime.peer);
            let mut lines = BufReader::new(reader).lines();
            for thread in ["root", "child"] {
                let call: serde_json::Value =
                    serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
                assert_eq!(call["method"], "fixture/history");
                assert_eq!(call["params"]["threadId"], thread);
                let items = if thread == "root" {
                    json!([{"id":"spawn","type":"tool_call","content":"","toolKind":"agent","toolName":"agent_activity","toolInput":{"agent_id":"child","agent_parent_thread_id":"root","agent_ordinal":1,"agent_state":"started","agent_binding_nonce":"previous","agent_runtime_epoch":1}}])
                } else {
                    json!([{"id":"answer","type":"assistant","content":"persisted child answer"}])
                };
                let result = RuntimeResult::HistoryPage {
                    thread_id: thread.into(),
                    items: serde_json::from_value(items).unwrap(),
                    next_cursor: None,
                    open_items: Vec::new(),
                    questions: None,
                    order: Some(HistoryOrder::Desc),
                };
                let reply = json!({"jsonrpc":"2.0","id":call["id"],"result":result});
                writer
                    .write_all(format!("{reply}\n").as_bytes())
                    .await
                    .unwrap();
            }
            Ok::<(), RuntimeFailure>(())
        };
        let read = history_inner(
            &fixture.service,
            &tab_id,
            "child",
            ChildHistoryRequest {
                root_thread_id: "root".into(),
                binding_nonce: "restored".into(),
                epoch,
                cursor: None,
            },
        );
        let (page, ()) = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::try_join!(read, peer)
        })
        .await
        .expect("parent and child history RPCs must complete")
        .expect("restored child history read must succeed");
        page
    });
    assert_eq!(page.epoch, 2);
    assert_eq!(page.binding_nonce, "restored");
    assert_eq!(page.child.status, "unknown");
    assert_eq!(page.items[0].content, "persisted child answer");
    assert!(session.context.lock().active_turns.is_empty());
}

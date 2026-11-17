use super::*;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};

fn adapter(completion_events: bool, source: &str) -> Arc<JsProvider> {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tests/fixtures/runtime-http-sse.json"
    )))
    .unwrap();
    let capability = fixture["capability"].clone();
    let mut manifest = json!({"id":"sync-send-fixture","version":"1.0.0","displayName":"Fixture","capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},"agentRuntime":capability},"permissions":{"subprocess":{"allow":[capability["command"]["binary"]]}}});
    manifest["capabilities"]["agentRuntime"]["httpSse"]["completionEvents"] =
        json!(completion_events);
    for alternative in manifest["capabilities"]["agentRuntime"]["httpSse"]["alternatives"]
        .as_array_mut()
        .unwrap()
    {
        alternative["completionEvents"] = json!(completion_events);
    }
    let manifest = PluginManifest::from_json_str(&manifest.to_string()).unwrap();
    Arc::new(JsProvider::new(
        manifest,
        Arc::new(PluginVm::from_source("sync-send-fixture", source.into()).unwrap()),
    ))
}

#[test]
fn acknowledged_http_requests_keep_event_completed_turns_interruptible() {
    for completion_events in [false, true] {
        let (_fixture, tab, _peer, _cleanup) = runtime_fixture_turn_scoped(SOURCE);
        let session = engine::for_tab(&tab).unwrap();
        let epoch = session.context.lock().epoch;
        *session.adapter.lock() = Some(adapter(completion_events, SOURCE));
        assert!(!waiting(&session, epoch));
        assert_eq!(no_provider_turn(&session, epoch), !completion_events);
    }
}

const ACK_SOURCE: &str = r#"
module.exports = {
 runtimePlan(ctx,op) {return {action:{kind:'request',call:{method:'v2.session.prompt',params:{},target:{threadId:op.threadId,turnId:'turn'}}},state:null};},
 runtimeResponse(ctx,op,state,reply) {return {kind:'complete',result:{kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:'turn'}};},
 runtimeReduce(ctx,state,frames) {return {state,events:[]};}
};
"#;

#[tokio::test]
async fn http_ack_keeps_written_evidence_window_until_own_completion_event() {
    let (fixture, tab, peer, _cleanup) = runtime_fixture_turn_scoped(ACK_SOURCE);
    let session = engine::for_tab(&tab).unwrap();
    *session.adapter.lock() = Some(adapter(true, ACK_SOURCE));
    let process = session.process.lock().clone().unwrap();
    let op = operation(&session.context.lock());
    let submit = async {
        let (result, written) = engine::run_submit(&fixture.service, &session, &process, op).await;
        assert!(written);
        assert!(matches!(
            result,
            Ok(RuntimeResult::TurnAccepted { .. }) | Err(RuntimeFailure::WrittenAwaitingEvidence)
        ));
    };
    let server = async {
        let mut changes = session.control_changes.subscribe();
        let mut peer = BufReader::new(peer);
        let mut line = String::new();
        peer.read_line(&mut line).await.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        peer.get_mut()
            .write_all(format!("{{\"id\":{},\"result\":{{}}}}\n", request["id"]).as_bytes())
            .await
            .unwrap();
        loop {
            changes.borrow_and_update();
            let acknowledged = session
                .turn_requests
                .lock()
                .get("send")
                .is_some_and(|pending| pending.outcome.borrow().is_some());
            if acknowledged {
                break;
            }
            changes.changed().await.unwrap();
        }
        let epoch = session.context.lock().epoch;
        assert!(waiting(&session, epoch));
        assert!(!no_provider_turn(&session, epoch));
        let window = stream_window(&session, vec![1]).await;
        assert_eq!(window.turn_requests.len(), 1);
        assert!(window.turn_requests[0].written);
        accept(&fixture.service, &session).await;
        let thread = session.context.lock().thread_id.clone().unwrap();
        subscriber::publish_events(
            &fixture.service,
            &session,
            epoch,
            vec![AgentRuntimeEvent::TurnState {
                thread_id: thread,
                turn_id: Some("turn".into()),
                state: RuntimeTurnState::Idle,
                detail: None,
                status_line: None,
                started_at: None,
                completed_at: Some(now()),
                parent_thread_id: None,
            }],
            now(),
        )
        .await
        .unwrap();
        completed(&session).await;
        assert!(!waiting(&session, epoch));
        peer
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        let ((), _peer) = tokio::join!(submit, server);
    })
    .await
    .unwrap();
}

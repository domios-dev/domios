use super::batch::{drain_ready, handle, Drained, Poll};
use super::bookkeeping::{self, Step};
use super::*;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use tokio::io::BufReader;

fn delta(seq: usize) -> RuntimeFrame {
    RuntimeFrame::Notification {
        method: "fixture/compacted".into(),
        params: json!({ "seq": seq }),
    }
}

#[test]
fn resumed_replay_stays_idle_and_first_new_turn_completes_once() {
    crate::async_runtime::block_on(async {
        let scene = fixtures::scene();
        let fixture: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../plugins/provider-acp/tests/fixtures/runtime/resume-buffered-replay-new-turn.json"
        )))
        .unwrap();
        let events: Vec<AgentRuntimeEvent> =
            serde_json::from_value(fixture["expected"]["events"].clone()).unwrap();
        // The durable row is restored by the host; replay no longer re-emits it once history exists.
        let restored = serde_json::json!({"kind":"chatItem","threadId":"thread","item":{
            "id":"thread:turn:0:item:2","seq":2,"type":"assistant","content":"old reply"}});
        let AgentRuntimeEvent::ChatItem { item: original, .. } =
            serde_json::from_value::<AgentRuntimeEvent>(restored).unwrap()
        else {
            unreachable!("fixture row is a chat item")
        };
        let epoch = {
            let mut state = scene.session.state.lock();
            let epoch = state.begin();
            state.load(epoch, "thread");
            epoch
        };
        scene.session.context.lock().epoch = epoch;
        let context = scene.session.context.lock().clone();
        assert!(scene.session.maps.lock().prepare_restore(&context));
        super::super::transcript::apply_restored_rows(
            &scene.session.cache,
            &scene.session.maps,
            &context,
            vec![original.clone()],
            || true,
        )
        .unwrap();
        let boundary = events
            .iter()
            .position(|event| {
                matches!(event,
                    AgentRuntimeEvent::ChatItem { item, .. } if item.id == "thread:turn:8:item:1"
                )
            })
            .unwrap();
        publish_events(
            &scene.fixture.service,
            &scene.session,
            epoch,
            events[..boundary].to_vec(),
            10,
        )
        .await
        .unwrap();
        assert!(scene.session.context.lock().active_turn_id.is_none());
        assert!(scene
            .session
            .state
            .lock()
            .active_turn_started_at_ms
            .is_none());
        scene.session.context.lock().claimed_sends =
            serde_json::from_value(fixture["operations"][2]["context"]["claimedSends"].clone())
                .unwrap();
        publish_events(
            &scene.fixture.service,
            &scene.session,
            epoch,
            events[boundary..].to_vec(),
            20,
        )
        .await
        .unwrap();
        assert!(scene.session.context.lock().active_turn_id.is_none());
        assert!(scene.session.context.lock().active_turns.is_empty());
        assert!(scene
            .session
            .state
            .lock()
            .active_turn_started_at_ms
            .is_none());
        assert!(scene.session.state.lock().notice.is_none());
        let live = serde_json::to_value(
            &scene
                .fixture
                .service
                .mux
                .read()
                .get_tab(&context.tab_id)
                .unwrap()
                .agent_state,
        )
        .unwrap();
        assert_eq!(live["state"], "idle");
        assert!(scene
            .session
            .completions
            .lock()
            .contains("thread", "thread:turn:8"));
        let rows = scene.session.cache.lock().snapshot();
        assert_eq!(
            rows.iter()
                .filter(|row| row.kind == "assistant" && row.content == "NEW-TURN-OK")
                .count(),
            1
        );
        assert_eq!(rows.iter().filter(|row| row.id == original.id).count(), 1);
        publish_events(
            &scene.fixture.service,
            &scene.session,
            epoch,
            vec![AgentRuntimeEvent::ChatItemAppend {
                thread_id: "thread".into(),
                item_id: original.id.clone(),
                content: "late".into(),
                offset: original.content.len(),
            }],
            30,
        )
        .await
        .unwrap();
        assert!(scene
            .session
            .state
            .lock()
            .notice
            .as_deref()
            .unwrap()
            .contains("retained content or active turn"));
        assert_eq!(
            scene
                .session
                .cache
                .lock()
                .snapshot()
                .iter()
                .find(|row| row.id == original.id)
                .unwrap()
                .content,
            "old reply"
        );
    });
}

fn request(id: &str) -> RuntimeFrame {
    RuntimeFrame::Request {
        request_id: RpcId::String(id.into()),
        method: "fixture/known".into(),
        params: json!({}),
    }
}

async fn answer_resync(peer: &mut BufReader<tokio::io::DuplexStream>) -> Vec<Value> {
    let mut other = Vec::new();
    for method in ["thread/resume", "thread/read"] {
        loop {
            let line = fixtures::read(peer).await;
            if line["method"] == method {
                fixtures::write(peer, json!({"id": line["id"], "result": {}})).await;
                break;
            }
            other.push(line);
        }
    }
    other
}

async fn run(
    scene: &mut fixtures::Scene,
    frames: Vec<RuntimeFrame>,
    lagged: bool,
    resync: bool,
) -> (Step, Vec<Value>) {
    let process = scene.session.process.lock().clone().unwrap();
    let epoch = scene.session.context.lock().epoch;
    let svc = scene.fixture.service.clone();
    let session = scene.session.clone();
    let sequences = (1..=frames.len() as u64).collect();
    let work = handle(&svc, &session, &process, epoch, frames, sequences, lagged);
    if resync {
        tokio::join!(work, answer_resync(&mut scene.peer))
    } else {
        (work.await, Vec::new())
    }
}

fn polls(count: usize, tail: Poll<usize>) -> impl FnMut() -> Poll<usize> {
    let mut next = 1;
    let mut tail = Some(tail);
    move || {
        if next < count {
            next += 1;
            Poll::Frame(next - 1)
        } else {
            tail.take().unwrap_or(Poll::Empty)
        }
    }
}

#[test]
fn drain_takes_every_ready_frame_in_order_and_stops_at_empty() {
    let Drained::Ready { frames, lagged } =
        drain_ready(Poll::Frame(0), polls(5, Poll::Empty), 1024)
    else {
        panic!("expected ready")
    };
    assert_eq!(frames, vec![0, 1, 2, 3, 4]);
    assert!(!lagged);
}

#[test]
fn drain_respects_the_batch_cap_and_reports_lag_and_close() {
    let Drained::Ready { frames, .. } = drain_ready(Poll::Frame(0), polls(100, Poll::Empty), 8)
    else {
        panic!("expected ready")
    };
    assert_eq!(frames.len(), 8);
    let Drained::Ready { frames, lagged } = drain_ready(Poll::Frame(0), polls(3, Poll::Lagged), 8)
    else {
        panic!("expected ready")
    };
    assert_eq!((frames.len(), lagged), (3, true));
    let Drained::Closed { frames } = drain_ready(Poll::Frame(0), polls(3, Poll::Closed), 8) else {
        panic!("expected closed")
    };
    assert_eq!(frames.len(), 3);
}

#[tokio::test]
async fn one_drained_batch_is_one_reduce_with_frames_in_order() {
    let mut scene = fixtures::scene();
    let frames: Vec<_> = (0..40).map(delta).collect();
    let (step, _) = run(&mut scene, frames.clone(), false, false).await;
    assert!(matches!(step, Step::Continue));
    let calls = scene.probe.reduces.lock();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0], frames);
}

#[tokio::test]
async fn unmapped_server_requests_reply_in_arrival_order() {
    let mut scene = fixtures::scene();
    let frames = vec![request("a"), delta(0), request("b"), request("c")];
    let (step, _) = run(&mut scene, frames, false, false).await;
    assert!(matches!(step, Step::Continue));
    assert_eq!(scene.probe.reduces.lock().len(), 1);
    for expected in ["a", "b", "c"] {
        let reply = fixtures::read(&mut scene.peer).await;
        assert_eq!(reply["id"], expected);
        assert_eq!(reply["error"]["code"], -32601);
    }
    assert!(scene.session.prompts.lock().is_empty());
}

#[tokio::test]
async fn reducer_failure_warns_resyncs_and_keeps_hook_state() {
    let mut scene = fixtures::scene();
    *scene.session.hook_state.lock() = json!({"keep": 1});
    scene.probe.fail_next.store(1, Ordering::SeqCst);
    let (step, replies) = run(&mut scene, vec![request("r"), delta(0)], false, true).await;
    assert!(matches!(step, Step::Continue));
    let reply = &replies[0];
    assert_eq!(reply["id"], "r");
    assert_eq!(reply["error"]["code"], -32602);
    assert_eq!(*scene.session.hook_state.lock(), json!({"keep": 1}));
    assert_eq!(
        scene
            .session
            .provider_operation_failures
            .load(Ordering::Relaxed),
        0
    );
    assert!(scene
        .session
        .bookkeeping_warning_emitted
        .load(Ordering::Relaxed));
    assert!(scene.session.process.lock().is_some());
}

#[tokio::test]
async fn a_successful_batch_preserves_epoch_warning_deduplication() {
    let mut scene = fixtures::scene();
    scene.probe.fail_next.store(1, Ordering::SeqCst);
    run(&mut scene, vec![delta(0)], false, true).await;
    assert_eq!(
        scene
            .session
            .provider_operation_failures
            .load(Ordering::Relaxed),
        0
    );
    let (step, _) = run(&mut scene, vec![delta(1)], false, false).await;
    assert!(matches!(step, Step::Continue));
    assert_eq!(
        scene
            .session
            .provider_operation_failures
            .load(Ordering::Relaxed),
        0
    );
}

#[tokio::test]
async fn repeated_reducer_failures_warn_once_and_keep_the_process() {
    let mut scene = fixtures::scene();
    scene.probe.fail_next.store(16, Ordering::SeqCst);
    for seq in 0..16 {
        let (step, _) = run(&mut scene, vec![delta(seq)], false, true).await;
        assert!(matches!(step, Step::Continue));
    }
    assert_eq!(scene.probe.reduce_failures.load(Ordering::SeqCst), 16);
    assert_eq!(
        scene
            .session
            .provider_operation_failures
            .load(Ordering::Relaxed),
        0
    );
    let rows = scene.session.cache.lock().snapshot();
    assert_eq!(
        rows.iter()
            .filter(|row| row.system_subtype.as_deref() == Some("runtime_warning"))
            .count(),
        1
    );
    let process = scene.session.process.lock().clone().unwrap();
    assert!(process.client.ended().borrow().is_none());
}

#[test]
fn resolution_resyncs_bookkeeping_and_stops_after_real_provider_failures() {
    for reason in [bookkeeping::Reason::Lag, bookkeeping::Reason::Hook] {
        assert_eq!(
            bookkeeping::resolve(bookkeeping::RecoveryEvent::Bookkeeping(reason), 7),
            bookkeeping::Resolution::Resync
        );
    }

    let mut failures = 0;
    for expected in 1..=8 {
        let resolution = bookkeeping::resolve(
            bookkeeping::RecoveryEvent::ProviderOperation(Err(RuntimeFailure::HookFailed(
                "provider operation failed".into(),
            ))),
            failures,
        );
        if expected < 8 {
            assert_eq!(
                resolution,
                bookkeeping::Resolution::Continue {
                    provider_failures: expected
                }
            );
            failures = expected;
        } else {
            assert!(matches!(resolution, bookkeeping::Resolution::Close(_)));
        }
    }

    assert_eq!(
        bookkeeping::resolve(bookkeeping::RecoveryEvent::ProviderOperation(Ok(())), 7),
        bookkeeping::Resolution::Continue {
            provider_failures: 0
        }
    );
    assert_eq!(
        bookkeeping::resolve(
            bookkeeping::RecoveryEvent::ProviderOperation(Err(RuntimeFailure::PipeEof)),
            0
        ),
        bookkeeping::Resolution::Close(RuntimeFailure::PipeEof)
    );
    assert_eq!(
        bookkeeping::resolve(
            bookkeeping::RecoveryEvent::ProviderOperation(Err(RuntimeFailure::Superseded)),
            0
        ),
        bookkeeping::Resolution::Break
    );
}

#[tokio::test]
async fn lagged_subscriber_resyncs_and_keeps_runtime_alive() {
    let mut scene = fixtures::scene();
    let (step, _) = run(&mut scene, Vec::new(), true, true).await;
    assert!(matches!(step, Step::Continue));
    assert!(scene.probe.reduces.lock().is_empty());
    assert_eq!(
        scene
            .session
            .provider_operation_failures
            .load(Ordering::Relaxed),
        0
    );
    let process = scene.session.process.lock().clone().unwrap();
    assert!(process.client.ended().borrow().is_none());
}

#[test]
fn warning_ids_are_unique_inside_one_epoch() {
    assert_ne!(bookkeeping::warning_id(3), bookkeeping::warning_id(3));
}

#[tokio::test]
async fn bookkeeping_resync_requests_ascending_history_and_lands_it_in_order() {
    let mut scene = fixtures::scene();
    *scene.probe.history.lock() = ["a", "b", "c"].map(String::from).to_vec();
    let process = scene.session.process.lock().clone().unwrap();
    let svc = scene.fixture.service.clone();
    let session = scene.session.clone();
    let answer = async {
        for _ in 0..4 {
            let line = fixtures::read(&mut scene.peer).await;
            fixtures::write(&mut scene.peer, json!({"id": line["id"], "result": {}})).await;
        }
    };
    let (result, ()) = tokio::join!(bookkeeping::resync(&svc, &session, &process), answer);
    result.unwrap();
    let ids: Vec<_> = session
        .cache
        .lock()
        .snapshot()
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(ids, ["a", "b", "c"]);
    assert_eq!(
        *scene.probe.history_orders.lock(),
        [Some(HistoryOrder::Asc); 3]
    );
}

#[test]
fn non_bridge_provider_native_named_request_keeps_existing_batch_behavior() {
    crate::async_runtime::block_on(async {
        let mut scene = fixtures::scene();
        let frame = RuntimeFrame::Request {
            request_id: RpcId::String("opaque-native-name".into()),
            method: "fs/write_text_file".into(),
            params: json!({"sessionId":"thread","path":"/outside","content":"opaque"}),
        };
        let (step, _) = run(&mut scene, vec![frame.clone()], false, false).await;
        assert!(matches!(step, Step::Continue));
        assert_eq!(*scene.probe.reduces.lock(), vec![vec![frame]]);
        let response = fixtures::read(&mut scene.peer).await;
        assert_eq!(response["error"]["code"], json!(-32601));
        assert_eq!(
            response["error"]["message"],
            json!("Request not handled by host")
        );
        assert_eq!(*scene.session.hook_state.lock(), Value::Null);
    });
}

#[test]
fn native_request_reducer_failure_and_prompt_id_reuse_are_refused_before_service() {
    for malicious in [false, true] {
        crate::async_runtime::block_on(async {
            let mut scene = fixtures::scene();
            scene.probe.bridges.store(true, Ordering::SeqCst);
            scene.probe.native_prompt.store(malicious, Ordering::SeqCst);
            if !malicious {
                scene.probe.fail_next.store(1, Ordering::SeqCst);
            }
            let frame = RuntimeFrame::Request {
                request_id: RpcId::String("native-only".into()),
                method: "fs/write_text_file".into(),
                params: json!({"sessionId":"thread","path":"/outside","content":"opaque"}),
            };
            let (_step, frames) = run(&mut scene, vec![frame], false, true).await;
            assert!(frames
                .iter()
                .any(|frame| frame["id"] == "native-only" && frame["error"]["code"] == -32602));
            let tab = scene.session.context.lock().tab_id.clone();
            assert!(scene
                .fixture
                .service
                .mux
                .read()
                .get_tab(&tab)
                .unwrap()
                .pending_prompt
                .is_none());
            assert!(scene.session.prompts.lock().is_empty());
        });
    }
}

#[cfg(any(unix, windows))]
#[test]
fn native_first_request_starts_provider_turn_before_approval_and_commits() {
    use crate::agent::provider::AgentProvider;
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    crate::async_runtime::block_on(async {
        let mut scene = fixtures::scene();
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let mut context = scene.session.context.lock().clone();
        context.cwd = root.to_string_lossy().into_owned();
        assert!(context.active_turn_id.is_none());
        let plugin =
            crate::test_support::fixture_manifest_dir().join("../../../plugins/provider-acp");
        let mut raw: Value =
            serde_json::from_str(&std::fs::read_to_string(plugin.join("plugin.json")).unwrap())
                .unwrap();
        raw["id"] = json!(scene.session.provider);
        let adapter = JsProvider::new(
            PluginManifest::from_json_str(&raw.to_string()).unwrap(),
            Arc::new(
                PluginVm::from_source(
                    &scene.session.provider,
                    std::fs::read_to_string(plugin.join("plugin.js")).unwrap(),
                )
                .unwrap(),
            ),
        );
        let unbound = RuntimeContext {
            thread_id: None,
            ..context.clone()
        };
        let init = adapter
            .runtime_plan(&unbound, &RuntimeOperation::Initialize)
            .await
            .unwrap();
        adapter
            .runtime_response(
                &unbound,
                &RuntimeOperation::Initialize,
                &init.state,
                &RpcReply::Success {
                    value: json!({"protocolVersion":1,"agentCapabilities":{},"authMethods":[]}),
                },
            )
            .await
            .unwrap();
        let open = RuntimeOperation::OpenThread {
            instructions: String::new(),
            settings: Default::default(),
        };
        let planned = adapter.runtime_plan(&unbound, &open).await.unwrap();
        adapter
            .runtime_response(
                &unbound,
                &open,
                &planned.state,
                &RpcReply::Success {
                    value: json!({"sessionId":"thread"}),
                },
            )
            .await
            .unwrap();
        let submit = RuntimeOperation::SubmitTurn {
            thread_id: "thread".into(),
            active_turn_id: None,
            send_id: "first-request-send".into(),
            input: TurnInput {
                text: "opaque".into(),
                attachments: Vec::new(),
            },
            settings: Default::default(),
        };
        let planned = adapter.runtime_plan(&context, &submit).await.unwrap();
        let declared = adapter.agent_runtime().unwrap().client_bridges;
        scene
            .fixture
            .service
            .agent_registry
            .register_provider(Box::new(adapter));
        *scene.session.adapter.lock() = scene
            .fixture
            .service
            .agent_registry
            .provider_arc(&scene.session.provider);
        context.client_bridges = super::super::client_bridge::install(&context, declared).unwrap();
        *scene.session.context.lock() = context.clone();
        // The real manifest declares host approvals, whose unset choice never asks.
        scene
            .fixture
            .service
            .mux
            .write()
            .get_tab_mut(&context.tab_id)
            .unwrap()
            .restorable_launch
            .host_approval_choice = Some(crate::multiplexer::types::HostApprovalChoice::Ask);
        let process = scene.session.process.lock().clone().unwrap();
        let (submitted, request) = tokio::join!(
            super::super::turn_requests::run(
                &scene.fixture.service,
                &scene.session,
                &process,
                context.clone(),
                submit,
                planned,
                None,
            ),
            fixtures::read(&mut scene.peer),
        );
        assert!(matches!(
            submitted,
            Err(RuntimeFailure::WrittenAwaitingEvidence)
        ));
        assert_eq!(request["method"], "session/prompt");
        assert_eq!(request["params"]["sessionId"], "thread");
        let window = super::super::turn_requests::stream_window(&scene.session, vec![1]).await;
        assert!(window.turn_requests[0].written);
        assert_eq!(window.turn_requests[0].after_sequence, Some(0));
        let frame = RuntimeFrame::Request {
            request_id: RpcId::String("first-write".into()),
            method: "fs/write_text_file".into(),
            params: json!({"sessionId":"thread","path":root.join("file"),"content":"approved-first-request"}),
        };
        let (step, _) = run(&mut scene, vec![frame], false, false).await;
        assert!(matches!(step, Step::Continue));
        assert_eq!(
            scene.session.context.lock().active_turn_id.as_deref(),
            Some("thread:turn:1")
        );
        assert!(!root.join("file").exists());
        assert!(scene
            .session
            .prompts
            .lock()
            .get(&RpcId::String("first-write".into()))
            .unwrap()
            .prompt
            .is_some());
        super::super::client_bridge::answer(
            &scene.fixture.service,
            &scene.session,
            &process,
            &RpcId::String("first-write".into()),
            &PromptAnswer::Choice {
                values: vec!["allow_once".into()],
            },
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("file")).unwrap(),
            "approved-first-request"
        );
        assert_eq!(fixtures::read(&mut scene.peer).await["result"], json!({}));
        super::super::client_bridge::cancel(&scene.fixture.service, &scene.session, &process, true)
            .await
            .unwrap();
        let mut changes = scene.session.control_changes.subscribe();
        fixtures::write(
            &mut scene.peer,
            json!({"id":request["id"],"result":{"stopReason":"end_turn"}}),
        )
        .await;
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                changes.borrow_and_update();
                if scene.session.turn_requests.lock().is_empty() {
                    break;
                }
                changes.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    });
}

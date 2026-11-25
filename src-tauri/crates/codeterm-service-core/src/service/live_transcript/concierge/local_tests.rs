use codeterm_decision::{
    DecisionError, DecisionMetadata, DecisionModel, DecisionModelInfo, DecisionPrimitives,
    TokenMode,
};
use mockito::{Matcher, Mock, Server};
use serde_json::Value;

use super::super::{BindingTarget, CursorStart};
use super::*;
use crate::{
    multiplexer::state::MultiplexerState,
    service::test_fixture::{RecordedAgentDeliveries, ServiceFixture},
};

struct LocalAdapter {
    address: Option<String>,
    cached: bool,
}

impl DecisionModel for LocalAdapter {
    fn id(&self) -> &str {
        "test-local"
    }

    fn token_mode(&self) -> TokenMode {
        TokenMode::None
    }

    fn metadata(&self) -> DecisionMetadata {
        DecisionMetadata {
            server_address: self.address.clone(),
            ..Default::default()
        }
    }

    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            choice: true,
            ..Default::default()
        }
    }

    fn cached_models(&self) -> Vec<DecisionModelInfo> {
        if self.cached {
            self.models()
        } else {
            Vec::new()
        }
    }

    fn models(&self) -> Vec<DecisionModelInfo> {
        ["Embedding-model", "first-chat", "second-chat"]
            .into_iter()
            .map(|id| DecisionModelInfo {
                id: id.into(),
                display_name: id.into(),
            })
            .collect()
    }

    fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
        panic!("Local routing must bypass the Scout adapter")
    }
}

fn fixture() -> (ServiceFixture, String, String) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.create_workspace("Local Concierge".into(), None);
    let owner = mux.create_tab(&workspace.id).unwrap();
    let target = mux.create_tab(&workspace.id).unwrap();
    mux.get_tab_mut(&target).unwrap().title = "Report Target".into();
    (ServiceFixture::new(mux), owner, target)
}

fn register(svc: &ServiceContext, id: &str, address: Option<String>, cached: bool) {
    svc.supporter_models
        .register(id, Arc::new(LocalAdapter { address, cached }));
}

fn attach(
    svc: &ServiceContext,
    owner: &str,
    server: &Server,
    trigger: TranscriptTrigger,
) -> TranscriptStore {
    register(svc, "test-local", Some(server.url()), true);
    svc.ext::<crate::config::ConfigState>()
        .lock()
        .live_transcript
        .provider = "local".into();
    let binding = svc
        .update_live_transcript_binding(owner, None, CursorStart::Beginning, Some(trigger))
        .unwrap();
    assert_eq!(binding.target, BindingTarget::Local);
    svc.open_live_transcript(owner).unwrap()
}

fn append(store: &TranscriptStore, owner: &str, at_ms: u64, text: &str) {
    store
        .append(
            owner,
            TranscriptInput {
                at_ms,
                content: TranscriptContent::Speech { text: text.into() },
            },
        )
        .unwrap();
}

fn mock_decision(server: &mut Server, body: &str) -> Mock {
    server
        .mock("POST", "/chat/completions")
        .match_body(Matcher::PartialJson(json!({
            "model": "first-chat", "temperature": 0.0, "max_tokens": 384,
            "response_format": {"type": "json_schema", "json_schema": {
                "strict": true,
                "schema": {"required": ["rationale", "action", "targetTabId", "message"]}
            }}
        })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"choices": [{"message": {"content": body}}]}).to_string())
        .expect(1)
        .create()
}

fn summary(store: &TranscriptStore) -> Value {
    serde_json::to_value(store.training_log_summary(50).unwrap()).unwrap()
}

#[test]
fn discovers_active_http_supporter_and_filters_embedding_models() {
    let server = Server::new();
    let (fixture, _, _) = fixture();
    let svc = &fixture.service;
    register(
        svc,
        "alternate",
        Some("http://localhost:11434/v1".into()),
        true,
    );
    register(svc, "preferred", Some(server.url()), false);
    register(svc, "non-http", Some("unix:/tmp/supporter".into()), true);
    let config = svc.ext::<crate::config::ConfigState>();
    {
        let mut config = config.lock();
        config.supporter.adapter = Some("preferred".into());
        config
            .supporter
            .connections
            .entry("preferred".into())
            .or_default()
            .model = Some("configured-chat".into());
    }
    let setup = svc.live_transcript_setup();
    assert_eq!(setup["router"]["provider"], "local");
    assert_eq!(setup["router"]["model"], "configured-chat");
    let provider = &setup["router"]["providers"][0];
    assert_eq!(provider["id"], "local");
    assert_eq!(provider["models"], json!(["first-chat", "second-chat"]));
    assert_eq!(provider["displayName"], "Local (Supporter)");
    config
        .lock()
        .supporter
        .connections
        .get_mut("preferred")
        .unwrap()
        .model = Some("Text-Embed".into());
    assert_eq!(svc.live_transcript_setup()["router"]["model"], "first-chat");
    config.lock().supporter.adapter = Some("non-http".into());
    assert_eq!(
        svc.live_transcript_setup()["router"]["providers"][0]["displayName"],
        "Local (Ollama)"
    );
}

#[test]
fn binds_without_hidden_router_and_uses_the_attached_cursor() {
    let server = Server::new();
    let (fixture, owner, _) = fixture();
    let svc = &fixture.service;
    let count = svc.mux.read().all_tabs().count();
    let store = attach(svc, &owner, &server, TranscriptTrigger::Manual);
    assert_eq!(svc.mux.read().all_tabs().count(), count);
    assert!(!svc
        .mux
        .read()
        .all_tabs()
        .any(|tab| tab.is_transcript_router()));
    let status = svc.live_transcript_status(&owner).unwrap();
    assert!(matches!(status.concierge, ConciergeStatus::Ready));
    assert_eq!(status.router.unwrap().activity, RouterActivity::Idle);
    append(&store, &owner, 100, "Context");
    let cursor = store.ack(&status.binding, 1).unwrap();
    assert_eq!(cursor.consumer_key, format!("concierge:{owner}"));
    assert_eq!(store.attached_cursor().unwrap(), cursor);
}

#[test]
fn normalizes_blank_send_to_ignore_and_acknowledges_in_one_call() {
    let mut server = Server::new();
    let (fixture, owner, target) = fixture();
    let svc = &fixture.service;
    let writes = fixture.bind_recorded_agent_recipient(&target, "target-session", "target-nonce");
    let store = attach(svc, &owner, &server, TranscriptTrigger::ScoutThenConcierge);
    append(&store, &owner, 100, "Thinking aloud");
    let endpoint = mock_decision(
        &mut server,
        &json!({
            "rationale": "No actionable request was addressed.",
            "action": "send", "targetTabId": target, "message": "  ",
        })
        .to_string(),
    );
    assert!(process(
        svc,
        &store,
        &owner,
        TranscriptRange {
            from_seq: 1,
            to_seq: 1
        }
    )
    .unwrap());
    endpoint.assert();
    assert_eq!(store.cursor(&store.binding()).unwrap().acked_seq, 1);
    assert_eq!(store.routing_state().unwrap().attempted_through, 1);
    assert!(store.forwards().unwrap().is_empty());
    assert!(writes.texts.lock().is_empty());
    assert!(!fixture.has_pending_agent_delivery(&target));
    let result = summary(&store);
    assert_eq!(result["ignoreCount"], 1);
    let sample = &result["recentSamples"][0];
    assert_eq!(sample["source"], "local_concierge");
    assert_eq!(sample["unreadRows"][0]["text"], "Thinking aloud");
    assert_eq!(sample["decision"]["action"], "ignore");
    assert!(sample["decision"]["targetTabId"].is_null());
    assert!(sample["decision"]["message"].is_null());
    assert!(store.inner.root.join("routing_samples.jsonl").is_file());
}

fn sent_fixture() -> (
    ServiceFixture,
    TranscriptStore,
    String,
    String,
    RecordedAgentDeliveries,
) {
    let mut server = Server::new();
    let (fixture, owner, target) = fixture();
    let svc = &fixture.service;
    let writes = fixture.bind_recorded_agent_recipient(&target, "target-session", "target-nonce");
    let store = attach(svc, &owner, &server, TranscriptTrigger::Manual);
    append(&store, &owner, 100, "Report Target, please");
    store.ack(&store.binding(), 1).unwrap();
    append(&store, &owner, 1_000, "summarize yesterday's results");
    let endpoint = mock_decision(
        &mut server,
        &format!(
            "<think>analysis</think>\n\x60\x60\x60json\n{}\n\x60\x60\x60",
            json!({
                "rationale": "The addressed request completed across two speech chunks.",
                "action": "send", "targetTabId": "rEpOrT TaRgEt", "message": " Summarize yesterday's results. ",
            })
        ),
    );
    svc.route_live_transcript_manually(
        &owner,
        TranscriptRange {
            from_seq: 2,
            to_seq: 2,
        },
    )
    .unwrap();
    endpoint.assert();
    (fixture, store, owner, target, writes)
}

#[test]
fn sends_to_the_resolved_tab_and_persists_forward_and_action() {
    let (_fixture, store, _, target, writes) = sent_fixture();
    let forwards = store.forwards().unwrap();
    assert_eq!(forwards.len(), 1);
    let forward = &forwards[0];
    assert_eq!(forward.target_tab_id, target);
    assert_eq!(
        forward.range,
        TranscriptRange {
            from_seq: 1,
            to_seq: 2
        }
    );
    assert_eq!(
        forward.rationale.as_deref(),
        Some("The addressed request completed across two speech chunks.")
    );
    assert_eq!(forward.epoch_id, None);
    assert_eq!(forward.sent_text, "Summarize yesterday's results.");
    assert_eq!(writes.texts.lock().len(), 1);
    assert!(writes.texts.lock()[0].contains(&forward.sent_text));
    assert_eq!(store.cursor(&store.binding()).unwrap().acked_seq, 2);
    let action = store.routed_action(&forward.forward_id).unwrap().unwrap();
    assert_eq!(action.action.destination, target);
    let result = summary(&store);
    assert_eq!(result["sendCount"], 1);
    let sample = &result["recentSamples"][0];
    assert_eq!(sample["forwardId"], forward.forward_id);
    assert_eq!(sample["decision"]["targetTabId"], target);
    assert_eq!(sample["contextRows"][0]["dtMs"], -900);
    assert_eq!(sample["unreadRows"][0]["seq"], 2);
}

#[test]
fn feedback_updates_the_forward_and_counts_the_latest_verdict() {
    let (fixture, store, owner, _, _writes) = sent_fixture();
    let forward = store.forwards().unwrap().pop().unwrap();
    let svc = &fixture.service;
    assert!(svc
        .record_live_transcript_feedback(&owner, &forward.forward_id, "bad", None)
        .is_err());
    let result = svc
        .record_live_transcript_feedback(
            &owner,
            &forward.forward_id,
            "wrong_send",
            Some("Wrong recipient".into()),
        )
        .unwrap();
    assert_eq!(result.feedback.as_deref(), Some("wrong_send"));
    assert_eq!(
        store.forwards().unwrap()[0].feedback.as_deref(),
        Some("wrong_send")
    );
    assert_eq!(summary(&store)["wrongSendCount"], 1);
    let log = std::fs::read_to_string(store.inner.root.join("routing_feedback.jsonl")).unwrap();
    let entry: Value = serde_json::from_str(log.trim()).unwrap();
    assert_eq!(entry["note"], "Wrong recipient");
    assert_eq!(entry["forwardId"], forward.forward_id);
    assert_eq!(entry["range"], json!(forward.range));
    svc.record_live_transcript_feedback(&owner, &forward.forward_id, "good_send", None)
        .unwrap();
    let result = summary(&store);
    assert_eq!(result["wrongSendCount"], 0);
    assert_eq!(result["goodSendCount"], 1);
}

#[test]
fn wake_word_reviews_the_whole_unread_batch_once() {
    let mut server = Server::new();
    let (fixture, owner, _) = fixture();
    let svc = &fixture.service;
    let store = attach(svc, &owner, &server, TranscriptTrigger::WakeWord);
    for at_ms in 1..=16 {
        append(&store, &owner, at_ms, "Thinking aloud");
    }
    assert!(!process(
        svc,
        &store,
        &owner,
        TranscriptRange {
            from_seq: 1,
            to_seq: 16
        }
    )
    .unwrap());
    assert_eq!(store.cursor(&store.binding()).unwrap().acked_seq, 0);
    append(
        &store,
        &owner,
        17,
        "Hey, Domios, I am testing my microphone.",
    );
    let endpoint = mock_decision(
        &mut server,
        &json!({
            "rationale": "Microphone commentary does not need routing.",
            "action": "ignore", "targetTabId": null, "message": null,
        })
        .to_string(),
    );
    assert!(process(
        svc,
        &store,
        &owner,
        TranscriptRange {
            from_seq: 17,
            to_seq: 17
        }
    )
    .unwrap());
    endpoint.assert();
    assert_eq!(store.cursor(&store.binding()).unwrap().acked_seq, 17);
    assert_eq!(
        summary(&store)["recentSamples"][0]["unreadRows"]
            .as_array()
            .unwrap()
            .len(),
        17
    );
}

#[test]
fn attached_cursor_carries_local_ack_and_keeps_cloud_sends_distinct_from_pure_acks() {
    use crate::multiplexer::types::TabAudience;

    let server = Server::new();
    let (fixture, owner, router) = fixture();
    let svc = &fixture.service;
    let store = attach(svc, &owner, &server, TranscriptTrigger::Manual);
    append(&store, &owner, 100, "Already reviewed locally");
    let local_cursor = store.ack(&store.binding(), 1).unwrap();
    let epoch = ConciergeEpoch {
        epoch_id: "cloud-epoch".into(),
        runtime_tab_id: router.clone(),
        session_id: "cloud-session".into(),
        binding_nonce: "cloud-nonce".into(),
        provider: "cloud-provider".into(),
        model: Some("cloud-model".into()),
        started_at_ms: 100,
    };
    {
        let mut mux = svc.mux.write();
        let tab = mux.get_tab_mut(&router).unwrap();
        tab.audience = TabAudience::TranscriptRouter;
        tab.spawned_by_tab_id = Some(owner.clone());
    }
    let revision = store.binding().revision;
    store.stage_epoch(revision, epoch.clone()).unwrap();
    let binding = store
        .set_binding(
            revision,
            BindingTarget::Attached {
                epoch_id: epoch.epoch_id,
            },
            CursorStart::Beginning,
        )
        .unwrap();
    assert_eq!(store.cursor(&binding).unwrap(), local_cursor);
    append(&store, &owner, 200, "Cloud ignore");
    svc.ack_transcript_and_log(&store, &binding, &owner, 2)
        .unwrap();
    append(&store, &owner, 300, "Cloud send");
    svc.record_live_transcript_forward(&router, &owner, "Cloud response");
    svc.ack_transcript_and_log(&store, &binding, &owner, 3)
        .unwrap();
    svc.ack_transcript_and_log(&store, &binding, &owner, 3)
        .unwrap();
    let result = summary(&store);
    assert_eq!(result["totalSamples"], 2);
    assert_eq!(result["sendCount"], 1);
    assert_eq!(result["ignoreCount"], 1);
    let send = &result["recentSamples"][0];
    let ignore = &result["recentSamples"][1];
    assert_eq!(send["source"], "cloud_concierge");
    assert_eq!(send["forwardId"], store.forwards().unwrap()[0].forward_id);
    assert_eq!(ignore["source"], "cloud_concierge");
    assert_eq!(ignore["range"], json!({"fromSeq": 2, "toSeq": 2}));
}

#[test]
fn local_context_stops_at_pause_boundary_and_stale_age() {
    let mut server = Server::new();
    let (fixture, owner, _) = fixture();
    let svc = &fixture.service;
    let store = attach(svc, &owner, &server, TranscriptTrigger::Manual);
    append(&store, &owner, 1_000, "Stale old request from earlier");
    store
        .append(
            &owner,
            TranscriptInput {
                at_ms: 2_000,
                content: TranscriptContent::Paused {
                    reason: "manual".into(),
                },
            },
        )
        .unwrap();
    append(&store, &owner, 200_000, "Fresh lead-in");
    store.ack(&store.binding(), 3).unwrap();
    append(&store, &owner, 205_000, "Microphone test 123");
    let endpoint = mock_decision(
        &mut server,
        &json!({
            "rationale": "Microphone test is ignored.",
            "action": "ignore", "targetTabId": null, "message": null,
        })
        .to_string(),
    );
    svc.route_live_transcript_manually(
        &owner,
        TranscriptRange {
            from_seq: 4,
            to_seq: 4,
        },
    )
    .unwrap();
    endpoint.assert();
    let context = summary(&store)["recentSamples"][0]["contextRows"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(context.len(), 1);
    assert_eq!(context[0]["seq"], 3);
    assert_eq!(context[0]["dtMs"], -5_000);
}

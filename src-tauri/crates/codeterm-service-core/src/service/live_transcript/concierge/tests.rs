use std::path::Path;

use codeterm_plugin::transcribe::{
    DictationAdmission, DictationSession, TranscribeError, TranscribeRegistry, Transcriber,
    TranscriberStatus, Transcript,
};
use codeterm_types::dictation::DictationSegment;
use uuid::Uuid;

use super::*;
use crate::{
    multiplexer::state::MultiplexerState,
    service::{
        dictation::live_capture::{LiveCaptureAdmission, LiveCaptureStart},
        tab_credentials::TabCaller,
        test_fixture::ServiceFixture,
    },
};

#[test]
fn scout_segments_preserve_any_language_without_keyword_filter_and_redact_secrets() {
    assert!(scout::scout_segment(1, "   \n\t  ").is_none());
    let ru = scout::scout_segment(2, "Отправь это в Main прямо сейчас").unwrap();
    assert_eq!(ru["seq"], 2);
    assert_eq!(ru["text"], "Отправь это в Main прямо сейчас");
    let with_secret = scout::scout_segment(3, "Bearer sk-1234567890abcdefghijklmnop").unwrap();
    assert!(!with_secret["text"]
        .as_str()
        .unwrap()
        .contains("sk-1234567890"));
    assert!(wake_word("Hey, Domios, show me"));
    assert!(!wake_word("I said hey Domios"));
}

fn fixture() -> (ServiceFixture, Arc<ServiceContext>, String, String) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.create_workspace("Transcript fixture".into(), None);
    let owner = mux.create_tab(&workspace.id).unwrap();
    let other = mux.create_tab(&workspace.id).unwrap();
    let fixture = ServiceFixture::new(mux);
    let svc = Arc::new(fixture.service.clone());
    (fixture, svc, owner, other)
}

#[test]
fn silence_hallucinations_match_whole_normalized_utterances() {
    for text in [
        "",
        "  « … THANK\tYOU very  much! » ",
        "'Thank you.'",
        "thanks",
        "Thanks for watching ...",
        "Thanks for listening!",
        "Thank you for watching.",
        "Thank you for listening.",
        "You.",
        "Bye!",
        "Goodbye.",
        "Subtitles by",
        "Transcribed by",
        "Спасибо!",
        "Спасибо за просмотр.",
        "Продолжение следует…",
        "Субтитры делал",
        "Субтитры подготовил",
    ] {
        assert!(is_silence_hallucination(text), "{text:?}");
    }
    for text in [
        "Thank you, please send that to Main",
        "Спасибо, отправь это в Мэйн",
        "Subtitles by Alex",
        "Yeah.",
        "Mm-hmm.",
        "Uh.",
    ] {
        assert!(!is_silence_hallucination(text), "{text:?}");
    }
}

#[test]
fn retryable_router_failures_do_not_set_the_persistent_banner() {
    let mut dispatch = OwnerDispatch::default();
    for error in [
        "Transcript router is busy or unavailable",
        "Transcript router requires an idle, exact ready session",
        "A transcript dispatch is in progress",
        "Send is awaiting confirmation",
        "This transcript range already has a send record",
        "Transcript router admission is held",
    ] {
        dispatch.settle_route(Err(error.into()));
        assert_eq!(dispatch.last_route_error, None);
    }
    dispatch.settle_route(Err("Committed speech unavailable".into()));
    dispatch.settle_route(Err("Transcript router runtime is busy".into()));
    assert_eq!(
        dispatch.last_route_error.as_deref(),
        Some("Committed speech unavailable")
    );
    dispatch.settle_route(Ok(()));
    assert_eq!(dispatch.last_route_error, None);
}

#[test]
fn forward_range_recovers_acked_speech_without_reclaiming_an_earlier_forward() {
    use super::super::{TranscriptForward, TranscriptRow};
    let rows = (1..=7)
        .map(|seq| TranscriptRow {
            seq,
            at_ms: if seq == 1 { 0 } else { 8_001 + seq * 1_000 },
            content: TranscriptContent::Speech {
                text: format!("Chunk {seq}"),
            },
        })
        .collect::<Vec<_>>();
    assert_eq!(
        forward::forward_range(6, 7, &rows, &[]),
        TranscriptRange {
            from_seq: 2,
            to_seq: 7
        }
    );
    let previous = TranscriptForward {
        forward_id: "earlier".into(),
        epoch_id: None,
        range: TranscriptRange {
            from_seq: 2,
            to_seq: 3,
        },
        at_ms: 12_000,
        target_tab_id: "tab".into(),
        target_title: "Main".into(),
        sent_text: "Earlier speech".into(),
        sent_excerpt: "Earlier speech".into(),
        rationale: None,
        feedback: None,
    };
    assert_eq!(
        forward::forward_range(6, 7, &rows, std::slice::from_ref(&previous)),
        TranscriptRange {
            from_seq: 4,
            to_seq: 7
        }
    );
    assert_eq!(
        forward::forward_range(2, 7, &rows, std::slice::from_ref(&previous)),
        TranscriptRange {
            from_seq: 4,
            to_seq: 7
        }
    );
    assert_eq!(
        forward::forward_range(3, 3, &rows, std::slice::from_ref(&previous)),
        previous.range
    );
}

#[test]
fn router_forward_receipts_preserve_epoch_and_rotation_keeps_reviewing_status() {
    use super::super::{BindingTarget, CursorStart};
    use crate::multiplexer::types::TabAudience;

    let (_fixture, svc, owner, router) = fixture();
    let store = svc.open_live_transcript(&owner).unwrap();
    let epoch = ConciergeEpoch {
        epoch_id: "epoch-1".into(),
        runtime_tab_id: router.clone(),
        session_id: "session-1".into(),
        binding_nonce: "nonce-1".into(),
        provider: "fixture-provider".into(),
        model: Some("fixture-model".into()),
        started_at_ms: 1,
    };
    let target_title = {
        let mut mux = svc.mux.write();
        let tab = mux.get_tab_mut(&router).unwrap();
        tab.audience = TabAudience::TranscriptRouter;
        tab.spawned_by_tab_id = Some(owner.clone());
        mux.display_label_for_tab(&owner)
    };
    let revision = store.binding().revision;
    store.stage_epoch(revision, epoch.clone()).unwrap();
    let binding = store
        .set_binding(
            revision,
            BindingTarget::Attached {
                epoch_id: epoch.epoch_id.clone(),
            },
            CursorStart::Beginning,
        )
        .unwrap();
    for at_ms in 1..=3 {
        store
            .append(
                &owner,
                TranscriptInput {
                    at_ms,
                    content: TranscriptContent::Speech {
                        text: "Speech chunk".into(),
                    },
                },
            )
            .unwrap();
    }
    store.ack(&binding, 2).unwrap();
    store
        .save_routing_state(&RoutingState {
            attempted_through: 3,
            ..Default::default()
        })
        .unwrap();
    let mut events = svc.events.subscribe();
    svc.record_live_transcript_forward(&owner, &router, "Wrong source");
    assert!(store.forwards().unwrap().is_empty());
    svc.record_live_transcript_forward(&router, &owner, &format!("  {}  ", "🦉".repeat(4_001)));
    let forwards = store.forwards().unwrap();
    let [forward] = forwards.as_slice() else {
        panic!("Expected one forward")
    };
    assert_eq!(forward.epoch_id.as_deref(), Some("epoch-1"));
    assert_eq!(
        forward.range,
        TranscriptRange {
            from_seq: 1,
            to_seq: 3
        }
    );
    assert_eq!(forward.target_tab_id, owner);
    assert_eq!(forward.target_title, target_title);
    assert_eq!(forward.sent_text, "🦉".repeat(4_000));
    assert_eq!(forward.sent_excerpt, format!("{}…", "🦉".repeat(240)));
    assert!(matches!(
        &events.try_recv().unwrap().event,
        codeterm_events::SystemEvent::Generic { event_type, data }
            if event_type == "live-transcript-changed" && data["ownerTabId"] == owner
    ));

    store.suspend_attached_epoch(&binding).unwrap();
    svc.ext_or_init(Dispatch::default)
        .owners
        .lock()
        .entry(owner.clone())
        .or_default()
        .running = true;
    let status = svc.live_transcript_status(&owner).unwrap();
    assert!(matches!(status.concierge, ConciergeStatus::Ready));
    assert_eq!(status.reviewing_through_seq, Some(3));
    let router = status.router.unwrap();
    assert_eq!(router.activity, RouterActivity::Reviewing);
    assert_eq!(router.provider, epoch.provider);
    assert_eq!(router.model, epoch.model);
}

#[test]
fn setup_uses_saved_trigger_and_mic_default_and_falls_back_without_scout() {
    let (_fixture, svc, owner, _) = fixture();
    let initial = svc.live_transcript_setup();
    assert_eq!(initial["configured"], false);
    assert_eq!(initial["defaultMicTap"], false);
    assert_eq!(initial["defaultTrigger"], "direct_batch");
    {
        let mut config = svc.ext::<crate::config::ConfigState>().lock();
        config.live_transcript.configured = true;
        config.live_transcript.default_mic_tap = true;
        config.live_transcript.trigger = "manual".into();
    }
    let setup = svc.live_transcript_setup();
    assert_eq!(setup["defaultTrigger"], "manual");
    let status = svc.live_transcript_status(&owner).unwrap();
    assert!(status.configured);
    assert!(status.default_mic_tap);
    assert_eq!(
        default_trigger("scout_then_concierge", ScoutStatus::Unavailable),
        TranscriptTrigger::DirectBatch,
    );
    assert_eq!(
        default_trigger("invalid", ScoutStatus::Ready),
        TranscriptTrigger::ScoutThenConcierge,
    );
}

#[test]
fn route_failure_is_visible_until_acknowledgement_and_emits_owner_change() {
    let (_fixture, svc, owner, _) = fixture();
    let store = svc.open_live_transcript(&owner).unwrap();
    svc.set_live_transcript_trigger(&owner, TranscriptTrigger::Manual)
        .unwrap();
    store
        .append(
            &owner,
            TranscriptInput {
                at_ms: 1,
                content: TranscriptContent::Speech {
                    text: "opaque".into(),
                },
            },
        )
        .unwrap();
    let mut events = svc.events.subscribe();
    let error = svc
        .route_live_transcript_manually(
            &owner,
            TranscriptRange {
                from_seq: 1,
                to_seq: 1,
            },
        )
        .unwrap_err();
    assert_eq!(
        svc.live_transcript_status(&owner).unwrap().last_route_error,
        Some(error)
    );
    svc.live_transcript_cursor_changed(&owner, false);
    assert!(svc
        .live_transcript_status(&owner)
        .unwrap()
        .last_route_error
        .is_some());
    svc.live_transcript_cursor_changed(&owner, true);
    assert!(svc
        .live_transcript_status(&owner)
        .unwrap()
        .last_route_error
        .is_none());
    let mut changes = 0;
    while let Ok(event) = events.try_recv() {
        if let codeterm_events::SystemEvent::Generic { event_type, data } = &event.event {
            if event_type == "live-transcript-changed" && data["ownerTabId"] == owner {
                changes += 1;
            }
        }
    }
    assert!(changes >= 4);
}

#[test]
fn default_scout_is_unavailable_and_external_scope_needs_exact_revision_and_nonce() {
    let (_fixture, svc, owner, other) = fixture();
    let initial = svc.live_transcript_status(&owner).unwrap();
    assert_eq!(initial.trigger, TranscriptTrigger::ScoutThenConcierge);
    assert!(matches!(initial.scout, ScoutStatus::Unavailable));

    svc.set_live_transcript_trigger(&owner, TranscriptTrigger::DirectBatch)
        .unwrap();
    let chosen = svc.live_transcript_status(&owner).unwrap();
    assert_eq!(chosen.trigger, TranscriptTrigger::DirectBatch);
    assert!(matches!(chosen.scout, ScoutStatus::Disabled));

    svc.mux
        .write()
        .get_tab_mut(&other)
        .unwrap()
        .session_binding_nonce = Some("nonce".into());
    let caller = TabCaller {
        tab_id: other.clone(),
        binding_nonce: "nonce".into(),
    };
    let binding = svc
        .set_transcript_external_override(
            &owner,
            Some(&other),
            super::super::CursorStart::Beginning,
        )
        .unwrap();
    let store = svc
        .with_live_transcript_consumer(&owner, &caller, binding.revision, |store, active| {
            assert_eq!(active, &binding);
            assert_eq!(
                store.cursor(active).unwrap().consumer_key,
                format!("tab:{other}")
            );
            store.clone()
        })
        .unwrap();
    assert!(svc
        .with_live_transcript_consumer(&owner, &caller, binding.revision - 1, |_, _| ())
        .is_err());
    let wrong_nonce = TabCaller {
        binding_nonce: "stale".into(),
        ..caller.clone()
    };
    assert!(svc
        .with_live_transcript_consumer(&owner, &wrong_nonce, binding.revision, |_, _| ())
        .is_err());
    svc.set_transcript_external_override(
        &owner,
        Some(&owner),
        super::super::CursorStart::Beginning,
    )
    .unwrap();
    assert!(svc
        .with_live_transcript_consumer(&owner, &caller, binding.revision, |_, _| ())
        .is_err());
    assert!(store.ack(&binding, 0).is_err());
}

struct CaptureProvider;
struct CaptureEngine;

#[async_trait::async_trait]
impl Transcriber for CaptureProvider {
    fn id(&self) -> &str {
        "concierge-capture-fixture"
    }
    fn supports_dictation(&self) -> bool {
        true
    }
    async fn status(&self) -> TranscriberStatus {
        TranscriberStatus::Ready
    }
    async fn start_dictation(&self) -> DictationAdmission {
        DictationAdmission::Ready(Box::new(CaptureEngine))
    }
    async fn transcribe(&self, _: &Path, _: &str) -> Result<Transcript, TranscribeError> {
        Err(TranscribeError::Empty)
    }
}

#[async_trait::async_trait]
impl DictationSession for CaptureEngine {
    async fn update(&mut self, pcm: Vec<u8>, finish: bool) -> Result<DictationSegment, String> {
        let mut segment = DictationSegment::default();
        if finish {
            segment.committed = "opaque_committed".into();
        } else if pcm.iter().any(|byte| *byte != 0) {
            segment.provisional = "opaque_provisional".into();
        }
        Ok(segment)
    }

    async fn close(self: Box<Self>) {}
}

#[tokio::test]
async fn capture_persists_only_the_committed_tail_and_the_exact_owner_pause() {
    let (_fixture, svc, owner, other) = fixture();
    let provider = Arc::new(CaptureProvider);
    svc.ext::<Arc<TranscribeRegistry>>()
        .register(provider.id(), provider.clone());
    svc.install_live_transcript_capture();
    svc.set_live_transcript_trigger(&owner, TranscriptTrigger::Manual)
        .unwrap();
    let store = svc.open_live_transcript(&owner).unwrap();
    let client = Uuid::new_v4();
    let admission = svc
        .start_live_capture(
            client,
            LiveCaptureStart {
                tab_id: owner.clone(),
                resume: false,
            },
        )
        .await;
    let LiveCaptureAdmission::Ready { lease } = admission else {
        panic!("Capture was not ready: {admission:?}");
    };
    let sessions = svc.live_capture_sessions();
    let update = sessions.frame(client, lease, 1, vec![2, 1]).await.unwrap();
    assert_eq!(update.provisional, "opaque_provisional");
    assert_eq!(store.committed_seq(), 0);
    assert!(sessions
        .pause_owner(&other, client, lease, 2)
        .await
        .is_err());
    let update = sessions
        .pause_owner(&owner, client, lease, 2)
        .await
        .unwrap();
    assert!(update.paused);
    let page = store.history(1, 8).unwrap();
    assert_eq!(page.rows.len(), 2);
    assert_eq!(
        page.rows[0].content,
        TranscriptContent::Speech {
            text: "opaque_committed".into()
        }
    );
    assert_eq!(
        page.rows[1].content,
        TranscriptContent::Paused {
            reason: "manual".into()
        }
    );
    assert!(sessions
        .pause_owner(&owner, client, lease, 2)
        .await
        .is_err());
}

#[test]
fn failed_summary_does_not_replace_raw_data() {
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::open(dir.path(), "owner", 1).unwrap();
    let binding = store
        .set_binding(
            0,
            super::super::BindingTarget::External {
                tab_id: "owner".into(),
            },
            super::super::CursorStart::Beginning,
        )
        .unwrap();
    store
        .append(
            "owner",
            TranscriptInput {
                at_ms: 1,
                content: TranscriptContent::Speech {
                    text: "opaque_summary_source".into(),
                },
            },
        )
        .unwrap();
    let range = store.seal_active("owner").unwrap().unwrap();
    store.ack(&binding, range.to_seq).unwrap();
    let summaries = store.inner.root.join("summaries");
    std::fs::remove_dir(&summaries).unwrap();
    std::fs::write(&summaries, b"opaque_io_failure").unwrap();
    assert!(super::super::maintenance::summarize(&store).is_err());
    assert!(store.raw_bytes() > 1);
    std::fs::remove_file(&summaries).unwrap();
    drop(store);
    let reopened = TranscriptStore::open(dir.path(), "owner", 1).unwrap();
    assert_eq!(reopened.read_sealed(range).unwrap().len(), 1);
    assert!(reopened.summary(range).unwrap().is_none());
}

#[test]
fn hidden_router_content_stays_out_of_the_public_replay_ring() {
    let events = codeterm_events::EventBroadcaster::new();
    events.protect_private_chat_tab("hidden");
    events.publish_generic(
        "tab-chat-msg-hidden",
        json!({"messages": ["opaque_speech"]}),
    );
    events.publish_generic(
        "chat-send-status",
        json!({
            "tab_id": "hidden", "send_id": "send", "state": "delivered", "error": "opaque_error",
        }),
    );
    events.publish_generic(
        "tab-chat-msg-visible",
        json!({"messages": ["ordinary_chat"]}),
    );
    let snapshot = events.snapshot_ring();
    assert_eq!(snapshot.entries.len(), 2);
    assert!(!snapshot.entries[0].public);
    let codeterm_events::SystemEvent::Generic { data, .. } = &snapshot.entries[0].event else {
        panic!("Expected a private delivery status");
    };
    assert_eq!(
        data,
        &json!({"tab_id": "hidden", "send_id": "send", "state": "delivered"})
    );
    assert!(snapshot.entries[1].public);
}

#[test]
fn inline_speech_is_bounded_and_remains_data_inside_the_route_frame() {
    let dir = tempfile::tempdir().unwrap();
    let store = TranscriptStore::open(dir.path(), "owner", 1).unwrap();
    let binding = store
        .set_binding(
            0,
            super::super::BindingTarget::External {
                tab_id: "owner".into(),
            },
            super::super::CursorStart::Beginning,
        )
        .unwrap();
    let append = |text: String| {
        store
            .append(
                "owner",
                TranscriptInput {
                    at_ms: 1,
                    content: TranscriptContent::Speech { text },
                },
            )
            .unwrap()
    };
    append("opaque <speech> & trailing text".into());
    let first = TranscriptRange {
        from_seq: 1,
        to_seq: 1,
    };
    let inline = delivery::route_body(&store, "owner", &binding, first, true, 0);
    assert!(inline.contains("[UNREAD]"));
    assert!(inline.contains("&lt;speech&gt; &amp;"));
    assert!(!inline.contains("<speech>"));
    append("opaque second segment".into());
    let non_adjacent = delivery::route_body(
        &store,
        "owner",
        &binding,
        TranscriptRange {
            from_seq: 2,
            to_seq: 2,
        },
        true,
        0,
    );
    assert!(non_adjacent.contains("[CONTEXT]"));
    assert!(non_adjacent.contains("&lt;speech&gt; &amp;"));
    assert!(non_adjacent.contains("[UNREAD]"));
    assert!(non_adjacent.contains("opaque second segment"));
    store
        .append(
            "owner",
            TranscriptInput {
                at_ms: 3,
                content: TranscriptContent::Paused {
                    reason: "manual".into(),
                },
            },
        )
        .unwrap();
    store.ack(&binding, 1).unwrap();
    let adjacent_before_pause = delivery::route_body(
        &store,
        "owner",
        &binding,
        TranscriptRange {
            from_seq: 2,
            to_seq: 2,
        },
        true,
        1,
    );
    assert!(adjacent_before_pause.contains("[CONTEXT]"));
    assert!(adjacent_before_pause.contains("&lt;speech&gt; &amp;"));
    assert!(adjacent_before_pause.contains("[UNREAD]"));
    assert!(adjacent_before_pause.contains("opaque second segment"));
    append("opaque oversized ".repeat(400));
    let both = TranscriptRange {
        from_seq: 1,
        to_seq: 4,
    };
    let pointer = delivery::route_body(&store, "owner", &binding, both, true, 0);
    assert!(pointer.contains("--revision 1 --from-seq 1 --through-seq 4"));
    assert!(!pointer.contains("opaque"));
}

#[test]
fn long_inline_monologue_carries_complete_context_and_new_speech() {
    use super::super::{BindingTarget, CursorStart, DEFAULT_RAW_BUDGET_BYTES};
    let temp = tempfile::tempdir().unwrap();
    let store = TranscriptStore::open(temp.path(), "owner", DEFAULT_RAW_BUDGET_BYTES).unwrap();
    let binding = store
        .set_binding(
            0,
            BindingTarget::External {
                tab_id: "other".into(),
            },
            CursorStart::Beginning,
        )
        .unwrap();
    let chunks = (1..=17)
        .map(|seq| {
            format!(
                "Monologue chunk {seq} ending in {}",
                "speech ".repeat(22).trim_end()
            )
        })
        .collect::<Vec<_>>();
    for (index, text) in chunks.iter().enumerate() {
        store
            .append(
                "owner",
                TranscriptInput {
                    at_ms: index as u64,
                    content: TranscriptContent::Speech { text: text.clone() },
                },
            )
            .unwrap();
    }
    store.ack(&binding, 16).unwrap();
    let body = delivery::route_body(
        &store,
        "owner",
        &binding,
        TranscriptRange {
            from_seq: 17,
            to_seq: 17,
        },
        true,
        16,
    );
    let (context, unread) = body.split_once("[UNREAD]").unwrap();
    assert!(context.contains("[CONTEXT]"));
    assert!(chunks[..16].iter().all(|chunk| context.contains(chunk)));
    assert!(unread.contains(&chunks[16]));
    assert!((1_600..=5_000).contains(&body.chars().count()));
}

struct RecordingScoutAdapter {
    model: String,
    calls: std::sync::atomic::AtomicUsize,
    seen_texts: parking_lot::Mutex<Vec<String>>,
    outcome: parking_lot::Mutex<Result<String, codeterm_decision::DecisionError>>,
}

impl RecordingScoutAdapter {
    fn new(model: &str, outcome: Result<&str, codeterm_decision::DecisionError>) -> Self {
        Self {
            model: model.into(),
            calls: std::sync::atomic::AtomicUsize::new(0),
            seen_texts: parking_lot::Mutex::new(Vec::new()),
            outcome: parking_lot::Mutex::new(outcome.map(str::to_owned)),
        }
    }
}

impl codeterm_decision::DecisionModel for RecordingScoutAdapter {
    fn id(&self) -> &str {
        "cli-fixture"
    }

    fn token_mode(&self) -> codeterm_decision::TokenMode {
        codeterm_decision::TokenMode::None
    }

    fn model_id(&self) -> Option<String> {
        Some(self.model.clone())
    }

    fn primitives(&self) -> codeterm_decision::DecisionPrimitives {
        codeterm_decision::DecisionPrimitives {
            choice: true,
            ..Default::default()
        }
    }

    fn models(&self) -> Vec<codeterm_decision::DecisionModelInfo> {
        vec![codeterm_decision::DecisionModelInfo {
            id: self.model.clone(),
            display_name: self.model.clone(),
        }]
    }

    fn decide(
        &self,
        request: &codeterm_decision::DecisionRequest,
        _deadline: std::time::Instant,
    ) -> Result<codeterm_decision::DecisionAnswer, codeterm_decision::DecisionError> {
        codeterm_decision::physical::before_request()?;
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(segments) = request.state["segments"].as_array() {
            let mut seen = self.seen_texts.lock();
            for segment in segments {
                if let Some(text) = segment["text"].as_str() {
                    seen.push(text.to_owned());
                }
            }
        }
        let choice = self.outcome.lock().clone()?;
        Ok(codeterm_decision::DecisionAnswer::Choice {
            probabilities: std::collections::BTreeMap::from([
                (choice.clone(), 0.9),
                (
                    if choice == "wake" {
                        "ignore".into()
                    } else {
                        "wake".into()
                    },
                    0.1,
                ),
            ]),
            choice,
            confidence: Some(0.9),
        })
    }
}

fn fixture_supporter_config(model_id: &str) -> codeterm_types::config::SupporterConfig {
    codeterm_types::config::SupporterConfig {
        adapter: Some("cli-fixture".into()),
        deadline_ms: 3000,
        calls_per_hour: Some(60),
        connections: std::collections::BTreeMap::from([(
            "cli-fixture".into(),
            codeterm_types::config::SupporterConnection {
                model: Some(model_id.into()),
                ..Default::default()
            },
        )]),
        ..Default::default()
    }
}

#[test]
fn active_supporter_matches_readiness_and_calls_adapter_on_committed_utterance() {
    let (_fixture, svc, owner, _) = fixture();
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(codeterm_supporter::SupporterRuntime::new(
        dir.path().to_path_buf(),
    ));
    let model_id = "cli-fixture/fast-free";
    let adapter = Arc::new(RecordingScoutAdapter::new(model_id, Ok("ignore")));
    svc.supporter_models
        .register("cli-fixture", adapter.clone());
    let _override = svc
        .override_supporter_decision_for_test(fixture_supporter_config(model_id), runtime.clone());

    let setup = svc.live_transcript_setup();
    assert_eq!(setup["scout"], "ready");
    assert!(setup["scoutReason"].is_null());

    let status = svc.live_transcript_status(&owner).unwrap();
    assert_eq!(status.trigger, TranscriptTrigger::ScoutThenConcierge);
    assert_eq!(status.scout, ScoutStatus::Ready);
    assert_eq!(status.scout_reason, None);

    let store = svc.open_live_transcript(&owner).unwrap();
    store
        .append(
            &owner,
            TranscriptInput {
                at_ms: 1,
                content: TranscriptContent::Speech {
                    text: "opaque utterance for scout".into(),
                },
            },
        )
        .unwrap();

    let routed = process(
        &svc,
        &store,
        &owner,
        TranscriptRange {
            from_seq: 1,
            to_seq: 1,
        },
    )
    .unwrap();
    assert!(!routed);
    assert_eq!(adapter.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        *adapter.seen_texts.lock(),
        vec!["opaque utterance for scout".to_string()]
    );
    assert_eq!(runtime.status(60).calls_last_hour, 1);
}

#[test]
fn scout_failure_propagates_specific_reason_and_backoff_aligns_readiness_and_status() {
    let (_fixture, svc, owner, _) = fixture();
    let store = svc.open_live_transcript(&owner).unwrap();
    store
        .append(
            &owner,
            TranscriptInput {
                at_ms: 1,
                content: TranscriptContent::Speech {
                    text: "opaque utterance".into(),
                },
            },
        )
        .unwrap();

    let unconfigured_setup = svc.live_transcript_setup();
    let unconfigured_status = svc.live_transcript_status(&owner).unwrap();
    assert_eq!(unconfigured_setup["scout"], "unavailable");
    assert_eq!(unconfigured_status.scout, ScoutStatus::Unavailable);
    let expected_reason = unconfigured_setup["scoutReason"]
        .as_str()
        .expect("readiness should explain unavailability")
        .to_owned();
    assert_ne!(expected_reason, "Scout unavailable");
    assert_eq!(
        unconfigured_status.scout_reason.as_deref(),
        Some(expected_reason.as_str())
    );
    let route_error = process(
        &svc,
        &store,
        &owner,
        TranscriptRange {
            from_seq: 1,
            to_seq: 1,
        },
    )
    .unwrap_err();
    assert_eq!(route_error, expected_reason);

    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(codeterm_supporter::SupporterRuntime::new(
        dir.path().to_path_buf(),
    ));
    let model_id = "cli-fixture/fast-free";
    let adapter = Arc::new(RecordingScoutAdapter::new(
        model_id,
        Err(codeterm_decision::DecisionError::Deadline),
    ));
    svc.supporter_models
        .register("cli-fixture", adapter.clone());
    let _override =
        svc.override_supporter_decision_for_test(fixture_supporter_config(model_id), runtime);

    let timeout_error = process(
        &svc,
        &store,
        &owner,
        TranscriptRange {
            from_seq: 1,
            to_seq: 1,
        },
    )
    .unwrap_err();
    assert!(timeout_error.contains("timed out"), "{timeout_error}");
    assert_ne!(timeout_error, "Scout unavailable");
    assert_eq!(svc.live_transcript_setup()["scout"], "ready");
    assert_eq!(
        svc.live_transcript_status(&owner).unwrap().scout,
        ScoutStatus::Ready
    );

    *adapter.outcome.lock() = Err(codeterm_decision::DecisionError::Unavailable(
        "plugin_unavailable".into(),
    ));
    let outage_error = process(
        &svc,
        &store,
        &owner,
        TranscriptRange {
            from_seq: 1,
            to_seq: 1,
        },
    )
    .unwrap_err();
    assert!(
        outage_error.contains("plugin_unavailable"),
        "{outage_error}"
    );

    let backed_off_setup = svc.live_transcript_setup();
    let backed_off_status = svc.live_transcript_status(&owner).unwrap();
    assert_eq!(backed_off_setup["scout"], "unavailable");
    assert_eq!(backed_off_status.scout, ScoutStatus::Unavailable);
    assert_eq!(
        backed_off_setup["scoutReason"].as_str(),
        backed_off_status.scout_reason.as_deref()
    );
    assert!(backed_off_status
        .scout_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("cooling down")));
}

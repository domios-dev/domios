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
    assert!(scout_segment(1, "   \n\t  ").is_none());
    let ru = scout_segment(2, "Отправь это в Main прямо сейчас").unwrap();
    assert_eq!(ru["seq"], 2);
    assert_eq!(ru["text"], "Отправь это в Main прямо сейчас");
    let with_secret = scout_segment(3, "Bearer sk-1234567890abcdefghijklmnop").unwrap();
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
    append("opaque oversized ".repeat(130));
    let both = TranscriptRange {
        from_seq: 1,
        to_seq: 4,
    };
    let pointer = delivery::route_body(&store, "owner", &binding, both, true, 0);
    assert!(pointer.contains("--revision 1 --from-seq 1 --through-seq 4"));
    assert!(!pointer.contains("opaque"));
}

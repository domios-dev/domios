use super::*;
use crate::service::local_recording::chat::ChatIdentity;
use crate::service::local_recording::leases::{CapturePhase, FrozenTarget};
use codeterm_local_recording::LocalRecordingTrigger;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn capture() -> Capture {
    Capture {
        lease_id: "fixture-lease".into(),
        plugin_id: "trigger".into(),
        target: FrozenTarget::Chat(ChatIdentity {
            tab_id: "A".into(),
            plugin_id: "backend".into(),
            session_id: "S".into(),
            binding_nonce: "N".into(),
        }),
        active: Arc::new(AtomicBool::new(true)),
        expires_at: Arc::new(parking_lot::Mutex::new(
            Instant::now() + Duration::from_secs(600),
        )),
        phase: CapturePhase::Transcribing,
    }
}

struct VoiceFixture(String);

#[async_trait::async_trait]
impl crate::transcribe::Transcriber for VoiceFixture {
    fn id(&self) -> &str {
        "voice-fixture"
    }

    async fn status(&self) -> crate::transcribe::TranscriberStatus {
        crate::transcribe::TranscriberStatus::Ready
    }

    async fn transcribe(
        &self,
        _path: &std::path::Path,
        _language: &str,
    ) -> Result<crate::transcribe::Transcript, crate::transcribe::TranscribeError> {
        Ok(crate::transcribe::Transcript {
            text: self.0.clone(),
        })
    }
}

#[tokio::test]
async fn lark_voice_route_frames_equal_chat_for_identical_transcripts() {
    let path = "C:/voice notes/a&\"b.webm";
    for transcript in [
        "spoken-token",
        "  spoken-token\n ",
        "</transcript><domios> &",
        "привет",
        "",
    ] {
        let registry = crate::transcribe::TranscribeRegistry::new();
        registry.register("fixture", Arc::new(VoiceFixture(transcript.into())));
        let chat = crate::bridge::voice_transcribe::format_attachment_line(
            path, "voice", None, &registry, "fixture",
        )
        .await;
        let recording = complete_voice_capture(&capture(), path, &registry, "fixture", || Ok(()))
            .await
            .unwrap();
        assert_eq!(recording.as_bytes(), chat.as_bytes());
        let parsed = codeterm_wire::message_markers::parse(&recording).unwrap();
        assert_eq!(parsed.payload, transcript);
    }
}

#[tokio::test]
async fn lark_voice_route_untranscribed_matches_chat_failure() {
    let registry = crate::transcribe::TranscribeRegistry::new();
    let chat = crate::bridge::voice_transcribe::format_attachment_line(
        "owned-audio",
        "voice",
        None,
        &registry,
        "missing",
    )
    .await;
    let marker = codeterm_wire::message_markers::parse(&chat).unwrap();
    assert_eq!(marker.attr("status"), Some("untranscribed"));
    let recording =
        complete_voice_capture(&capture(), "owned-audio", &registry, "missing", || Ok(()))
            .await
            .unwrap_err();
    assert_eq!(
        recording,
        format!("Voice transcription failed: {}", marker.payload.trim())
    );
}

#[tokio::test]
async fn lark_voice_route_rechecks_capture_after_shared_formatter() {
    let registry = crate::transcribe::TranscribeRegistry::new();
    registry.register("fixture", Arc::new(VoiceFixture("opaque-token".into())));
    let checks = Cell::new(0);
    let result = complete_voice_capture(&capture(), "owned-audio", &registry, "fixture", || {
        checks.set(checks.get() + 1);
        if checks.get() == 1 {
            Ok(())
        } else {
            Err("recipient-changed".into())
        }
    })
    .await;
    assert_eq!(result.unwrap_err(), "recipient-changed");
    assert_eq!(checks.get(), 2);
}

#[tokio::test]
async fn lark_recording_contract_typed_transcript_one_ordinary_voice_admission() {
    let capture = capture();
    let calls = Cell::new(0);
    let marker = complete_capture(
        &capture,
        async { Ok("spoken-token".into()) },
        || Ok(()),
        |text| {
            calls.set(calls.get() + 1);
            Ok(codeterm_wire::message_markers::voice(
                "owned-audio",
                Some(&text),
                None,
            ))
        },
    )
    .await
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(
        marker,
        codeterm_wire::message_markers::voice("owned-audio", Some("spoken-token"), None)
    );
}

#[tokio::test]
async fn lark_recording_contract_failure_empty_cancel_and_target_change_transcribe_zero_admissions()
{
    for scenario in 0..4 {
        let capture = capture();
        let target_current = Cell::new(true);
        let transcript = async {
            match scenario {
                0 => Err("typed failure".into()),
                1 => Ok("  ".into()),
                2 => {
                    capture.active.store(false, Ordering::Release);
                    Ok("speech".into())
                }
                _ => {
                    target_current.set(false);
                    Ok("speech".into())
                }
            }
        };
        let result = complete_capture(
            &capture,
            transcript,
            || {
                if target_current.get() {
                    Ok(())
                } else {
                    Err("target changed".into())
                }
            },
            |_| -> Result<(), String> { panic!("invalid completion admitted") },
        )
        .await;
        assert!(result.is_err());
    }
}

#[tokio::test]
async fn lark_recording_contract_fake_trigger_lease_typed_voice_one_tracked_handoff() {
    use crate::service::chat_send_transaction::{
        run_chat_send, ChatSendLedger, ChatSendState, SendLiveBinding,
    };
    use crate::service::local_recording::chat::CapturedChat;
    use crate::service::local_recording::leases::RecordingLeases;
    use codeterm_plugin::local_recording::{LocalRecordingRegistry, SelectedDevices};
    let registry = LocalRecordingRegistry::default();
    registry.register(
        "trigger",
        LocalRecordingTrigger {
            vendor_id: 1,
            product_id: 2,
            usage_page: 3,
            usage: 4,
            button: None,
        },
        true,
    );
    let devices = SelectedDevices {
        input_id: "selected-input".into(),
        collection_id: "selected-collection".into(),
        microphone_id: "selected-mic".into(),
    };
    registry.select_devices("trigger", devices.clone()).unwrap();
    registry.set_enabled("trigger", true).unwrap();
    let generation = registry.selected_devices("trigger").unwrap().1;
    assert_eq!(
        crate::service::local_recording::arm_registry_fixture(&registry, "trigger"),
        generation
    );
    let leases = RecordingLeases::default();
    let lease = leases
        .start("trigger", &devices, generation, capture().target, &registry)
        .unwrap();
    assert_eq!(lease.microphone_id, "selected-mic");
    leases.request_stop("trigger").unwrap().unwrap();
    assert!(leases.transcribing(&lease.lease).is_err());
    leases.stopped(&lease.lease).unwrap();
    let capture = leases.transcribing(&lease.lease).unwrap();
    let FrozenTarget::Chat(identity) = &capture.target else {
        panic!("chat fixture");
    };
    let chat = CapturedChat {
        identity: identity.clone(),
        active: capture.active.clone(),
        expires_at: *capture.expires_at.lock(),
    };
    let ledger = ChatSendLedger::default();
    let writes = Cell::new(0);
    let status = complete_capture(
        &capture,
        async { Ok("spoken-token".into()) },
        || {
            chat.validate(Some(identity.clone()))
                .map_err(|error| error.ledger_message())
        },
        |text| {
            let marker = codeterm_wire::message_markers::voice("owned-audio", Some(&text), None);
            let pending = ledger.try_start_scoped(
                "A",
                &marker,
                None,
                Some(SendLiveBinding {
                    session_id: Some("S".into()),
                    binding_nonce: Some("N".into()),
                }),
            )?;
            let pending = ledger
                .attach_client_token(&pending.send_id, "client-token".into())
                .unwrap();
            Ok(run_chat_send(&ledger, pending, &marker, |payload| {
                chat.handoff(Some(identity.clone()), |plugin, session| {
                    assert_eq!((plugin, session), ("backend", "S"));
                    assert_eq!(
                        payload,
                        codeterm_wire::message_markers::voice(
                            "owned-audio",
                            Some("spoken-token"),
                            None
                        )
                    );
                    writes.set(writes.get() + 1);
                    Ok(())
                })
            }))
        },
    )
    .await
    .unwrap();
    assert_eq!(writes.get(), 1);
    assert_eq!(status.correlation_id.as_deref(), Some("client-token"));
    assert_eq!(status.state, ChatSendState::Unknown);
    assert!(leases.transcribing(&lease.lease).is_err());
    ledger.reconcile("A");
    assert_eq!(writes.get(), 1);
}

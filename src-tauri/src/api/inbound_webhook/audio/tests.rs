use super::*;

fn transcribed(text: &str) -> HostAudio {
    HostAudio {
        path: Some("/uploads/abcd-recording.m4a".into()),
        outcome: HostOutcome::Transcribed(text.into()),
    }
}

#[test]
fn pebble_audio_receiver_degraded_marks_empty_failure_markers_and_short_transcripts() {
    assert!(degraded("", None));
    assert!(degraded("   ", Some("hello there")));
    assert!(degraded("turn on the <UNK> light", None));
    assert!(degraded("[BLANK_AUDIO]", None));
    assert!(degraded(
        "on",
        Some("please turn on the porch light right now thanks")
    ));
    assert!(!degraded("on", Some("turn on the light")));
    assert!(!degraded(
        "turn on the porch light",
        Some("turn on the porch light please")
    ));
}

#[test]
fn pebble_audio_receiver_host_wins_unless_only_the_host_is_degraded() {
    assert_eq!(
        choose_primary("lights on", Some("lights on")),
        Primary::Host
    );
    assert_eq!(choose_primary("lights on", None), Primary::Host);
    assert_eq!(choose_primary("<unk>", Some("lights on")), Primary::Sender);
    assert_eq!(choose_primary("<unk>", Some("[inaudible]")), Primary::Host);
    assert_eq!(choose_primary("", None), Primary::Host);
}

#[test]
fn pebble_audio_receiver_render_orders_primary_then_comparison_then_audio_path() {
    let lines = render(
        "client: \"ring\"",
        Some("lights on"),
        Some(&transcribed("Lights on.")),
    );
    assert_eq!(
        lines,
        [
            "client: \"ring\"".to_string(),
            format!("{HOST_PRIMARY_LABEL} Lights on."),
            format!("{SENDER_COMPARISON_LABEL} lights on"),
            format!("{AUDIO_FILE_LABEL} /uploads/abcd-recording.m4a"),
        ]
        .join("\n")
    );
    let swapped = render("meta", Some("lights on"), Some(&transcribed("")));
    assert_eq!(
        swapped,
        [
            "meta".to_string(),
            format!("{SENDER_PRIMARY_LABEL} lights on"),
            format!("{HOST_COMPARISON_LABEL} "),
            format!("{AUDIO_FILE_LABEL} /uploads/abcd-recording.m4a"),
        ]
        .join("\n")
    );
}

#[test]
fn pebble_audio_receiver_render_failure_keeps_the_sender_transcript_and_the_note() {
    let failed = HostAudio {
        path: None,
        outcome: HostOutcome::Failed("note-token".into()),
    };
    assert_eq!(
        render("meta", Some("lights on"), Some(&failed)),
        format!("meta\n{SENDER_PRIMARY_LABEL} lights on\n{HOST_FAILURE_LABEL} note-token")
    );
    assert_eq!(
        render("meta", None, Some(&failed)),
        format!("meta\n{HOST_FAILURE_LABEL} note-token")
    );
}

#[test]
fn pebble_audio_receiver_render_without_audio_labels_the_sender_transcript_only() {
    assert_eq!(
        render("meta", Some("lights on"), None),
        format!("meta\n{SENDER_PRIMARY_LABEL} lights on")
    );
    assert_eq!(render("meta", None, None), "meta");
}

#[test]
fn pebble_audio_receiver_clip_stays_on_a_char_boundary_within_the_text_limit() {
    let long = "\u{1f680}".repeat(MAX_DELIVERY_TEXT_BYTES);
    let clipped = clip(&long);
    assert!(clipped.len() <= MAX_DELIVERY_TEXT_BYTES);
    assert!(clipped.chars().all(|c| c == '\u{1f680}'));
}

#[tokio::test]
async fn pebble_audio_receiver_unstorable_audio_is_an_honest_failure_without_transcription() {
    let blocker = std::env::temp_dir().join(format!(
        "ct-pebble-audio-blocker-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::write(&blocker, b"not a directory").unwrap();
    let registry = TranscribeRegistry::new();
    let host = transcribe(
        &registry,
        "",
        &blocker,
        AudioPart {
            file_name: "audio.m4a".into(),
            media_type: "audio/mp4".into(),
            bytes: axum::body::Bytes::from_static(b"m4a"),
        },
    )
    .await;
    let _ = std::fs::remove_file(&blocker);
    assert_eq!(host.path, None);
    assert!(matches!(host.outcome, HostOutcome::Failed(ref note) if !note.is_empty()));
}

use super::*;

#[test]
fn pending_message_round_trips_requester_identity() {
    let message = PendingMessage {
        text: "opaque update request".into(),
        sender: Some(TurnResumeSender {
            tab_id: "tab-a".into(),
            label: "Agent A".into(),
            from_agent: Some(true),
            from_model: Some("model-a".into()),
        }),
    };

    let serialized = serde_json::to_string(&message).unwrap();
    let restored: PendingMessage = serde_json::from_str(&serialized).unwrap();

    assert_eq!(restored, message);
}

#[test]
fn prior_plain_text_stashes_remain_readable() {
    let restored = parse_pending("opaque legacy text").unwrap();

    assert_eq!(restored.text, "opaque legacy text");
    assert!(restored.sender.is_none());
}

#[test]
fn pending_reads_the_stashed_message_from_its_owned_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(FILENAME);
    let message = PendingMessage {
        text: "opaque update prompt".into(),
        sender: None,
    };
    std::fs::write(&path, serde_json::to_vec(&message).unwrap()).unwrap();

    assert_eq!(pending_at(&path), Some(message));
}

#[test]
fn durable_stash_preserves_requester_and_replaces_only_its_own_record() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("post-update-message.txt");
    let sender = TurnResumeSender {
        tab_id: "caller-a".into(),
        label: "Caller".into(),
        from_agent: Some(true),
        from_model: Some("opaque-model".into()),
    };
    stash_at(&path, "opaque-first", Some(sender.clone())).unwrap();
    assert_eq!(
        pending_at(&path),
        Some(PendingMessage {
            text: "opaque-first".into(),
            sender: Some(sender.clone())
        })
    );
    stash_at(&path, "opaque-second", Some(sender.clone())).unwrap();
    assert_eq!(
        pending_at(&path),
        Some(PendingMessage {
            text: "opaque-second".into(),
            sender: Some(sender)
        })
    );
}

#[test]
fn stash_failure_is_reported_before_handoff_can_be_accepted() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("regular-file");
    std::fs::write(&parent, b"occupied").unwrap();
    assert!(stash_at(&parent.join("handoff"), "opaque-message", None).is_err());
}

use super::*;

#[test]
fn prior_plain_text_stashes_remain_readable() {
    let restored = parse_pending("opaque legacy text").unwrap();

    assert_eq!(restored.text, "opaque legacy text");
    assert!(restored.sender.is_none());
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

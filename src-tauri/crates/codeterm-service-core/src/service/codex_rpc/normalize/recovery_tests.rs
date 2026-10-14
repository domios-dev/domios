use super::*;
use serde_json::json;

fn restored(text: &str) -> Normalizer {
    let mut normalizer = Normalizer::from_history(
        "bound",
        vec![ChatMsg {
            id: "i".into(),
            seq: 9,
            kind: "assistant".into(),
            content: text.into(),
            tool_flow_id: Some("t".into()),
            ..Default::default()
        }],
    );
    normalizer.restore_open_items("bound", &HashMap::from([("i".into(), "t".into())]));
    normalizer
}

fn delta(id: &str, text: &str) -> Frame {
    Frame::Notification {
        method: "item/agentMessage/delta".into(),
        params: json!({"threadId":"bound","turnId":"t","itemId":id,"delta":text}),
    }
}

fn assert_answer(normalizer: &Normalizer, text: &str) {
    let messages = normalizer.snapshot("bound");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, "i");
    assert_eq!(messages[0].seq, 9);
    assert_eq!(messages[0].content, text);
}

#[test]
fn restored_active_schedules_replace_from_item_completion_without_replay() {
    for (snapshot, before, after) in [
        ("hel", vec!["hel", "lo"], vec![]),
        ("hel", vec!["lo"], vec![]),
        ("hello", vec![], vec!["lo"]),
    ] {
        let mut normalizer = restored(snapshot);
        for text in before {
            assert!(normalizer.apply(&delta("i", text)).is_none());
        }
        normalizer.apply(&Frame::Notification {
            method: "item/completed".into(),
            params: json!({"threadId":"bound","turnId":"t","item":{"type":"agentMessage","id":"i","text":"hello"}}),
        }).unwrap();
        for text in after {
            assert!(normalizer.apply(&delta("i", text)).is_none());
        }
        assert_answer(&normalizer, "hello");
        assert!(normalizer.finish_restored_turn("bound", "t").is_empty());
    }
}

#[test]
fn restored_active_schedules_replace_from_interrupted_turn_history_without_completion() {
    for (snapshot, before_read, during_read, after_read) in [
        ("hel", vec!["hel", "lo"], vec![], vec![]),
        ("hel", vec![], vec!["lo"], vec![]),
        ("hello", vec![], vec![], vec!["lo"]),
    ] {
        let mut normalizer = restored(snapshot);
        for text in before_read {
            assert!(normalizer.apply(&delta("i", text)).is_none());
        }
        assert!(normalizer.finish_restored_turn("foreign", "t").is_empty());
        assert!(normalizer.finish_restored_turn("bound", "other").is_empty());
        let ids = normalizer.finish_restored_turn("bound", "t");
        assert_eq!(ids, HashSet::from(["i".into()]));
        assert!(normalizer.finish_restored_turn("bound", "t").is_empty());
        for text in during_read {
            assert!(normalizer.apply(&delta("i", text)).is_none());
        }
        let rows = normalizer.replace_restored_items(
            "bound",
            &ids,
            vec![
                ChatMsg {
                    id: "i".into(),
                    seq: 42,
                    kind: "assistant".into(),
                    content: "hello".into(),
                    ..Default::default()
                },
                ChatMsg {
                    id: "unrelated".into(),
                    content: "foreign".into(),
                    ..Default::default()
                },
            ],
        );
        assert_eq!(rows.len(), 1);
        for text in after_read {
            assert!(normalizer.apply(&delta("i", text)).is_none());
        }
        assert_answer(&normalizer, "hello");
    }
}

#[test]
fn restored_turn_read_failure_retains_snapshot_and_never_requests_again() {
    let mut normalizer = restored("hel");
    assert_eq!(normalizer.finish_restored_turn("bound", "t").len(), 1);
    assert!(normalizer.finish_restored_turn("bound", "t").is_empty());
    assert_answer(&normalizer, "hel");
    assert!(normalizer.apply(&delta("i", "lo")).is_none());
    let new = normalizer.apply(&delta("new", "visible")).unwrap();
    assert_eq!(new.content, "visible");
    assert_eq!(new.seq, 10);
    let new = normalizer.apply(&delta("new", " answer")).unwrap();
    assert_eq!(new.content, "visible answer");
}

#[test]
fn closed_restored_item_never_requests_history_on_turn_completion() {
    let mut normalizer = Normalizer::from_history(
        "bound",
        vec![ChatMsg {
            id: "closed".into(),
            content: "complete".into(),
            ..Default::default()
        }],
    );
    assert!(normalizer.finish_restored_turn("bound", "t").is_empty());
    assert!(normalizer.apply(&delta("closed", "complete")).is_none());
}

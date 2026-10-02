use super::*;
use serde_json::json;

#[test]
fn supporter_journal_rotation_keeps_two_bounded_files() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path().into());
    let value = json!({ "fixture": "x".repeat(MAX_JOURNAL_BYTES as usize / 2) });
    for _ in 0..4 {
        journal.append(&value).unwrap();
    }
    let files: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap())
        .collect();
    assert_eq!(files.len(), 2);
    for file in files {
        assert!(file.metadata().unwrap().len() <= MAX_JOURNAL_BYTES);
        for line in fs::read_to_string(file.path()).unwrap().lines() {
            serde_json::from_str::<Value>(line).unwrap();
        }
    }
}

#[test]
fn supporter_feed_includes_rotated_records_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path().into());
    let old = json!({ "time": 1, "kind": "rerank", "label": "ranked", "action": "rerank", "reason": "all_candidates_scored" });
    let new = json!({ "time": 2, "kind": "tab_state", "label": "other", "action": "weak_hint", "reason": "launch_ready" });
    std::fs::write(
        dir.path().join("journal.previous.jsonl"),
        format!("{old}\n"),
    )
    .unwrap();
    journal.append(&new).unwrap();
    let entries = journal.feed(2, None).unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["time"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert_eq!(journal.feed(1, None).unwrap()[0]["time"], 2);
}

#[test]
fn supporter_journal_question_answer_optional_field_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path().into());
    let mut record = ActionRecord {
        kind: VerdictKind::Rerank,
        tab: None,
        label: "ranked",
        confidence: 0.9,
        action: Action::Rerank,
        reason: "all_candidates_scored",
        asked: None,
    };
    assert!(serde_json::to_value(&record)
        .unwrap()
        .get("asked")
        .is_none());
    let question = "supplied-question";
    let answer = "supplied-answer";
    record.asked = Some(vec![super::super::narrative::QuestionAnswer::new(
        "q", question, answer,
    )]);
    let mut value = serde_json::to_value(record).unwrap();
    value["time"] = json!(1);
    journal.append(&value).unwrap();
    let restored = Journal::new(dir.path().into());
    let feed = restored.feed(10, None).unwrap();
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0]["asked"][0]["question"], question);
    assert_eq!(feed[0]["asked"][0]["answer"], answer);
}

#[test]
fn supporter_journal_scrubs_question_answer_before_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path().into());
    let secret = "journal-boundary-secret";
    let raw = format!("CODETERM_API_TOKEN={secret} {}", "界".repeat(500));
    journal
        .append(&json!({ "asked": [{"name": raw, "question": raw, "answer": raw}] }))
        .unwrap();
    let text = fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    assert!(!text.contains(secret));
    assert!(!text.contains(&"界".repeat(500)));
    let value: Value = serde_json::from_str(text.trim()).unwrap();
    let pairs: Vec<super::super::narrative::QuestionAnswer> =
        serde_json::from_value(value["asked"].clone()).unwrap();
    assert_eq!(pairs.len(), 1);
    assert!(pairs[0].question.chars().count() <= super::super::narrative::MAX_QUESTION_CHARS);
    assert!(pairs[0].answer.chars().count() <= super::super::narrative::MAX_ANSWER_CHARS);
}

#[test]
fn supporter_journal_strong_action_preserves_wire_name() {
    assert_eq!(
        serde_json::to_value(Action::Strong).unwrap(),
        json!("strong_action")
    );
}

#[test]
fn supporter_journal_keeps_observation_identity_and_scrubbed_narrative_together() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path().into());
    let secret = "supplied-query-secret";
    let code = "918273";
    let question = format!(
        "supplied-question https://example.test/auth?token={secret} {code} {}",
        "界".repeat(500)
    );
    let answer = "supplied-answer";
    journal
        .append(&json!({
            "time": 1, "kind": "tab_state", "label": "other", "action": "classification",
            "reason": "model_answer", "observation_id": 42, "decided_by": "adapter",
            "asked": [{"name": "supplied-name", "question": question, "answer": answer}],
        }))
        .unwrap();
    let text = fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    assert!(!text.contains(secret));
    assert!(!text.contains(code));
    let stored: Value = serde_json::from_str(text.trim()).unwrap();
    let feed = Journal::new(dir.path().into()).feed(10, None).unwrap();
    for value in [&stored, &feed[0]] {
        assert_eq!(value["observation_id"], 42);
        assert_eq!(value["decided_by"], "adapter");
        assert_eq!(value["asked"][0]["answer"], answer);
        let question = value["asked"][0]["question"].as_str().unwrap();
        assert!(question.contains("supplied-question"));
        assert!(question.chars().count() <= super::super::narrative::MAX_QUESTION_CHARS);
    }
}

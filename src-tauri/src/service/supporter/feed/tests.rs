use super::*;

#[test]
fn supporter_feed_safe_step_preserves_narrative_and_observation() {
    for reason in ["provider_safe_step_authorized", "provider_safe_step_stale"] {
        let record = serde_json::json!({"time": 1, "kind": "tab_state", "action": "recovery", "label": "provider_update_prompt", "confidence": 0.95, "reason": reason, "decided_by": "adapter", "observation_id": 42, "asked": [{"name": "safe_choice", "question": "opaque-question", "answer": "token=private-value"}]});
        let projected = entry(&record).unwrap();
        assert_eq!(projected["reason"], reason);
        assert_eq!(projected["decided_by"], "adapter");
        assert_eq!(projected["observation_id"], 42);
        assert_eq!(projected["asked"][0]["name"], "safe_choice");
        assert!(!projected.to_string().contains("private-value"));
    }
}
use crate::service::supporter::narrative::{self, QuestionAnswer};

fn record(asked: Value) -> Value {
    json!({ "time": 1, "kind": "rerank", "label": "ranked", "action": "rerank",
        "reason": "all_candidates_scored", "confidence": 0.9, "asked": asked })
}

#[test]
fn supporter_feed_question_answer_from_daemon_fields() {
    let query = "supplied-query";
    let title = "supplied-title";
    let asked = narrative::rerank(query, Some(title), 3);
    let feed = entry(&record(json!(asked))).unwrap();
    assert_eq!(feed["asked"].as_array().unwrap().len(), 1);
    assert!(feed["asked"][0]["question"]
        .as_str()
        .unwrap()
        .contains(query));
    assert!(feed["asked"][0]["answer"].as_str().unwrap().contains(title));
    assert!(feed["asked"][0]["answer"].as_str().unwrap().contains('2'));
}

#[test]
fn supporter_feed_secret_looking_query_scrubbed() {
    let secrets = [
        "secret-value",
        "sk-supplied-private-token",
        "ghp_supplied_private_token",
    ];
    let query = format!(
        "OPENAI_API_KEY={} {} {}",
        secrets[0], secrets[1], secrets[2]
    );
    let asked = narrative::rerank(&query, Some(&query), 1);
    let feed = entry(&record(json!(asked))).unwrap().to_string();
    for secret in secrets {
        assert!(!feed.contains(secret));
    }
    assert!(feed.contains("redacted"));
}

#[test]
fn supporter_feed_long_query_truncated() {
    let query = "界".repeat(500);
    let asked = narrative::rerank(&query, Some("title"), 1);
    let question = &asked[0].question;
    assert_eq!(
        question.matches('界').count(),
        narrative::MAX_QUERY_CHARS - 1
    );
    assert!(question.contains('…'));
    let feed = entry(&record(json!(asked))).unwrap();
    assert!(!feed.to_string().contains(&query));
    assert!(
        feed["asked"][0]["question"]
            .as_str()
            .unwrap()
            .chars()
            .count()
            <= narrative::MAX_QUESTION_CHARS
    );
}

#[test]
fn supporter_feed_projection_scrubs_and_caps_untrusted_journal_fields_again() {
    let secret = "feed-boundary-secret";
    let supplied = format!("Authorization: Bearer {secret} {}", "界".repeat(500));
    let feed = entry(&record(json!([{
        "name": supplied, "question": supplied, "answer": supplied,
        "screen": "raw-screen", "probe_output": "raw-probe",
    }])))
    .unwrap();
    let text = feed.to_string();
    for private in [secret, "raw-screen", "raw-probe"] {
        assert!(!text.contains(private));
    }
    for (field, cap) in [
        ("name", narrative::MAX_NAME_CHARS),
        ("question", narrative::MAX_QUESTION_CHARS),
        ("answer", narrative::MAX_ANSWER_CHARS),
    ] {
        assert!(feed["asked"][0][field].as_str().unwrap().chars().count() <= cap);
    }
    assert_eq!(feed["asked"][0].as_object().unwrap().len(), 3);
}

#[test]
fn supporter_feed_never_contains_screen_or_probe_output() {
    let question = "fixture-question";
    let answer = "fixture-answer";
    let mut value = record(json!([QuestionAnswer::new("q", question, answer)]));
    let private = [
        "screen-fixture",
        "probe-fixture",
        "body-fixture",
        "token-fixture",
    ];
    for (field, text) in ["screen", "probe_output", "body", "token"]
        .iter()
        .zip(private)
    {
        value[*field] = json!(text);
    }
    value["distribution"] = json!({"option": private[0]});
    let feed = entry(&value).unwrap();
    for text in private {
        assert!(!feed.to_string().contains(text));
    }
    assert_eq!(feed["asked"][0]["question"], question);
    assert_eq!(feed["asked"][0]["answer"], answer);
}

#[test]
fn supporter_feed_question_count_is_bounded_and_malformed_pairs_are_omitted() {
    let pairs = vec![json!({"question": "q", "answer": "a"}); narrative::MAX_QUESTIONS + 10];
    let feed = entry(&record(json!(pairs))).unwrap();
    assert_eq!(
        feed["asked"].as_array().unwrap().len(),
        narrative::MAX_QUESTIONS
    );
    for asked in [
        json!("wrong-type"),
        json!([{ "question": "q", "answer": 42 }]),
        json!([]),
    ] {
        assert!(entry(&record(asked)).unwrap().get("asked").is_none());
    }
}

#[test]
fn supporter_feed_keeps_observation_identity_and_capped_narrative_together() {
    let secret = "supplied-query-secret";
    let code = "918273";
    let question = format!(
        "supplied-question https://example.test/auth?token={secret} {code} {}",
        "界".repeat(500)
    );
    let answer = "supplied-answer";
    let value = json!({
        "time": 1, "kind": "tab_state", "label": "other", "action": "classification",
        "reason": "model_answer", "decided_by": "fallback(timeout)", "observation_id": 42,
        "asked": [{"name": "supplied-name", "question": question, "answer": answer}],
    });
    let projected = entry(&value).unwrap();
    assert_eq!(projected["observation_id"], 42);
    assert_eq!(projected["decided_by"], "fallback(timeout)");
    assert_eq!(projected["asked"][0]["answer"], answer);
    let question = projected["asked"][0]["question"].as_str().unwrap();
    assert!(question.contains("supplied-question"));
    assert!(question.chars().count() <= narrative::MAX_QUESTION_CHARS);
    assert!(!projected.to_string().contains(secret));
    assert!(!projected.to_string().contains(code));
}

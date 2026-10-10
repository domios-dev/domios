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

#[test]
fn supporter_feed_projects_only_sanitized_error_categories_for_error_fallbacks() {
    for category in [
        "transport",
        "http",
        "unavailable",
        "unsupported",
        "parse",
        "invalid_observation",
    ] {
        let value = serde_json::json!({
            "time": 1,
            "kind": "tab_state",
            "action": "classification",
            "label": "other",
            "reason": "model_answer",
            "decided_by": "fallback(error)",
            "error_category": category,
            "observation_id": 42,
        });
        let projected = entry(&value).unwrap();
        assert_eq!(projected["error_category"], category);
    }

    let mut invalid = serde_json::json!({
        "time": 1,
        "kind": "tab_state",
        "action": "classification",
        "label": "other",
        "reason": "model_answer",
        "decided_by": "fallback(error)",
        "error_category": "token=private-value",
        "observation_id": 42,
    });
    let projected = entry(&invalid).unwrap();
    assert!(projected.get("error_category").is_none());
    assert!(!projected.to_string().contains("private-value"));

    invalid["decided_by"] = serde_json::json!("fallback(timeout)");
    let projected = entry(&invalid).unwrap();
    assert!(projected.get("error_category").is_none());
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

#[test]
fn supporter_feed_passive_correlation_scrubs_and_rejects_wrong_point_or_identity() {
    let value = json!({
        "time":1, "kind":"tab_state", "action":"classification", "label":"provider_update_prompt",
        "reason":"model_answer", "confidence":0.95, "decided_by":"adapter", "observation_id":42,
        "phase":"classification_completed",
        "decision_point":"prompt_candidate", "observation_status":"completed",
        "model_verdict":{"status":"observed", "label":"provider_update_prompt", "confidence":0.95},
        "alternate_verdict":{"status":"skipped"},
        "correlation":{
            "version":2, "decision_point":"prompt_candidate", "producer":"output_candidate",
            "parent_observation_id":null, "observation_id":42,
            "launch_id":"a".repeat(64), "session_id":null, "occurrence_id":"b".repeat(64),
            "occurrence":3, "frame_hash":"0000000000000007",
            "candidate_evidence":{"status":"unavailable"},
        },
    });
    let feed = entry(&value).unwrap();
    assert_eq!(feed["correlation"], value["correlation"]);
    assert_eq!(feed["decision_point"], "prompt_candidate");
    assert_eq!(feed["observation_status"], "completed");
    assert_eq!(feed["phase"], "classification_completed");
    assert_eq!(feed["alternate_verdict"], json!({"status":"skipped"}));
    for (field, supplied) in [
        ("decision_point", json!("tab_exit")),
        ("observation_id", json!(43)),
        ("screen", json!("private-screen")),
    ] {
        let mut changed = value.clone();
        changed["correlation"][field] = supplied;
        let feed = entry(&changed).unwrap();
        assert!(feed.get("correlation").is_none());
        assert!(!feed.to_string().contains("private-screen"));
    }
    let mut changed = value;
    changed["model_verdict"] =
        json!({"status":"observed", "label":"token=private-model", "confidence":0.95});
    changed["alternate_verdict"] = json!({"status":"skipped", "label":"private-alternate"});
    let feed = entry(&changed).unwrap();
    assert!(!feed.to_string().contains("private-model"));
    assert!(!feed.to_string().contains("private-alternate"));
}

#[test]
fn supporter_feed_action_phase_keeps_correlated_observation() {
    let mut value = json!({
        "time":1, "kind":"tab_state", "action":"recovery", "label":"login_or_auth_required",
        "reason":"provider_safe_step_authorized", "confidence":0.95, "decided_by":"adapter",
        "observation_id":42, "decision_point":"prompt_candidate", "observation_status":"completed",
        "phase":"action_authorized", "asked":[{"name":"safe_choice", "question":"q", "answer":"a"}],
        "correlation": {
            "version":2, "decision_point":"prompt_candidate", "producer":"prompt_confirmation",
            "parent_observation_id":41, "observation_id":42, "launch_id":"a".repeat(64),
            "session_id":null, "occurrence_id":"b".repeat(64), "occurrence":3,
            "frame_hash":"0000000000000007", "candidate_evidence":{"status":"unavailable"}
        }
    });
    let feed = entry(&value).unwrap();
    assert_eq!(feed["phase"], "action_authorized");
    assert_eq!(feed["correlation"]["parent_observation_id"], 41);
    value["correlation"]["parent_observation_id"] = json!(42);
    let invalid = entry(&value).unwrap();
    assert_eq!(invalid["observation_status"], "unavailable");
    assert!(invalid.get("correlation").is_none());
    assert!(invalid.get("phase").is_none());
}

#[test]
fn supporter_feed_passive_no_adapter_source_sets_fallback_honestly() {
    let value = json!({"time":1, "kind":"tab_state", "action":"classification", "label":"other",
        "reason":"model_answer", "decided_by":"fallback(no_adapter)", "observation_id":42,
        "observation_status":"unavailable", "alternate_verdict":{"status":"unavailable"}});
    let feed = entry(&value).unwrap();
    assert_eq!(feed["fallback"], true);
    assert_eq!(feed["observation_status"], "unavailable");
    assert!(feed.get("correlation").is_none());
}

#[test]
fn supporter_feed_tab_title_is_scrubbed_capped_and_optional() {
    let mut value = record(json!([]));
    assert!(entry(&value).unwrap().get("tab_title").is_none());
    value["tab_title"] = json!(format!("sk-private-subject {}", "界".repeat(200)));
    let feed = entry(&value).unwrap();
    let title = feed["tab_title"].as_str().unwrap();
    assert!(!title.contains("sk-private-subject"));
    assert!(title.chars().count() <= narrative::MAX_NAME_CHARS);
    value["tab_title"] = json!("supplied-title\nsecond-line");
    assert_eq!(
        entry(&value).unwrap()["tab_title"],
        "supplied-title second-line"
    );
}

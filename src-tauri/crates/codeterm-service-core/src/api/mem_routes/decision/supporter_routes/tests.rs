use super::*;
use crate::service::supporter::{Action, ActionRecord, SupporterRuntime, VerdictKind};

#[test]
fn supporter_feed_api_bounded_and_team_filtered() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    let team = Arc::new(parking_lot::Mutex::new("team_aabbccdd".to_owned()));
    let lookup = team.clone();
    runtime.attach_visibility(
        Arc::new(move |_| Some(lookup.lock().clone())),
        Arc::new(|_| {}),
    );
    for index in 0..110 {
        *team.lock() = if index % 2 == 0 {
            "team_aabbccdd"
        } else {
            "team_11223344"
        }
        .to_owned();
        runtime.action(ActionRecord {
            kind: VerdictKind::TabState,
            tab: Some("tab"),
            label: "other",
            confidence: 0.9,
            action: Action::WeakHint,
            reason: "launch_ready",
            asked: None,
        });
    }
    let query = Query::<SupporterFeedQuery>::try_from_uri(
        &"/api/supporter/activity?limit=999&team_id=team_aabbccdd"
            .parse()
            .unwrap(),
    )
    .unwrap()
    .0;
    let filtered = feed_snapshot(&runtime, query).unwrap();
    let entries = filtered["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 55);
    assert!(entries
        .iter()
        .all(|entry| entry["team_id"] == "team_aabbccdd"));
    let all = feed_snapshot(
        &runtime,
        SupporterFeedQuery {
            limit: Some(usize::MAX),
            team_id: None,
        },
    )
    .unwrap();
    assert_eq!(all["entries"].as_array().unwrap().len(), 100);
    let empty = feed_snapshot(
        &runtime,
        SupporterFeedQuery {
            limit: Some(0),
            team_id: None,
        },
    )
    .unwrap();
    assert_eq!(empty["entries"], json!([]));
    assert!(Query::<SupporterFeedQuery>::try_from_uri(
        &"/api/supporter/activity?limit=invalid".parse().unwrap()
    )
    .is_err());
}

#[test]
fn supporter_feed_api_redacts_untrusted_journal_fields() {
    let dir = tempfile::tempdir().unwrap();
    let secret = "sk-private-opaque-token";
    let screen = "private shell output\nrequest body";
    let record = json!({ "time": 1700000000, "team_id": "team_aabbccdd", "tab": secret, "kind": "tab_state",
        "label": screen, "confidence": 0.9, "action": secret, "reason": screen, "screen": screen,
        "token": secret, "body": screen, "distribution": { "choice": secret }, "budget": { "reached": false, "screen": screen } });
    let mut verdict = record.clone();
    verdict["kind"] = json!("rerank");
    verdict["action"] = json!("verdict");
    verdict["question_count"] = json!(10);
    std::fs::write(
        dir.path().join("journal.jsonl"),
        format!("{record}\n{verdict}\nmalformed\n"),
    )
    .unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    let value = feed_snapshot(
        &runtime,
        SupporterFeedQuery {
            limit: None,
            team_id: None,
        },
    )
    .unwrap();
    let text = value.to_string();
    assert!(!text.contains(secret));
    assert!(!text.contains(screen));
    assert_eq!(value["entries"][0]["label"], "redacted");
    assert_eq!(value["entries"][0]["action"], "notify_only");
    assert_eq!(value["entries"][0]["reason"], "redacted");
    assert!(value["entries"][0].get("distribution").is_none());
    assert_eq!(value["entries"].as_array().unwrap().len(), 2);
    for entry in value["entries"].as_array().unwrap() {
        assert!(entry.get("distribution").is_none());
        assert!(entry.get("tab").is_none());
        assert_eq!(entry["tab_id"], Value::Null);
    }
}

#[test]
fn supporter_spend_status_exposes_today_month_and_credit_state() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    let price = codeterm_decision::DecisionPricing {
        display_name: "Fixture".into(),
        input_usd_per_million: 0.5,
        output_usd_per_million: 0.5,
    };
    runtime.record_spend(
        "fixture",
        Some(&price),
        &Ok(codeterm_decision::DecisionResponse {
            answers: Default::default(),
            usage: Some(codeterm_decision::DecisionUsage {
                input_tokens: 1_000_000,
                output_tokens: 0,
            }),
        }),
    );
    let spend = spend_snapshot(&runtime, "fixture", Some(&price));
    assert_eq!(spend["today"]["estimated_usd"], 0.5);
    assert_eq!(spend["month"]["estimated_usd"], 0.5);
    let unpriced = spend_snapshot(&runtime, "fixture", None);
    assert_eq!(unpriced["today"]["estimated_usd"], Value::Null);
    assert_eq!(unpriced["month"]["estimated_usd"], Value::Null);
}

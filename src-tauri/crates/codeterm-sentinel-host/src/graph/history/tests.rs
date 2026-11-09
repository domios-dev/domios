use super::*;
use crate::handlers::Transition;
use serde_json::json;

#[test]
fn retained_history_is_bounded_newest_first_without_private_output() {
    let mut job: Sentinel = serde_json::from_value(json!({
        "id":"node","workspace_id":"workspace","team_id":null,"creator_tab_id":"creator",
        "cwd":"private-directory","probe":"private-code","question":"Ready?","buckets":["ready"],
        "routes":{},"matches":{},"interval_ms":10000,"next_run":0,"expires_at":null,
        "until":null,"last_reason":null,
        "machine":{"paused":false,"stopped":false,"last_hash":null,"last_bucket":null}
    }))
    .unwrap();
    job.transitions = (0..70)
        .map(|index| Transition {
            causal_run: None,
            id: format!("event-{index}"),
            bucket: "ready".into(),
            accepted_at: index,
            terminal: false,
            handlers: vec![],
            decision: Some(crate::provenance::Decision {
                decided_by: crate::provenance::DecidedBy::Adapter,
                adapter_id: Some("fixture-adapter".into()),
                model: Some("fixture-model".into()),
                confidence: Some(0.87),
            }),
        })
        .collect();
    let history = project(&job, &mut 4096);
    let history = json!(history).as_array().unwrap().clone();
    assert_eq!(history.len(), MAX_TRANSITIONS);
    assert_eq!(history[0]["id"], "event-69");
    assert_eq!(history[63]["id"], "event-6");
    assert_eq!(history[0]["decided_by"], "adapter");
    assert_eq!(history[0]["adapter_id"], "fixture-adapter");
    assert_eq!(history[0]["model"], "fixture-model");
    assert_eq!(history[0]["confidence"], 0.87);
    assert_eq!(project(&job, &mut 2).len(), 2);
    assert!(!json!(history).to_string().contains("private-"));
}

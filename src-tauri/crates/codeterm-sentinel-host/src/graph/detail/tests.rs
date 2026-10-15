use super::*;
use crate::{
    classify::with_pattern_precedence,
    handlers::Transition,
    machine::{self, Verdict},
    provenance::{Classification, Scope},
    source::TabBinding,
};
use std::cell::Cell;

fn job() -> Sentinel {
    serde_json::from_value(json!({
        "id":"node", "workspace_id":"workspace", "team_id":"team", "creator_tab_id":"creator",
        "cwd":"private-directory", "probe":"printf ready", "question":"Ready?", "buckets":["done"],
        "routes":{"done":[{"id":"build","action":{"kind":"command","target":{
            "command":"make check", "label":"Check", "timeout_ms":1000}},"target":null}]},
        "matches":{"done":"^ready$"}, "interval_ms":10000, "next_run":20000,
        "expires_at":null,"until":null,"last_reason":null,
        "machine":{"paused":false,"stopped":false,"last_hash":null,"last_bucket":null},"agent_authored":true
    })).unwrap()
}

fn detail(job: &Sentinel, workspace: &str, team: Option<&str>) -> Value {
    let (mux, _) = MultiplexerState::new();
    project(job, &mux, workspace, team, 1000, false)
}

#[test]
fn definition_access_requires_exact_team_and_workspace() {
    let job = job();
    let value = detail(&job, "workspace", Some("team"));
    assert_eq!(
        value["scope"],
        json!({"workspace_id":"workspace","team_id":"team","node_id":"node"})
    );
    assert_eq!(value["definition"]["probe"], "printf ready");
    assert_eq!(value["definition"]["question"], "Ready?");
    assert_eq!(
        value["definition"]["buckets"],
        json!([{"name":"done","pattern":"^ready$"}])
    );
    assert_eq!(
        value["definition"]["commands"],
        json!([{"bucket":"done","handler_id":"build",
        "label":"Check","command":"make check","command_truncated":false,"timeout_ms":1000}])
    );
    for (workspace, team) in [
        ("foreign", Some("team")),
        ("workspace", Some("foreign")),
        ("workspace", None),
    ] {
        assert!(detail(&job, workspace, team)["definition"]["unavailable"].is_string());
    }
    assert!(!value.to_string().contains("private-directory"));
}

#[test]
fn opened_solo_definition_requires_its_exact_creation_scope() {
    let mut job = job();
    job.team_id = None;
    assert_eq!(
        detail(&job, "workspace", None)["definition"]["probe"],
        "printf ready"
    );
    assert!(detail(&job, "workspace", Some("team"))["definition"]["unavailable"].is_string());
    assert!(detail(&job, "foreign", None)["definition"]["unavailable"].is_string());
}

#[test]
fn definition_text_is_redacted_bounded_and_explicitly_truncated() {
    let mut job = job();
    job.set_probe(format!(
        "password=credential-fixture\n{}",
        "\u{03bb}".repeat(3000)
    ));
    if let Route::Command(command) = &mut job.routes.get_mut("done").unwrap()[0].action {
        command.command = format!(
            "{}\nhttps://user:pass@example.test/?token=credential-fixture",
            "a".repeat(5000)
        );
    }
    let value = definition(&job);
    assert_eq!(value["probe_truncated"], true);
    assert!(value["probe"].as_str().unwrap().len() <= MAX_TEXT_BYTES);
    assert_eq!(value["commands"][0]["command_truncated"], true);
    assert!(!value.to_string().contains("credential-fixture"));
    job.matches.insert("done".into(), "x".repeat(5000));
    assert_eq!(
        definition(&job),
        json!({"unavailable":"definition_exceeds_v1_bounds"})
    );
}

#[test]
fn origin_and_next_check_do_not_infer_missing_evidence() {
    let mut job = job();
    assert_eq!(detail(&job, "workspace", Some("team"))["origin"], "agent");
    job.agent_authored = false;
    assert_eq!(detail(&job, "workspace", Some("team"))["origin"], "unknown");
    assert!(detail(&job, "workspace", Some("team"))["creator"]["name"].is_null());
    assert_eq!(
        detail(&job, "workspace", Some("team"))["next_check_at_ms"],
        20000
    );
    job.machine.paused = true;
    assert!(detail(&job, "workspace", Some("team"))["next_check_at_ms"].is_null());
    job.machine.paused = false;
    job.set_observe(Some(TabBinding::CronRun("cron".into())));
    let value = detail(&job, "workspace", Some("team"));
    assert_eq!(
        value["observe_target"],
        json!({"kind":"cron_run","id":"cron"})
    );
    assert!(value["next_check_at_ms"].is_null());
    job.set_observe(Some(TabBinding::Tab("watched".into())));
    assert_eq!(
        detail(&job, "workspace", Some("team"))["observe_target"],
        json!({"kind":"tab","id":"watched"})
    );
}

fn evaluate(
    job: &mut Sentinel,
    output: &str,
    adapter: impl FnOnce() -> Classification,
) -> Decision {
    let scope = Scope::from(&*job);
    let classification = with_pattern_precedence(job, output, adapter);
    machine::evaluate(
        &mut job.machine,
        output.as_bytes(),
        &job.buckets,
        &job.routes,
        None,
        scope,
        || classification,
    )
    .evidence
    .decision
    .unwrap()
}

#[test]
fn connected_adapter_does_not_replace_pattern_provenance() {
    let mut job = job();
    let calls = Cell::new(0);
    let decision = evaluate(&mut job, "ready", || {
        calls.set(calls.get() + 1);
        Classification::adapter(Ok(Verdict {
            bucket: "done".into(),
            confidence: 0.9,
        }))
    });
    assert_eq!(calls.get(), 0);
    let mut result = json!({});
    project_decision(&mut result, Some(&decision));
    assert_eq!(
        result,
        json!({"decided_by":"pattern","adapter_id":null,"model":null,"confidence":1.0})
    );
}

#[test]
fn explicit_model_records_adapter_and_incomplete_history_is_unknown() {
    let mut job = job();
    job.matches.clear();
    job.model_classification = true;
    let decision = evaluate(&mut job, "ambiguous", || {
        let mut classification = Classification::adapter(Ok(Verdict {
            bucket: "done".into(),
            confidence: 0.9,
        }));
        classification.adapter_id = Some("adapter".into());
        classification.model = Some("model".into());
        classification
    });
    let mut result = json!({});
    project_decision(&mut result, Some(&decision));
    assert_eq!(
        result,
        json!({"decided_by":"adapter","adapter_id":"adapter","model":"model","confidence":0.9})
    );
    job.transitions.push(Transition {
        causal_run: None,
        id: "accepted".into(),
        bucket: "done".into(),
        accepted_at: 1000,
        terminal: false,
        handlers: vec![],
        decision: Some(decision),
    });
    assert_eq!(
        detail(&job, "workspace", Some("team"))["decisions"],
        json!({"history_complete":true,"adapter_used":true})
    );
    job.transitions[0].decision = None;
    assert_eq!(
        detail(&job, "workspace", Some("team"))["decisions"],
        json!({"history_complete":false,"adapter_used":null})
    );
}

#[test]
fn fallback_is_configuration_and_does_not_claim_adapter_use() {
    let job = job();
    assert_eq!(
        configured_fallback(Some("adapter"), Some("model")),
        json!({"adapter_id":"adapter","model":"model"})
    );
    assert_eq!(configured_fallback(None, None), Value::Null);
    assert_eq!(
        detail(&job, "workspace", Some("team"))["decisions"],
        json!({"history_complete":true,"adapter_used":false})
    );
}

#[test]
fn invalid_history_provenance_is_not_complete() {
    let mut job = job();
    job.transitions.push(Transition {
        causal_run: None,
        id: "invalid".into(),
        bucket: "done".into(),
        accepted_at: 1000,
        terminal: false,
        handlers: vec![],
        decision: Some(Decision {
            decided_by: DecidedBy::Adapter,
            adapter_id: None,
            model: None,
            confidence: Some(2.0),
        }),
    });
    assert_eq!(
        detail(&job, "workspace", Some("team"))["decisions"],
        json!({"history_complete":false,"adapter_used":null})
    );
}

#[test]
fn missing_node_explains_unavailable_definition_and_history() {
    let value = missing(&crate::graph::identity(&job()));
    assert_eq!(value["scope"]["node_id"], "node");
    assert_eq!(value["definition"], json!({"unavailable":"node_missing"}));
    assert_eq!(value["origin"], "unknown");
    assert_eq!(
        value["decisions"],
        json!({"history_complete":false,"adapter_used":null})
    );
    let mut transition = json!({});
    project_decision(&mut transition, None);
    assert_eq!(
        transition,
        json!({"decided_by":null,"adapter_id":null,"model":null,"confidence":null})
    );
}

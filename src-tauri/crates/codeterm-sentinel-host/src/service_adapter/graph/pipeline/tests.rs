use super::*;
use crate::store::{Owner, Store};
use codeterm_blueprint_types::blueprint_pipeline::StepKind;
use codeterm_service_core::cron::clock::MockClock;
use std::collections::BTreeMap;

fn owner() -> Owner {
    Owner {
        creator_tab_id: "creator".into(),
        team_id: Some("team".into()),
        workspace_id: "workspace".into(),
        cwd: ".".into(),
    }
}

fn project(job: &Sentinel) -> Value {
    project_snapshot(job, &BTreeMap::new()).blueprints.remove(0)
}

fn project_snapshot(
    job: &Sentinel,
    journals: &BTreeMap<String, Vec<BlueprintJournalEntry>>,
) -> Snapshot {
    let mut snapshot = Snapshot {
        workspace_id: job.workspace_id.clone(),
        team_id: job.team_id.clone(),
        all_teams: false,
        revision: "revision".into(),
        scenarios: Vec::new(),
        nodes: vec![json!({"identity":graph::identity(job)})],
        edges: Vec::new(),
        truncated: false,
        configured_fallback: Value::Null,
        runs: vec![],
        blueprints: vec![],
    };
    project_pipeline_contract(
        &mut snapshot,
        std::slice::from_ref(job),
        journals,
        &BTreeSet::new(),
        &codeterm_plugin::graph_events::SubscriptionTable::new(),
    );
    snapshot
}

#[test]
fn blueprint_pipeline_team_owner_defaults_result_target_to_team() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(
        directory.path().join("blueprints.json"),
        MockClock::starting_at(1000),
    );
    let document = json!({"schema_version":2,"kind":"pipeline","name":"team pipeline","owner":"team","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"emit","run":{"lit":"printf ready"}}],"result":{"to":"owner"}});
    let job = store.prepare_document(document, owner(), true).unwrap();

    let node = project(&job);
    assert_eq!(node["owner"]["kind"], "team");
    assert!(node["owner"]["tab_id"].is_null());
    assert_eq!(node["owner"]["team_id"], "team");
    assert_eq!(node["result"]["target"]["kind"], "team");
    assert_eq!(node["result"]["target"]["id"], "team");
}

#[test]
fn blueprint_pipeline_read_model_revision_tracks_journal_rows() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(
        directory.path().join("blueprints.json"),
        MockClock::starting_at(1000),
    );
    let document = json!({"schema_version":2,"kind":"pipeline","name":"revision pipeline","owner":"tab","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"emit","run":{"lit":"echo ready"}}],"result":{"to":"owner"}});
    let job = store.prepare_document(document, owner(), true).unwrap();
    let before = project_snapshot(&job, &BTreeMap::new()).revision;
    let journals = BTreeMap::from([(
        job.id.clone(),
        vec![BlueprintJournalEntry {
            entry_id: 17,
            run_id: "run_revision".into(),
            step_index: 0,
            step_name: "emit".into(),
            state: "done".into(),
            outcome: Some("done".into()),
            started_at_ms: 1000,
            ended_at_ms: Some(1100),
            result_summary: Some("step completed".into()),
            reason: None,
            body_ref: None,
        }],
    )]);

    let after = project_snapshot(&job, &journals).revision;
    assert_ne!(before, after);
}

#[test]
fn blueprint_delivery_read_model_projects_reason_and_body_reference() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(
        directory.path().join("blueprints.json"),
        MockClock::starting_at(1000),
    );
    let document = json!({"schema_version":2,"kind":"pipeline","name":"result delivery","owner":"tab","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"emit","run":{"lit":"echo ready"}}],"result":{"to":"owner"}});
    let job = store.prepare_document(document, owner(), true).unwrap();
    let journal = BlueprintJournalEntry {
        entry_id: 17,
        run_id: "run_result".into(),
        step_index: -1,
        step_name: "$result".into(),
        state: "failed".into(),
        outcome: Some("handler_unconfirmed".into()),
        started_at_ms: 1000,
        ended_at_ms: Some(1100),
        result_summary: Some("result delivery unconfirmed".into()),
        body_ref: Some("inbox/result.md".into()),
        reason: Some("delivery_pending".into()),
    };
    let node = project_snapshot(&job, &BTreeMap::from([(job.id.clone(), vec![journal])]))
        .blueprints
        .remove(0);

    assert_eq!(node["result"]["state"], "failed");
    assert_eq!(node["result"]["reason"], "delivery_pending");
    assert_eq!(node["result"]["body_ref"], "inbox/result.md");
}

fn contract_job() -> Sentinel {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(
        directory.path().join("blueprints.json"),
        MockClock::starting_at(1000),
    );
    let mut job = store
        .prepare_document(
            json!({"schema_version":2,"kind":"pipeline","name":"Echo pipeline","owner":"tab","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"emit","run":{"lit":"echo ready"}},{"id":"match","uses":"filter","with":{"pattern":{"lit":"ready"}}}],"result":{"to":"notify"}}),
            owner(),
            false,
        )
        .unwrap();
    job.id = "blueprint-echo".into();
    job.scenario_id = Some(job.id.clone());
    job
}

fn row(
    id: i64,
    step: i64,
    name: &str,
    state: &str,
    outcome: Option<&str>,
) -> BlueprintJournalEntry {
    BlueprintJournalEntry {
        entry_id: id,
        run_id: "run-echo".into(),
        step_index: step,
        step_name: name.into(),
        state: state.into(),
        outcome: outcome.map(Into::into),
        started_at_ms: 1000 + id as u64,
        ended_at_ms: (state != "running").then_some(1100 + id as u64),
        result_summary: None,
        reason: None,
        body_ref: None,
    }
}

fn closed_rows() -> Vec<BlueprintJournalEntry> {
    vec![
        row(1, -1, "$run", "running", None),
        row(2, 0, "emit", "running", None),
        row(3, 0, "emit", "done", Some("done")),
        row(4, 1, "match", "running", None),
        row(5, 1, "match", "done", Some("done")),
        row(6, -1, "$result", "done", Some("handler_result")),
        row(7, -1, "$run", "done", Some("done")),
    ]
}

#[test]
fn pipeline_closed_run_is_idle_with_no_current_step() {
    let job = contract_job();
    let mut snapshot = project_snapshot(&job, &BTreeMap::from([(job.id.clone(), closed_rows())]));
    project_pipeline_contract(
        &mut snapshot,
        std::slice::from_ref(&job),
        &BTreeMap::from([(job.id.clone(), closed_rows())]),
        &BTreeSet::from([job.id.clone()]),
        &codeterm_plugin::graph_events::SubscriptionTable::new(),
    );
    let blueprint = &snapshot.blueprints[0];
    assert_eq!(blueprint["state"], "idle");
    assert!(blueprint["current_step"].is_null());
    assert_eq!(blueprint["steps"][0]["state"], "done");
    assert_eq!(blueprint["runs"][0]["state"], "completed");
    assert_eq!(snapshot.nodes[0]["state"], "idle");
}

#[test]
fn pipeline_newest_step_row_wins_over_old_running_rows() {
    let job = contract_job();
    let rows = vec![
        row(4, 1, "match", "running", None),
        row(2, 0, "emit", "running", None),
        row(3, 0, "emit", "done", Some("done")),
        row(1, -1, "$run", "running", None),
    ];
    let snapshot = project_snapshot(&job, &BTreeMap::from([(job.id.clone(), rows)]));
    assert_eq!(snapshot.blueprints[0]["current_step"], 1);
    assert_eq!(snapshot.blueprints[0]["steps"][0]["state"], "done");
    assert_eq!(snapshot.blueprints[0]["steps"][1]["state"], "running");
}

#[test]
fn pipeline_contract_golden_json() {
    let job = contract_job();
    assert!(job.is_scenario_root());
    assert!(!job.agent_authored);
    assert!(
        job.buckets.is_empty()
            && job.routes.is_empty()
            && job.matches.is_empty()
            && job.question.is_empty()
    );
    let mut snapshot = project_snapshot(&job, &BTreeMap::from([(job.id.clone(), closed_rows())]));
    snapshot.scenarios.push(graph::Scenario {
        id: job.id.clone(),
        name: "Echo pipeline".into(),
        root: graph::identity(&job),
    });
    snapshot.nodes[0]["stop"] = json!({"allowed":true,"reason":null});
    let value = serde_json::to_value(snapshot).unwrap();
    assert_eq!(value["blueprints"][0]["steps"][1]["name"], "match");
    assert!(value["nodes"][0].get("blueprint").is_none());
    let path = codeterm_service_core::test_support::fixture_manifest_dir()
        .join("../../../tests/fixtures/blueprint-pipeline.golden.json");
    let rendered = format!("{}\n", serde_json::to_string_pretty(&value).unwrap());
    if std::env::var_os("UPDATE_BLUEPRINT_GOLDEN").is_some() {
        std::fs::write(&path, &rendered).unwrap();
    }
    let expected: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value, expected);
    let output = tempfile::tempdir().unwrap();
    std::fs::write(
        output.path().join("blueprint-pipeline.golden.json"),
        rendered,
    )
    .unwrap();
}

#[test]
fn pipeline_retained_runs_keep_distinct_newest_step_states() {
    let job = contract_job();
    let mut rows = closed_rows();
    let mut next = row(8, -1, "$run", "running", None);
    next.run_id = "run-next".into();
    rows.push(next);
    let mut next = row(9, 0, "emit", "running", None);
    next.run_id = "run-next".into();
    rows.push(next);
    let snapshot = project_snapshot(&job, &BTreeMap::from([(job.id.clone(), rows)]));
    assert_eq!(snapshot.blueprints[0]["state"], "running");
    assert_eq!(snapshot.blueprints[0]["runs"][0]["id"], "run-next");
    assert_eq!(
        snapshot.blueprints[0]["runs"][0]["steps"][0]["state"],
        "running"
    );
    assert_eq!(snapshot.blueprints[0]["runs"][1]["id"], "run-echo");
    assert_eq!(
        snapshot.blueprints[0]["runs"][1]["steps"][0]["state"],
        "done"
    );
}

#[test]
fn pipeline_without_flat_stages_survives_store_reload() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("blueprints.json");
    let mut store = Store::new(path.clone(), MockClock::starting_at(1000));
    let job = contract_job();
    store.insert(job.clone()).unwrap();
    let loaded = Store::new(path, MockClock::starting_at(1000));
    loaded.ensure_available().unwrap();
    assert!(loaded.get(&job.id).unwrap().routes.is_empty());
    assert_eq!(
        loaded.get(&job.id).unwrap().pipeline().cloned(),
        job.pipeline().cloned()
    );
}

#[test]
fn blueprint_pipeline_github_source_exposes_scope_without_command_text() {
    use codeterm_blueprint_types::blueprint_github::{GithubProbe, GithubResource};
    let kind = StepKind::Github {
        probe: GithubProbe {
            repo: "cli/cli".into(),
            resource: GithubResource::Run,
            id: 39652641013,
            org: None,
            label: None,
            timeout_ms: 60000,
        },
    };
    let view = pipeline_step_view(0, "Check CI", &kind, None, None);
    assert_eq!(view["kind"], "github");
    assert_eq!(
        view["source"],
        json!({"repo":"cli/cli", "resource":"run", "id":39652641013u64})
    );
    assert_eq!(view["state"], "waiting");
    assert!(view.get("command").is_none());
}

#[test]
fn source_cards_project_retained_observation_states_and_sanitized_http_scope() {
    for (kind, state, summary) in [
        (
            json!({"kind":"github", "probe":{"repo":"owner/project","resource":"run","id":42}}),
            "failing",
            "failing · Linux",
        ),
        (
            json!({"kind":"github", "probe":{"org":"owner","resource":"runners","label":"linux-build"}}),
            "online",
            "online · 2 online",
        ),
        (
            json!({"kind":"http", "probe":{"url":"https://example.com/health?token=private"}}),
            "down",
            "down · 503",
        ),
    ] {
        let kind: StepKind = serde_json::from_value(kind).unwrap();
        let mut event = row(1, 0, "check", "done", Some(state));
        event.result_summary = Some(summary.into());
        let event_ref = &event;
        let view = pipeline_step_view(
            0,
            "Renamed imported step",
            &kind,
            Some(&event_ref),
            Some(&event_ref),
        );
        assert_eq!(view["source_state"], state);
        assert_eq!(view["result_summary"], summary);
        assert_eq!(view["kind"], kind.name());
        if kind.name() == "http" {
            assert_eq!(view["source"]["url"], "https://example.com/health");
        }
    }
}

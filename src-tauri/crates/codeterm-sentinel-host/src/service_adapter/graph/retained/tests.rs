use super::*;
use crate::store::{Owner, Store};
use codeterm_blueprint_types::blueprint_document::calls;
use codeterm_service_core::cron::clock::MockClock;

fn document(body: &str) -> Value {
    let value = json!({"schema_version":2,"kind":"pipeline","name":"retained","trigger":{"type":"schedule","every":{"lit":"1m"}},
        "steps":[{"id":"invoke","call":{"ref":"callee","with":{"p":{"lit":"input"}}},"out":{"type":"string"}}],
        "blueprints":{"callee":{"inputs":{"p":{"type":"string"}},"steps":[{"id":"invoke","run":{"lit":body},"in":{"type":"string"},"out":{"type":"string"}}],"result":{"type":{"type":"string"},"from":"invoke.out"}}},"result":{"to":"owner"}});
    json!(
        calls::resolve_local("fixture.yaml", &serde_yaml::to_string(&value).unwrap())
            .unwrap()
            .document()
    )
}
fn owner() -> Owner {
    Owner {
        creator_tab_id: "tab".into(),
        team_id: Some("team".into()),
        workspace_id: "workspace".into(),
        cwd: ".".into(),
    }
}
fn row(
    id: i64,
    run: &str,
    version: Option<u64>,
    index: i64,
    parent: Option<i64>,
) -> BlueprintJournalEntry {
    serde_json::from_value(json!({"entry_id":id,"run_id":run,"definition_version":version,"step_index":index,"parent_step_index":parent,
        "step_name":"invoke","state":"failed","outcome":"failed","started_at_ms":id,"reason":"contract_violation"})).unwrap()
}
fn snapshot(job: &Sentinel) -> Snapshot {
    Snapshot {
        workspace_id: job.workspace_id.clone(),
        team_id: job.team_id.clone(),
        all_teams: false,
        revision: "revision".into(),
        nodes: vec![json!({"identity":graph::identity(job)})],
        scenarios: vec![],
        edges: vec![],
        runs: vec![],
        blueprints: vec![],
        configured_fallback: Value::Null,
        quarantined: Vec::new(),
        truncated: false,
    }
}

#[test]
fn blueprint_v2_project_each_run_uses_its_immutable_callee_and_own_journal() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::new(
        temp.path().join("blueprints.json"),
        MockClock::starting_at(1000),
    );
    let old = document("printf old");
    let current = document("printf current");
    let mut job = store
        .prepare_document(current.clone(), owner(), true)
        .unwrap();
    job.definition_version = 2;
    let rows = vec![
        row(20, "current", Some(2), 0, None),
        row(19, "current", Some(2), 1, Some(0)),
        row(10, "old", Some(1), 0, None),
        row(9, "old", Some(1), 1, Some(0)),
        row(5, "unknown", None, 0, None),
        row(4, "evicted", Some(99), 0, None),
    ];
    let journals = BTreeMap::from([(job.id.clone(), rows)]);
    let versions = BTreeMap::from([(
        job.id.clone(),
        BTreeMap::from([(1, old.clone()), (2, current.clone())]),
    )]);
    let mut view = snapshot(&job);
    pipeline::project_pipeline_contract(
        &mut view,
        std::slice::from_ref(&job),
        &journals,
        &BTreeSet::new(),
        &codeterm_plugin::graph_events::SubscriptionTable::new(),
    );
    project(
        &mut view,
        std::slice::from_ref(&job),
        &journals,
        &versions,
        &Default::default(),
    )
    .unwrap();
    let root = &view.blueprints[0];
    assert_eq!(root["graph_definition_version"], 2);
    assert_eq!(
        root["expanded_calls"][0]["graph"]["steps"][0]["run"]["lit"],
        "printf current"
    );
    for item in root["runs"].as_array().unwrap() {
        let id = item["id"].as_str().unwrap();
        assert!(item["nested_run"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["run_id"] == id));
        let expected = match id {
            "current" => Some((&current, 2)),
            "old" => Some((&old, 1)),
            _ => None,
        };
        let document =
            expected.map(|(doc, _)| serde_json::from_value::<Document>(doc.clone()).unwrap());
        let rows = journals[&job.id]
            .iter()
            .filter(|row| row.run_id == id)
            .cloned()
            .collect::<Vec<_>>();
        let expected = crate::blueprint_calls::project_retained(
            document.as_ref(),
            &rows,
            expected
                .map(|(_, version)| version)
                .or_else(|| (id == "evicted").then_some(99)),
            &Default::default(),
        )
        .unwrap();
        for field in [
            "call_graph",
            "expanded_calls",
            "nested_run",
            "historical_graph_available",
            "graph_definition_version",
        ] {
            assert_eq!(item[field], expected[field], "{id}: {field}");
        }
    }
    let historical = &root["runs"][1];
    assert_ne!(
        historical["expanded_calls"][0]["pin"]["hash"],
        root["expanded_calls"][0]["pin"]["hash"]
    );
    assert_eq!(historical["nested_run"]["nodes"][1]["parent_step_index"], 0);
    assert_eq!(root["runs"][2]["historical_graph_available"], false);
    assert_eq!(root["runs"][3]["call_graph"], Value::Null);
}

#[test]
fn blueprint_v2_project_watch_and_unstarted_pipeline_have_current_graph_without_invented_evidence()
{
    let temp = tempfile::tempdir().unwrap();
    let store = Store::new(
        temp.path().join("blueprints.json"),
        MockClock::starting_at(1000),
    );
    let watch = json!({"schema_version":2,"kind":"watch","name":"watch","steps":[
        {"id":"source","uses":"probe","with":{"command":{"lit":"printf ready"}},"every":{"lit":"10s"},"timeout":{"lit":"1h"}},
        {"id":"decide","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":["done"],"matches":{"done":{"lit":"^ready$"}},"routes":{"done":"@stop"}}]});
    for source in [watch, document("printf current")] {
        let job = store
            .prepare_document(source.clone(), owner(), true)
            .unwrap();
        let definitions = BTreeMap::from([(
            job.id.clone(),
            BTreeMap::from([(job.definition_version, source.clone())]),
        )]);
        let mut view = snapshot(&job);
        project(
            &mut view,
            std::slice::from_ref(&job),
            &BTreeMap::new(),
            &definitions,
        )
        .unwrap();
        assert_eq!(view.blueprints[0]["call_graph"]["kind"], source["kind"]);
        assert_eq!(view.blueprints[0]["historical_graph_available"], true);
        assert!(view.blueprints[0]["nested_run"]["id"].is_null());
        assert_eq!(view.blueprints[0]["nested_run"]["nodes"], json!([]));
        assert_eq!(view.blueprints[0]["runs"], json!([]));
    }
}

#[test]
fn blueprint_v2_project_loads_retained_definition_history_before_projection() {
    use codeterm_service_core::{
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    let temp = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        temp.path().join("blueprints.json"),
        MockClock::starting_at(1000),
    );
    let old = document("printf old");
    let current = document("printf current");
    let mut job = store
        .prepare_document(current.clone(), owner(), true)
        .unwrap();
    let mut previous = crate::amend::DefinitionSnapshot::from_job(&job);
    previous.document = Some(old.clone());
    job.definition_version = 2;
    job.amendments.push(crate::amend::Amendment {
        version: 2,
        replaced_version: 1,
        author_tab_id: "tab".into(),
        cause_transition_id: None,
        at: 1001,
        previous_definition: Some(Box::new(previous)),
    });
    store.insert(job.clone()).unwrap();
    let (mux, _) = MultiplexerState::new();
    let service = ServiceFixture::new(mux);
    let handle = crate::scheduler::fixture_handle_with_ui(store, service.service.ui.clone());
    let journals = BTreeMap::from([(job.id.clone(), vec![row(10, "old", Some(1), 0, None)])]);
    let retained = definitions(&handle, std::slice::from_ref(&job), &journals).unwrap();
    assert_eq!(retained[&job.id][&1], old);
    assert_eq!(retained[&job.id][&2], current);
    assert_eq!(
        run_projection(&job, &journals[&job.id], retained.get(&job.id)).unwrap()
            ["graph_definition_version"],
        1
    );
}

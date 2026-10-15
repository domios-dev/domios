use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::{
    amend::Amendment,
    graph,
    machine::Route,
    store::{Owner, Sentinel, Store},
};
use codeterm_service_core::multiplexer::state::MultiplexerState;

fn setup() -> (tempfile::TempDir, Store, MultiplexerState, Owner) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(
        dir.path().join("sentinels.json"),
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.snapshot().active_workspace_id.unwrap();
    let creator = mux.create_tab(&workspace).unwrap();
    let owner = Owner {
        creator_tab_id: creator,
        team_id: None,
        workspace_id: workspace,
        cwd: "/tmp".into(),
    };
    (dir, store, mux, owner)
}

fn create(store: &mut Store, owner: &Owner, paused: bool, routes: Value) -> Sentinel {
    let mut job=store.prepare(serde_json::from_value(json!({"name":"Supplied scenario","probe":"secret-probe","question":"Declared purpose", "buckets":["done"],"every":"10s","timeout":"1h","matches":{"done":"done"},"paused":paused,"handlers":routes})).unwrap(),owner.clone(),false).unwrap();
    job.agent_authored = true;
    store.insert(job).unwrap()
}

fn snapshot(store: &Store, mux: &MultiplexerState, owner: &Owner) -> graph::Snapshot {
    graph::project(
        &owner.workspace_id,
        owner.team_id.as_deref(),
        store.list(),
        mux,
        &BTreeMap::new(),
        Value::Null,
        store.clock.now_ms(),
        &store.live_leases(),
    )
    .unwrap()
}

#[test]
fn sentinel_graph_named_scenario_has_one_root_and_reachable_internal_stage() {
    let (_dir, mut store, mux, owner) = setup();
    let child = create(&mut store, &owner, true, json!({}));
    let root = create(
        &mut store,
        &owner,
        false,
        json!({"done":[{"id":"branch","type":"blueprint","parameters":{"target":child.id}}]}),
    );
    let graph = snapshot(&store, &mux, &owner);
    assert_eq!(graph.scenarios.len(), 1);
    assert_eq!(graph.scenarios[0].id, root.id);
    assert_eq!(graph.scenarios[0].root, graph::identity(&root));
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(
        store.get(&child.id).unwrap().scenario_id.as_deref(),
        Some(root.id.as_str())
    );
    assert_eq!(
        graph.edges[0]["evidence"],
        json!({"kind":"configured","route_id":"branch"})
    );
    let encoded = serde_json::to_string(&graph).unwrap();
    assert!(!encoded.contains("session_binding_nonce"));
    assert_eq!(
        graph
            .nodes
            .iter()
            .find(|n| n["identity"]["id"] == root.id)
            .unwrap()["blueprint_definition"],
        json!({"source":"stored_agent_definition","definition_id":root.id,"creator_tab_id":owner.creator_tab_id,"definition_version":1,"lineage":[]})
    );
}

#[test]
fn sentinel_update_snapshot_exposes_read_only_definition_version_and_lineage() {
    let (_dir, mut store, mux, owner) = setup();
    let root = create(&mut store, &owner, false, json!({}));
    let mut jobs = store.list().to_vec();
    jobs[0].definition_version = 2;
    jobs[0].amendments = vec![Amendment {
        version: 2,
        replaced_version: 1,
        author_tab_id: "editor".into(),
        cause_transition_id: Some("transition_1".into()),
        at: 2000,
        previous_definition: None,
    }];

    let graph = graph::project(
        &owner.workspace_id,
        None,
        &jobs,
        &mux,
        &BTreeMap::new(),
        Value::Null,
        1000,
        &Default::default(),
    )
    .unwrap();

    let definition = graph
        .nodes
        .iter()
        .find(|node| node["identity"]["id"] == root.id)
        .unwrap()["blueprint_definition"]
        .clone();
    assert_eq!(definition["definition_version"], 2);
    assert_eq!(definition["lineage"][0]["replaced_version"], 1);
    assert_eq!(definition["lineage"][0]["author_tab_id"], "editor");
    assert_eq!(
        definition["lineage"][0]["cause_transition_id"],
        "transition_1"
    );
}

#[test]
fn sentinel_graph_shell_root_is_visible_and_orphan_paused_nodes_are_not_roots() {
    let (_dir, mut store, mux, owner) = setup();
    let mut legacy=store.prepare(serde_json::from_value(json!({"name":"legacy-origin-watch","probe":"p","question":"q","buckets":["done"],"every":"10s","matches":{"done":"done"}})).unwrap(),owner.clone(),false).unwrap();
    legacy.agent_authored = false;
    let root = store.insert(legacy).unwrap();
    assert!(root.is_scenario_root());
    create(&mut store, &owner, true, json!({}));
    let graph = snapshot(&store, &mux, &owner);
    assert_eq!(graph.scenarios.len(), 1);
    assert_eq!(graph.scenarios[0].id, root.id);
    assert_eq!(graph.nodes.len(), 1);
    assert!(graph.edges.is_empty());
}

#[test]
fn sentinel_graph_shared_targets_keep_two_entries_and_one_execution_identity() {
    let (_dir, mut store, mux, owner) = setup();
    let child = create(&mut store, &owner, true, json!({}));
    let first = create(
        &mut store,
        &owner,
        false,
        json!({"done":[{"id":"branch","type":"blueprint","parameters":{"target":child.id}}]}),
    );
    let second = create(
        &mut store,
        &owner,
        false,
        json!({"done":[{"id":"branch","type":"blueprint","parameters":{"target":child.id}}]}),
    );
    let graph = snapshot(&store, &mux, &owner);
    assert_eq!(graph.scenarios.len(), 2);
    assert_eq!(graph.nodes.len(), 3);
    assert_eq!(graph.edges.len(), 2);
    assert_ne!(first.id, second.id);
    assert_eq!(
        store.get(&child.id).unwrap().scenario_id.as_deref(),
        Some(first.id.as_str())
    );
    assert_eq!(
        graph
            .nodes
            .iter()
            .filter(|n| n["identity"]["id"] == child.id)
            .count(),
        1
    );
}

#[test]
fn sentinel_graph_missing_exact_target_has_no_foreign_metadata() {
    let (_dir, mut store, mux, owner) = setup();
    let root = create(&mut store, &owner, false, json!({}));
    let mut jobs = store.list().to_vec();
    jobs[0].routes.get_mut("done").unwrap()[0].action = Route::Send("missing-tab".into());
    let graph = graph::project(
        &owner.workspace_id,
        None,
        &jobs,
        &mux,
        &BTreeMap::new(),
        Value::Null,
        store.clock.now_ms(),
        &store.live_leases(),
    )
    .unwrap();
    assert_eq!(graph.nodes.len(), 2);
    let missing = graph
        .nodes
        .iter()
        .find(|n| n["identity"]["id"] == "missing-tab")
        .unwrap();
    assert_eq!(missing["availability"], "missing");
    assert_eq!(missing["hidden"], Value::Null);
    assert_eq!(missing["executor"], Value::Null);
    assert_eq!(graph.scenarios[0].id, root.id);
    jobs[0].routes.get_mut("done").unwrap()[0].action = Route::Send(owner.creator_tab_id.clone());
    let graph = graph::project(
        &owner.workspace_id,
        None,
        &jobs,
        &mux,
        &BTreeMap::new(),
        Value::Null,
        store.clock.now_ms(),
        &store.live_leases(),
    )
    .unwrap();
    assert_eq!(
        graph
            .nodes
            .iter()
            .find(|n| n["identity"]["id"] == owner.creator_tab_id)
            .unwrap()["availability"],
        "available"
    );
    jobs[0].routes.get_mut("done").unwrap()[0].target = Some(graph::Identity {
        kind: "worker".into(),
        id: owner.creator_tab_id.clone(),
        workspace_id: owner.workspace_id.clone(),
        team_id: None,
        owner_tab_id: Some("foreign-owner".into()),
    });
    let graph = graph::project(
        &owner.workspace_id,
        None,
        &jobs,
        &mux,
        &BTreeMap::new(),
        Value::Null,
        store.clock.now_ms(),
        &store.live_leases(),
    )
    .unwrap();
    let missing = graph
        .nodes
        .iter()
        .find(|node| node["identity"]["id"] == owner.creator_tab_id)
        .unwrap();
    assert_eq!(missing["availability"], "missing");
    assert_eq!(missing["name"], "Missing worker");
    assert_eq!(missing["executor"], Value::Null);
}

#[test]
fn sentinel_graph_scope_and_host_stop_share_actual_store_authority() {
    let (_dir, mut store, mux, owner) = setup();
    let root = create(&mut store, &owner, false, json!({}));
    assert!(graph::validate_scope(&mux, "missing", None).is_err());
    assert!(graph::validate_scope(&mux, &owner.workspace_id, Some("missing-team")).is_err());
    assert!(
        crate::service_adapter::graph::host_stop(&mut store, &mux, "foreign", None, &root.id)
            .is_err()
    );
    let stopped = crate::service_adapter::graph::host_stop(
        &mut store,
        &mux,
        &owner.workspace_id,
        None,
        &root.id,
    )
    .unwrap();
    assert!(stopped.machine.stopped);
    assert_eq!(stopped.last_reason.as_deref(), Some("stopped_by_host"));
    let graph = snapshot(&store, &mux, &owner);
    assert_eq!(graph.nodes[0]["stop"]["allowed"], false);
    assert_eq!(graph.nodes[0]["stop"]["reason"], "stopped_by_host");
}

#[test]
fn sentinel_graph_committed_changes_are_scoped_and_failed_writes_emit_nothing() {
    let (dir, mut store, _mux, owner) = setup();
    let root = create(&mut store, &owner, false, json!({}));
    let created = store.take_changes();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["workspace_id"], owner.workspace_id);
    assert_eq!(created[0]["team_id"], Value::Null);
    assert!(created[0]["revision"].is_string());
    assert!(store.take_changes().is_empty());
    store.stop(&root.id, "explicit").unwrap();
    assert_eq!(store.take_changes().len(), 1);
    let path = dir.path().join("sentinels.json");
    std::fs::rename(&path, dir.path().join("saved.json")).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(store.stop(&root.id, "failed-write").is_err());
    assert!(store.take_changes().is_empty());
}

#[test]
fn sentinel_graph_explicit_null_team_adapter_refuses_ambiguous_scope() {
    use crate::service_adapter::graph::explicit_team;
    assert_eq!(explicit_team(&Value::Null).unwrap(), None);
    assert_eq!(
        explicit_team(&json!("supplied-team")).unwrap(),
        Some("supplied-team".into())
    );
    for value in [json!(""), json!(false), json!({}), json!([]), json!(3)] {
        assert!(explicit_team(&value).is_err());
    }
}

#[test]
fn sentinel_graph_actual_cron_history_joins_exact_hidden_worker_and_missing_tab() {
    let (dir, mut store, mut mux, owner) = setup();
    let mut crons =
        codeterm_service_core::cron::store::CronStore::new_with_path(dir.path().join("crons.json"));
    let cron=crons.create(serde_json::from_value(json!({"name":"nightly-run-watch","workspace_id":owner.workspace_id,"schedule":"0 * * * *","task":"RAW_PRIVATE_TASK","creator_tab_id":owner.creator_tab_id})).unwrap()).unwrap();
    let worker = mux.create_tab(&owner.workspace_id).unwrap();
    let tab = mux.get_tab_mut(&worker).unwrap();
    tab.hidden = true;
    tab.agent_type = Some("supplied-provider".into());
    tab.spawned_by_tab_id = Some(owner.creator_tab_id.clone());
    tab.title = "CODETERM_API_TOKEN=private-token".into();
    tab.agent_session_model = Some("reported-model".into());
    crons
        .record_run(
            &cron.id,
            codeterm_service_core::cron::store::CronRunInfo {
                run_id: "exact-run".into(),
                started_at: 1234,
                finished_at: None,
                status: codeterm_service_core::cron::store::CronRunStatus::Running,
                tab_id: Some(worker.clone()),
                session_id: Some("SECRET_SESSION".into()),
                cwd: Some("PRIVATE_CWD".into()),
                summary: Some("PRIVATE_RAW_SUMMARY".into()),
                report_delivery: None,
            },
        )
        .unwrap();
    let root = create(
        &mut store,
        &owner,
        false,
        json!({"done":[{"id":"trigger","type":"cron","parameters":{"target":cron.id}}]}),
    );
    let connected = crate::service_adapter::graph::connected_crons(
        &crons,
        store.list(),
        &owner.workspace_id,
        None,
    );
    let snapshot = graph::project(
        &owner.workspace_id,
        None,
        store.list(),
        &mux,
        &connected,
        Value::Null,
        store.clock.now_ms(),
        &store.live_leases(),
    )
    .unwrap();
    assert_eq!(snapshot.scenarios[0].id, root.id);
    assert_eq!(snapshot.nodes.len(), 3);
    let node = snapshot
        .nodes
        .iter()
        .find(|n| n["identity"]["id"] == worker)
        .unwrap();
    assert_eq!(node["hidden"], true);
    assert_eq!(node["executor"]["reported_model"], "reported-model");
    assert_eq!(node["state"], "running");
    let observed = snapshot
        .edges
        .iter()
        .find(|e| e["evidence"]["kind"] == "observed")
        .unwrap();
    assert_eq!(
        observed["evidence"],
        json!({"kind":"observed","execution_id":"exact-run"})
    );
    assert_eq!(observed["result"]["accepted_at"], 1234);
    let raw = serde_json::to_string(&snapshot).unwrap();
    for private in [
        "private-token",
        "RAW_PRIVATE_TASK",
        "SECRET_SESSION",
        "PRIVATE_CWD",
        "PRIVATE_RAW_SUMMARY",
    ] {
        assert!(!raw.contains(private));
    }
    mux.rollback_spawn_tab(&owner.workspace_id, &worker);
    let missing = graph::project(
        &owner.workspace_id,
        None,
        store.list(),
        &mux,
        &connected,
        Value::Null,
        store.clock.now_ms(),
        &store.live_leases(),
    )
    .unwrap();
    let node = missing
        .nodes
        .iter()
        .find(|n| n["identity"]["id"] == worker)
        .unwrap();
    assert_eq!(node["availability"], "missing");
    assert_eq!(node["identity"], observed["target"]);
    let mut historical_jobs = store.list().to_vec();
    historical_jobs[0].routes.get_mut("done").unwrap()[0].target =
        Some(connected[&cron.id].identity.clone());
    crons.delete_preserving_history(&cron.id).unwrap();
    let retained = crate::service_adapter::graph::connected_crons(
        &crons,
        &historical_jobs,
        &owner.workspace_id,
        None,
    );
    let retained_graph = graph::project(
        &owner.workspace_id,
        None,
        &historical_jobs,
        &mux,
        &retained,
        Value::Null,
        store.clock.now_ms(),
        &store.live_leases(),
    )
    .unwrap();
    assert_eq!(
        retained_graph
            .edges
            .iter()
            .filter(|edge| edge["evidence"]["kind"] == "observed")
            .count(),
        1
    );
    assert_eq!(
        retained_graph
            .nodes
            .iter()
            .find(|node| node["identity"]["id"] == cron.id)
            .unwrap()["availability"],
        "missing"
    );
    let mut no_tab = connected.clone();
    no_tab.get_mut(&cron.id).unwrap().runs[0].tab_id = None;
    let graph = graph::project(
        &owner.workspace_id,
        None,
        store.list(),
        &mux,
        &no_tab,
        Value::Null,
        store.clock.now_ms(),
        &store.live_leases(),
    )
    .unwrap();
    assert_eq!(graph.nodes.len(), 2);
    assert_eq!(graph.edges.len(), 1);
}

#[test]
fn sentinel_graph_unavailable_store_refuses_projection_and_host_stop_without_replacing_bytes() {
    for raw in [
        "{broken",
        r#"[{"id":"old","routes":{"done":{"kind":"notify"}}}]"#,
    ] {
        let (dir, _store, mux, owner) = setup();
        let path = dir.path().join("unavailable.json");
        std::fs::write(&path, raw).unwrap();
        let mut store = Store::new(
            path.clone(),
            codeterm_service_core::cron::clock::MockClock::starting_at(1000),
        );
        assert!(
            graph::scoped_inventory(&store, &mux, &owner.workspace_id, None)
                .unwrap_err()
                .starts_with("blueprint_store_")
        );
        assert!(crate::service_adapter::graph::host_stop(
            &mut store,
            &mux,
            &owner.workspace_id,
            None,
            "old"
        )
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), raw.as_bytes());
    }
}

#[test]
fn sentinel_graph_connected_send_resolution_uses_actual_mux_scope_and_agent() {
    let (_dir, mut store, mut mux, owner) = setup();
    let root = create(&mut store, &owner, false, json!({}));
    let target = mux.create_tab(&owner.workspace_id).unwrap();
    assert_eq!(
        super::routes::resolve_send(&mux, &root, &target).unwrap(),
        Route::Send(target.clone())
    );
    mux.get_tab_mut(&target).unwrap().agent_type = Some("supplied-agent".into());
    assert_eq!(
        super::routes::resolve_send(&mux, &root, &target).unwrap(),
        Route::Send(target.clone())
    );
    mux.get_tab_mut(&target).unwrap().team_id = Some("foreign-team".into());
    assert!(super::routes::resolve_send(&mux, &root, &target).is_err());
    assert!(super::routes::resolve_send(&mux, &root, "missing").is_err());
    mux.get_tab_mut(&target).unwrap().team_id = None;
    let foreign = Sentinel {
        workspace_id: "foreign-workspace".into(),
        ..root
    };
    assert!(super::routes::resolve_send(&mux, &foreign, &target).is_err());
}

#[test]
fn sentinel_graph_cycles_terminate_and_disconnected_participants_stay_excluded() {
    let (_dir, mut store, mux, owner) = setup();
    let child = create(&mut store, &owner, true, json!({}));
    let root = create(
        &mut store,
        &owner,
        false,
        json!({"done":[{"id":"branch","type":"blueprint","parameters":{"target":child.id}}]}),
    );
    let orphan = create(&mut store, &owner, true, json!({}));
    let mut jobs = store.list().to_vec();
    jobs.iter_mut()
        .find(|job| job.id == child.id)
        .unwrap()
        .routes
        .get_mut("done")
        .unwrap()[0]
        .action = Route::Sentinel(root.id.clone());
    let ids = graph::reachable_ids(&jobs);
    assert_eq!(ids.len(), 2);
    assert!(!ids.contains(&orphan.id));
    let projected = graph::project(
        &owner.workspace_id,
        None,
        &jobs,
        &mux,
        &BTreeMap::new(),
        Value::Null,
        store.clock.now_ms(),
        &store.live_leases(),
    )
    .unwrap();
    assert_eq!(projected.scenarios.len(), 1);
    assert_eq!(projected.nodes.len(), 2);
    assert_eq!(projected.edges.len(), 2);
}

#[test]
fn sentinel_graph_host_stop_refuses_foreign_stored_scope_and_stale_current_team_relationship() {
    let (_dir, mut store, mut mux, owner) = setup();
    let other = mux
        .create_workspace("Foreign existing workspace".into(), None)
        .id;
    let root = create(&mut store, &owner, false, json!({}));
    assert!(
        crate::service_adapter::graph::host_stop(&mut store, &mux, &other, None, &root.id).is_err()
    );
    let team = codeterm_types::team::Team::new(owner.workspace_id.clone(), 1000);
    mux.upsert_team(team.clone());
    mux.get_tab_mut(&owner.creator_tab_id).unwrap().team_id = Some(team.id.clone());
    assert!(crate::service_adapter::graph::host_stop(
        &mut store,
        &mux,
        &owner.workspace_id,
        Some(&team.id),
        &root.id
    )
    .is_err());
    let scoped = Owner {
        team_id: Some(team.id.clone()),
        ..owner.clone()
    };
    let team_root = create(&mut store, &scoped, false, json!({}));
    assert!(graph::validate_scope(&mux, &scoped.workspace_id, Some(&team.id)).is_ok());
    mux.move_tab(&owner.creator_tab_id, &owner.workspace_id, &other)
        .unwrap();
    assert!(crate::service_adapter::graph::host_stop(
        &mut store,
        &mux,
        &owner.workspace_id,
        Some(&team.id),
        &team_root.id
    )
    .is_err());
    assert!(crate::service_adapter::graph::host_stop(
        &mut store,
        &mux,
        &other,
        Some(&team.id),
        &team_root.id
    )
    .is_err());
    assert!(!store.get(&team_root.id).unwrap().machine.stopped);
}

#[test]
fn sentinel_graph_sharing_an_independent_scenario_does_not_absorb_its_root() {
    let (_dir, mut store, mux, owner) = setup();
    let independent = create(&mut store, &owner, false, json!({}));
    let parent = create(
        &mut store,
        &owner,
        false,
        json!({"done":[{"id":"share","type":"blueprint","parameters":{"target":independent.id}}]}),
    );
    let graph = snapshot(&store, &mux, &owner);
    assert_eq!(graph.scenarios.len(), 2);
    assert_eq!(graph.nodes.len(), 2);
    assert_ne!(parent.id, independent.id);
    assert_eq!(
        store.get(&independent.id).unwrap().scenario_id.as_deref(),
        Some(independent.id.as_str())
    );
}

#[test]
fn sentinel_graph_connected_worker_status_projects_actual_tracker_without_default_idle_guess() {
    let (_dir, mut store, mut mux, owner) = setup();
    let worker = mux.create_tab(&owner.workspace_id).unwrap();
    let tab = mux.get_tab_mut(&worker).unwrap();
    tab.agent_type = Some("supplied-agent".into());
    tab.pty_id = Some("supplied-pty-id".into());
    create(
        &mut store,
        &owner,
        false,
        json!({"done":[{"id":"send","type":"send","parameters":{"target":worker}}]}),
    );
    let mut graph = snapshot(&store, &mux, &owner);
    let mut tracker = codeterm_service_core::agent::status::TabStatusTracker::new();
    graph::project_worker_states(&mut graph, &mux, |tab| {
        tab.pty_id
            .as_deref()
            .and_then(|id| tracker.get_status_if_tracked(id))
    });
    assert_eq!(
        graph
            .nodes
            .iter()
            .find(|node| node["identity"]["id"] == worker)
            .unwrap()["state"],
        Value::Null
    );
    tracker.set_working("supplied-pty-id");
    graph::project_worker_states(&mut graph, &mux, |tab| {
        tab.pty_id
            .as_deref()
            .and_then(|id| tracker.get_status_if_tracked(id))
    });
    let node = graph
        .nodes
        .iter()
        .find(|node| node["identity"]["id"] == worker)
        .unwrap();
    assert_eq!(node["state"], "working");
    assert_eq!(node["state_source"], "tab_status");
}

#[test]
fn sentinel_graph_host_stop_fences_until_terminal_batch_while_actual_lease_is_live() {
    let (_dir, mut store, mux, owner) = setup();
    let mut job=store.prepare(serde_json::from_value(json!({"name":"host-stop-watch","probe":"status","question":"state?","buckets":["done"],"matches":{"done":"done"},"timeout":"1h","until":"done"})).unwrap(),owner.clone(),false).unwrap();
    job.agent_authored = true;
    store.insert(job.clone()).unwrap();
    let lease = store.due().unwrap().remove(0);
    let scope = super::provenance::Scope::from(&lease.job);
    let outcome = super::machine::evaluate(
        &mut job.machine,
        b"done",
        &job.buckets,
        &job.routes,
        job.until.as_deref(),
        scope,
        || {
            super::provenance::Classification::regex(super::machine::Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            })
        },
    );
    store
        .accept_transition(&job, "done", &outcome.evidence.evaluation_id)
        .unwrap();
    store
        .claim_handler(
            &job,
            &outcome.evidence.evaluation_id,
            &job.routes["done"][0],
        )
        .unwrap()
        .unwrap();
    let graph = snapshot(&store, &mux, &owner);
    assert_eq!(graph.nodes[0]["state"], "finishing");
    assert_eq!(graph.nodes[0]["stop"]["allowed"], true);
    crate::service_adapter::graph::host_stop(&mut store, &mux, &owner.workspace_id, None, &job.id)
        .unwrap();
    assert!(lease.cancelled.load(std::sync::atomic::Ordering::SeqCst));
    assert!(!store.action_permitted(&job, &outcome.evidence.evaluation_id));
    assert_eq!(
        snapshot(&store, &mux, &owner).nodes[0]["stop"]["allowed"],
        false
    );
}

#[test]
fn sentinel_graph_terminal_lease_release_invalidates_with_committed_scope_revision() {
    let (_dir, mut store, mux, owner) = setup();
    let job = create(&mut store, &owner, false, json!({}));
    let lease = store.due().unwrap().remove(0);
    let mut terminal = lease.job.clone();
    terminal.machine.stopped = true;
    terminal.last_reason = Some("until".into());
    store.finish(&terminal).unwrap();
    assert_eq!(
        snapshot(&store, &mux, &owner).nodes[0]["state"],
        "finishing"
    );
    let revision = store
        .committed_revision(&owner.workspace_id, None)
        .unwrap()
        .to_owned();
    store.take_changes();
    store.release(&job.id);
    let changes = store.take_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0],
        json!({"workspace_id":owner.workspace_id,"team_id":null,"revision":revision})
    );
    assert_eq!(snapshot(&store, &mux, &owner).nodes[0]["state"], "stopped");
    assert_eq!(
        snapshot(&store, &mux, &owner).nodes[0]["stop"]["allowed"],
        false
    );
}

#[test]
fn blueprint_snapshot_keeps_fallback_separate_from_execution() {
    let (_dir, mut store, mux, owner) = setup();
    create(&mut store, &owner, false, json!({}));
    let fallback = json!({"adapter_id":"configured-adapter","model":"configured-model"});
    let graph = graph::project(
        &owner.workspace_id,
        None,
        store.list(),
        &mux,
        &BTreeMap::new(),
        fallback.clone(),
        1000,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(graph.configured_fallback, fallback);
    assert!(graph.nodes[0]["executor"].is_null());
    assert_eq!(graph.nodes[0]["detail"]["version"], 1);
    assert_eq!(graph.nodes[0]["detail"]["decisions"]["adapter_used"], false);
}

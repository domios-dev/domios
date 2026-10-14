//! One coherent committed graph scope, bounded incremental writes and the registered
//! schema relationships after an in-place upgrade from the foundation schema.

use std::collections::BTreeSet;

use super::*;
use crate::cron::store::{CronRunInfo, CronRunStatus, CronStore};
use codeterm_pty::multiplexer::state::MultiplexerState;
use crate::graph;
use crate::service::sentinel::graph::connected_crons;
use codeterm_storage::{ScheduleUpdate, SentinelId};

fn read_only(root: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open_with_flags(
        Storage::database_path(root),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("read-only database")
}

fn user_version(root: &Path) -> i64 {
    read_only(root)
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version")
}

#[test]
fn storage_sentinel_graph_projects_one_committed_scope_with_configured_and_observed_edges() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.snapshot().active_workspace_id.unwrap();
    let creator = mux.create_tab(&workspace).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&worker).unwrap();
    tab.hidden = true;
    tab.agent_type = Some("supplied-provider".into());
    tab.spawned_by_tab_id = Some(creator.clone());
    let mut crons = CronStore::new_with_path(root.path().join("crons.json"));
    let cron = crons
        .create(
            serde_json::from_value(json!({"name":"Supplied cron","workspace_id":workspace,
                "schedule":"0 * * * *","task":"supplied task","creator_tab_id":creator}))
            .unwrap(),
        )
        .unwrap();
    let run = CronRunInfo {
        run_id: "exact-run".into(),
        started_at: 1234,
        finished_at: None,
        status: CronRunStatus::Running,
        tab_id: Some(worker),
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    };
    crons.record_run(&cron.id, run).unwrap();
    let scope = Owner {
        creator_tab_id: creator,
        team_id: None,
        workspace_id: workspace.clone(),
        cwd: "/tmp".into(),
    };
    let authored = |paused: bool, handlers: Value| -> Sentinel {
        let mut job = store
            .lock()
            .prepare(params(paused, handlers), scope.clone(), false)
            .expect("admitted definition");
        job.agent_authored = true;
        commit(&store, |current| current.stage_insert(job)).expect("committed creation")
    };
    let child = authored(true, json!({}));
    let scenario = authored(
        false,
        json!({"done":[
            {"id":"branch","type":"blueprint","parameters":{"target":child.id}},
            {"id":"tell","type":"send","parameters":{"target":"absent-worker"}},
            {"id":"trigger","type":"cron","parameters":{"target":cron.id}}
        ]}),
    );
    create(&store, Some("team"), false, json!({}));
    assert_eq!(
        store.lock().get(&child.id).unwrap().scenario_id.as_deref(),
        Some(scenario.id.as_str()),
        "the committed association is published with the new root"
    );

    let (jobs, leases) =
        super::super::committed_scope(&store, &workspace, None).expect("committed scope");
    let project = |jobs: &[Sentinel]| {
        let connected = connected_crons(&crons, jobs, &workspace, None);
        graph::project(
            &workspace,
            None,
            jobs,
            &mux,
            &connected,
            Value::Null,
            1000,
            &leases,
        )
        .expect("projection")
    };
    let committed = project(&jobs);
    let cached: Vec<Sentinel> = store
        .lock()
        .list()
        .iter()
        .filter(|job| job.workspace_id == workspace && job.team_id.is_none())
        .cloned()
        .collect();
    assert_eq!(
        serde_json::to_value(&committed).unwrap(),
        serde_json::to_value(project(&cached)).unwrap()
    );
    assert_eq!(committed.scenarios.len(), 1);
    assert_eq!(committed.scenarios[0].id, scenario.id);
    let edge = |handler: &str| {
        committed
            .edges
            .iter()
            .find(|edge| edge["handler_id"] == handler)
            .cloned()
            .expect("projected edge")
    };
    assert_eq!(edge("branch")["evidence"]["kind"], "configured");
    assert_eq!(edge("branch")["target"]["id"], json!(child.id));
    assert_eq!(edge("trigger")["evidence"]["kind"], "configured");
    assert_eq!(edge("exact-run")["evidence"]["kind"], "observed");
    assert_eq!(edge("exact-run")["evidence"]["execution_id"], "exact-run");
    assert_eq!(edge("tell")["target"]["id"], "absent-worker");
    let missing = committed
        .nodes
        .iter()
        .find(|node| node["identity"]["id"] == "absent-worker")
        .expect("unknown target keeps its exact identity");
    assert_eq!(missing["availability"], "missing");
    assert_eq!(
        Some(committed.revision.as_str()),
        store.lock().committed_revision(&workspace, None)
    );
    close(storage);
}

#[test]
fn storage_sentinel_rows_written_stay_bounded_by_the_changed_stage() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let repo = storage.sentinels();
    let measure = |id: &str| {
        let id = SentinelId::new(id).expect("sentinel id");
        let advance = ScheduleUpdate::Advance {
            id: id.clone(),
            next_run: 5000,
        };
        let advanced = done(repo.schedule(vec![advance])).receipt.rows_written;
        let stopped = done(repo.stop(id, "measured".into())).receipt.rows_written;
        (advanced, stopped)
    };
    let first = create(&store, None, true, json!({}));
    let small = measure(&first.id);

    let bulk = (0..120)
        .map(|_| {
            let job = store
                .lock()
                .prepare(params(true, json!({})), owner(None), false)
                .expect("admitted definition");
            codec::import(&job).expect("import record")
        })
        .collect();
    done(repo.import(bulk));
    let last = create(&store, None, true, json!({}));
    let large = measure(&last.id);
    assert_eq!(small, large, "changed rows never scale with the inventory");
    assert!(small.0 <= 2 && small.1 <= 2, "{small:?}");
    assert_eq!(done(repo.inventory()).value.len(), 122);
    close(storage);
}

#[test]
fn storage_sentinel_schema_upgrades_v1_in_place_with_exact_relationships() {
    let root = temp_root();
    Storage::create_at_schema_version(root.path(), 1).expect("foundation schema root");
    rusqlite::Connection::open(Storage::database_path(root.path()))
        .and_then(|conn| {
            conn.execute(
                "INSERT INTO workspaces (id, ordinal, name, cwd, provisioned_cwd, protected, \
                 task_key_prefix, task_key_next, revision) \
                 VALUES ('kept', 0, 'Kept', NULL, NULL, 0, NULL, 0, 1)",
                [],
            )
        })
        .expect("foundation row");

    let storage = open(root.path());
    let kept = done(storage.workspaces().list()).value;
    assert_eq!(kept.workspaces.len(), 1);
    assert_eq!(kept.workspaces[0].record.name, "Kept");
    let store = typed(&storage, &MockClock::starting_at(1000));
    create(&store, Some("team"), false, json!({}));
    close(storage);

    let fresh = temp_root();
    close(open(fresh.path()));
    assert_eq!(user_version(root.path()), user_version(fresh.path()));
    assert!(user_version(root.path()) > 1);

    let conn = read_only(root.path());
    let parents = |table: &str| -> BTreeSet<String> {
        let mut statement = conn
            .prepare(&format!("PRAGMA foreign_key_list({table})"))
            .expect("foreign keys");
        let rows = statement
            .query_map([], |row| row.get::<_, String>(2))
            .expect("foreign key rows");
        rows.map(|row| row.expect("parent table")).collect()
    };
    let relationships: [(&str, &[&str]); 11] = [
        ("blueprints", &["blueprints"]),
        ("blueprint_definitions", &["blueprints"]),
        ("blueprint_heads", &["blueprint_definitions", "blueprints"]),
        ("blueprint_buckets", &["blueprint_definitions"]),
        ("blueprint_handlers", &["blueprint_buckets"]),
        ("blueprint_handler_targets", &["blueprint_handlers"]),
        ("blueprint_runtime", &["blueprints"]),
        (
            "blueprint_transitions",
            &["blueprint_definitions", "blueprints"],
        ),
        ("blueprint_handler_results", &["blueprint_transitions"]),
        ("blueprint_command_results", &["blueprint_handler_results"]),
        ("blueprint_command_cleanup", &["blueprint_command_results"]),
    ];
    for (table, expected) in relationships {
        let expected: BTreeSet<String> = expected.iter().map(|name| name.to_string()).collect();
        assert_eq!(parents(table), expected, "{table}");
    }
    let violations: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .expect("foreign key check");
    assert_eq!(violations, 0);
}

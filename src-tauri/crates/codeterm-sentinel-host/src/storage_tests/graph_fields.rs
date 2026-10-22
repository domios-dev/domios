//! Schema v4 graph fields: the guarded in-place upgrade, lossless round trips on both
//! backends, versioned lineage and observed-tab prompt-response admission.

use std::collections::BTreeSet;

use super::*;
use crate::amend::{Amendment, MAX_AMENDMENTS};
use crate::machine::SafeKind;
use crate::source::TabBinding;
use codeterm_storage::{SentinelId, StorageError, MAX_SENTINEL_LINEAGE};

fn native_fixture(document: Value) -> Sentinel {
    let root = temp_root();
    let store = Store::new(
        root.path().join("native.json"),
        MockClock::starting_at(1000),
    );
    let mut job = store
        .prepare_document(
            document,
            Owner {
                creator_tab_id: "creator".into(),
                team_id: Some("team".into()),
                workspace_id: "workspace".into(),
                cwd: "/projects/one".into(),
            },
            true,
        )
        .expect("native graph fixture");
    job.agent_authored = true;
    job.max_transitions = 64;
    job.max_actions = 256;
    job
}

fn fresh_fixture() -> Sentinel {
    let document = codeterm_sentinel::test_support::watch(
        json!("Fresh launch gate"),
        json!({"uses":"observe","with":{"kind":{"lit":"tab"},"id":{"lit":"agent-tab"}},"every":{"lit":"10s"},"timeout":{"lit":"1h"}}),
        json!({"with":{"question":{"lit":"Which prompt?"}},"buckets":["trust"],"escalation_timeout_ms":600000}),
        json!({"trust":[{"id":"answer","uses":"respond_prompt","with":{"target":{"lit":"onboarding"}}}],"other":[{"id":"ask","uses":"escalate","with":{"target":{"lit":"creator"}}}]}),
        Value::Null,
    );
    let mut job = native_fixture(document);
    job.id = "sentinel_fresh".into();
    job.scenario_id = Some(job.id.clone());
    job
}

fn graph_fixture() -> Sentinel {
    let document = codeterm_sentinel::test_support::watch(
        json!("Launch gate"),
        json!({"uses":"observe","with":{"kind":{"lit":"cron_run"},"id":{"lit":"nightly"}},"every":{"lit":"10s"},"timeout":{"lit":"1h"}}),
        json!({"with":{"question":{"lit":"Which launch state?"}},"buckets":["trust","working"],"escalation_timeout_ms":1800000}),
        json!({"trust":[{"id":"answer","uses":"respond_prompt","with":{"target":{"lit":"directory_trust"}}}],"working":[{"id":"permit","uses":"respond_prompt","with":{"target":{"lit":"permission"}}},{"id":"tool-permit","uses":"respond_prompt","with":{"target":{"lit":"tool_permission_special"}}}],"other":[{"id":"ask","uses":"escalate","with":{"target":{"lit":"agent:helper-tab"}}},{"id":"tell","uses":"notify"},{"id":"note","uses":"notify"}]}),
        Value::Null,
    );
    let mut job = native_fixture(document);
    job.id = "sentinel_graph".into();
    job.scenario_id = Some(job.id.clone());
    job.expires_at = Some(9_000_000);
    job.next_run = 21_000;
    job.machine.last_hash = Some("abc123".into());
    job.machine.last_bucket = Some("trust".into());
    job.machine.held = Some("eval-other".into());
    job.last_reason = Some("other".into());
    job.revision = 4;
    job.run = Some(crate::source::RunBinding {
        run_id: "run-7".into(),
        tab_id: "run-tab".into(),
        bound_at: 1400,
    });
    job.on_other[0].target = Some(codeterm_sentinel::identity::Identity {
        kind: "worker".into(),
        id: "helper-tab".into(),
        workspace_id: "workspace".into(),
        team_id: Some("team".into()),
        owner_tab_id: None,
    });
    job.transitions = vec![crate::handlers::Transition {
        causal_run: None,
        decision: None,
        id: "eval-other".into(),
        bucket: "other".into(),
        accepted_at: 1500,
        terminal: false,
        handlers: vec![crate::handlers::ResultRecord {
            handler_id: "ask".into(),
            status: Status::Unconfirmed,
            claim_id: Some("claim-1".into()),
            execution_id: None,
            accepted_at: Some(1501),
            reason: None,
            command_result: None,
        }],
    }];
    job.definition_version = 3;
    job.amendments = vec![Amendment {
        version: 3,
        replaced_version: 2,
        author_tab_id: "helper-tab".into(),
        cause_transition_id: Some("eval-other".into()),
        at: 1450,
        previous_definition: None,
    }];
    job
}

fn read_only(root: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open_with_flags(
        Storage::database_path(root),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("read-only database")
}

fn user_version(conn: &rusqlite::Connection) -> i64 {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("schema version")
}

fn columns(conn: &rusqlite::Connection, table: &str) -> BTreeSet<String> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("table info");
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .expect("column rows");
    rows.map(|row| row.expect("column name")).collect()
}

#[test]
fn sentinel_schema_v4_migration_upgrades_an_empty_previous_schema_root_in_place() {
    let root = temp_root();
    Storage::create_at_schema_version(root.path(), 4).expect("schema before Sentinel graph v4");
    rusqlite::Connection::open(Storage::database_path(root.path()))
        .and_then(|conn| {
            conn.execute(
                "INSERT INTO workspaces (id, ordinal, name, cwd, provisioned_cwd, protected, \
                 task_key_prefix, task_key_next, revision) \
                 VALUES ('kept', 0, 'Kept', NULL, NULL, 0, NULL, 0, 1)",
                [],
            )
        })
        .expect("v3 row");

    let storage = open(root.path());
    let kept = done(storage.workspaces().list()).value;
    assert_eq!(kept.workspaces[0].record.name, "Kept");
    let fresh = fresh_fixture();
    let created = done(
        storage
            .sentinels()
            .create(codec::create(&fresh, &[]).expect("create record")),
    );
    assert_eq!(created.value.outcome, MutationOutcome::Applied);
    close(storage);

    let fresh_root = temp_root();
    close(open(fresh_root.path()));
    let (upgraded, initialized) = (read_only(root.path()), read_only(fresh_root.path()));
    assert!(
        user_version(&upgraded) > 4,
        "the previous-schema root was migrated"
    );
    assert_eq!(user_version(&upgraded), user_version(&initialized));
    let expected: [(&str, &[&str]); 3] = [
        (
            "blueprint_definitions",
            &[
                "observe_kind",
                "observe_ref",
                "escalation_timeout_ms",
                "author_tab_id",
                "cause_transition_id",
                "replaced_version",
            ],
        ),
        ("blueprint_handlers", &["safe_kind", "escalate_to"]),
        (
            "blueprint_runtime",
            &["run_id", "run_tab_id", "run_bound_at", "held"],
        ),
    ];
    for (table, added) in expected {
        let present = columns(&upgraded, table);
        assert_eq!(present, columns(&initialized, table), "{table}");
        for column in added {
            assert!(present.contains(*column), "{table}.{column}");
        }
    }
    assert!(columns(&upgraded, "blueprint_buckets").contains("rearm_on_change"));
}

#[test]
fn sentinel_schema_v4_migration_refuses_a_previous_schema_root_holding_stages() {
    let root = temp_root();
    Storage::create_at_schema_version(root.path(), 4).expect("schema before Sentinel graph v4");
    rusqlite::Connection::open(Storage::database_path(root.path()))
        .and_then(|conn| {
            conn.execute(
                "INSERT INTO sentinels (id, ordinal, creator_tab_id, workspace_id, team_id, cwd, \
                 agent_authored, scenario_id, revision) \
                 VALUES ('sentinel_kept', 0, 'creator', 'workspace', NULL, '/tmp', 0, NULL, 1)",
                [],
            )
        })
        .expect("v3 stage");

    let refused = Storage::open(root.path(), StorageConfig::default())
        .err()
        .expect("the rebuild refuses stored stages");
    assert!(
        matches!(
            refused,
            StorageError::MigrationRefused {
                schema_version: 4,
                ..
            }
        ),
        "{refused:?}"
    );
    let conn = read_only(root.path());
    assert_eq!(user_version(&conn), 4, "nothing was migrated");
    let kept: i64 = conn
        .query_row("SELECT COUNT(*) FROM sentinels", [], |row| row.get(0))
        .expect("stored stages");
    assert_eq!(kept, 1, "the stored stage survives the refusal");
}

#[test]
fn sentinel_schema_v4_graph_fields_round_trip_on_both_backends() {
    let (fresh, graph) = (fresh_fixture(), graph_fixture());
    let both = values(&[fresh.clone(), graph.clone()]);
    let root = temp_root();
    let storage = open(root.path());
    let repo = storage.sentinels();
    let created = done(repo.create(codec::create(&fresh, &[]).expect("create record")));
    assert_eq!(created.value.outcome, MutationOutcome::Applied);
    let imported = done(repo.import(vec![codec::import(&graph).expect("import record")]));
    assert_eq!(imported.value.outcome, MutationOutcome::Applied);
    for job in [&fresh, &graph] {
        let decoded = codec::decode(&stored(&storage, &job.id));
        assert_eq!(values(&[decoded]), values(&[job.clone()]), "{}", job.id);
    }
    assert_eq!(
        values(typed(&storage, &MockClock::starting_at(1000)).lock().list()),
        both
    );
    close(storage);
    let storage = open(root.path());
    assert_eq!(
        values(typed(&storage, &MockClock::starting_at(1000)).lock().list()),
        both,
        "the typed load validates and decodes every field after reopen"
    );
    close(storage);

    let mut lossy = graph.clone();
    lossy.amendments.insert(
        0,
        Amendment {
            version: 2,
            replaced_version: 1,
            author_tab_id: "creator".into(),
            cause_transition_id: None,
            at: 1300,
            previous_definition: None,
        },
    );
    assert!(
        codec::import(&lossy).is_err(),
        "lineage whose definition is unavailable is refused, never dropped"
    );

    let path = root.path().join("sentinels.json");
    std::fs::write(&path, serde_json::to_string(&[&fresh, &graph]).unwrap()).unwrap();
    let clock = MockClock::starting_at(1000);
    let mut legacy = Store::new(path.clone(), clock.clone());
    legacy
        .ensure_available()
        .expect("the file store validates graph fields");
    assert_eq!(values(legacy.list()), both);
    legacy.stop(&fresh.id, "explicit").expect("saved stop");
    let mut stopped = fresh.clone();
    stopped.machine.stopped = true;
    stopped.revision += 1;
    stopped.last_reason = Some("explicit".into());
    stopped.cleanup.stopped_at = Some(1_000);
    stopped.cleanup.deletes_at = Some(1_801_000);
    assert_eq!(
        values(Store::new(path, clock).list()),
        values(&[stopped, graph]),
        "a save rewrites every graph field unchanged"
    );
}

#[test]
fn blueprint_terminal_session_definition_and_budgets_survive_typed_reload() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = MockClock::starting_at(1_000);
    let store = typed(&storage, &clock);
    let mut job = fresh_fixture();
    job.set_terminal(Some(codeterm_blueprint_types::blueprint_terminal::Config {
        pattern: Some("ready".into()),
        idle_ms: Some(200),
    }));
    job.session = true;
    job.max_transitions = 1;
    job.max_actions = 1;
    job.on_other.clear();
    job.escalation_timeout_ms = None;
    job.routes = std::collections::BTreeMap::from([("trust".into(), vec![])]);
    let document = job.document.as_mut().unwrap();
    document["steps"].as_array_mut().unwrap().truncate(2);
    document["steps"][0]["uses"] = json!("terminal");
    document["steps"][0]["with"]["pattern"] = json!({"lit":"ready"});
    document["steps"][0]["with"]["idle_ms"] = json!({"lit":200});
    document["steps"][0]["session"] = json!(true);
    document["steps"][1]["routes"] = json!({"trust":"@stop"});
    document["steps"][1]
        .as_object_mut()
        .unwrap()
        .remove("escalation_timeout_ms");
    crate::store::validate(vec![job.clone()]).expect("valid terminal session fixture");
    let inserted = commit(&store, |store| store.stage_insert(job.clone())).unwrap();
    assert_eq!(inserted.terminal().cloned(), job.terminal().cloned());
    for index in 0..3 {
        let live = store.lock().get(&job.id).unwrap().clone();
        let staged =
            store
                .lock()
                .stage_accept_transition(&live, "trust", &format!("exchange-{index}"));
        assert!(staged.settle(&store).unwrap());
    }
    crate::store::validate(vec![codec::decode(&stored(&storage, &job.id))])
        .expect("persisted terminal session remains valid beyond the ordinary budget");
    drop(store);
    close(storage);
    let storage = open(root.path());
    let store = typed(&storage, &clock);
    let live = store.lock().get(&job.id).unwrap().clone();
    assert_eq!(live.terminal().cloned(), job.terminal().cloned());
    assert!(live.session);
    assert_eq!(live.transitions.len(), 3);
    assert!(!crate::store::transition_budget_spent(&live));
    drop(store);
    close(storage);
}

#[test]
fn blueprint_insert_refuses_orphan_escalation_timeout_before_persistence() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = MockClock::starting_at(1_000);
    let typed_store = typed(&storage, &clock);
    let legacy = Arc::new(Mutex::new(Store::new(
        root.path().join("sentinels.json"),
        clock.clone(),
    )));
    let mut job = fresh_fixture();
    job.on_other.clear();
    for store in [&typed_store, &legacy] {
        let revision = storage.revision();
        let error = commit(store, |store| store.stage_insert(job.clone())).unwrap_err();
        assert_eq!(error, "invalid blueprint escalation");
        assert!(store.lock().get(&job.id).is_none());
        assert_eq!(storage.revision(), revision);
    }
    drop(typed_store);
    close(storage);
    let storage = open(root.path());
    let store = typed(&storage, &clock);
    assert!(store.lock().list().is_empty());
    drop(store);
    close(storage);
}

#[test]
fn sentinel_schema_v4_update_definition_appends_lineage_and_advances_generation_once() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let job = create(&store, None, false, json!({}));
    let id = SentinelId::new(job.id.clone()).expect("sentinel id");
    let repo = storage.sentinels();
    let base = stored(&storage, &job.id).record;
    let update = |expect: u64, at: u64| {
        let mut definition = base.definition.definition.clone();
        definition.question = format!("Revision {at}?");
        let amendment = DefinitionUpdate {
            definition,
            author_tab_id: "author".into(),
            cause_transition_id: Some(format!("cause-{at}")),
            authored_at: at,
        };
        done(repo.update_definition(id.clone(), expect, amendment))
            .value
            .outcome
    };
    let window = MAX_SENTINEL_LINEAGE as u64;
    for expect in 1..=window + 1 {
        assert_eq!(update(expect, 2000 + expect), MutationOutcome::Applied);
    }
    assert_eq!(update(1, 9999), MutationOutcome::Fenced(Fence::Version));

    let head = window + 2;
    let record = stored(&storage, &job.id).record;
    assert_eq!(record.definition.version, head);
    assert_eq!(
        record.runtime,
        StageRuntime {
            causal_run_json: None,
            generation: base.runtime.generation + head - 1,
            ..base.runtime.clone()
        },
        "each applied version advances the generation once; the fenced one does not"
    );
    let versions: Vec<u64> = record.amendments.iter().map(|a| a.version).collect();
    assert_eq!(
        versions,
        (3..=head).collect::<Vec<_>>(),
        "newest window, oldest first"
    );
    assert!(record
        .amendments
        .iter()
        .all(|a| a.replaced_version == a.version - 1 && a.author_tab_id == "author"));
    assert_eq!(
        record
            .amendments
            .last()
            .and_then(|a| a.cause_transition_id.clone()),
        Some(format!("cause-{}", 2000 + head - 1))
    );
    let history = done(repo.history(id)).value;
    assert_eq!(history.len() as u64, head);
    assert_eq!(
        history[0], base.definition,
        "the original version stays immutable"
    );
    assert_eq!(history[0].amendment, None);

    let decoded = codec::decode(&stored(&storage, &job.id));
    assert_eq!(decoded.definition_version, head);
    assert_eq!(decoded.amendments.len(), MAX_AMENDMENTS);
    crate::store::validate(vec![decoded]).expect("the decoded window is a valid stage");
    close(storage);
}

#[test]
fn sentinel_schema_v4_prompt_response_requires_observed_tab_on_both_backends() {
    let root = temp_root();
    let clock = MockClock::starting_at(1000);
    let storage = open(root.path());
    let typed_store = typed(&storage, &clock);
    let legacy_path = root.path().join("sentinels.json");
    let legacy = Arc::new(Mutex::new(Store::new(legacy_path.clone(), clock.clone())));
    let base = legacy
        .lock()
        .prepare_document(params(false, json!({})), owner(None), false)
        .expect("admitted definition");

    let mut shell_answer = base.clone();
    shell_answer.routes.insert(
        "done".into(),
        vec![Handler {
            id: "answer".into(),
            action: Route::RespondPrompt(SafeKind::Permission),
            target: None,
        }],
    );
    assert_eq!(shell_answer.pending_capability(), Some("respond_prompt"));
    for store in [&legacy, &typed_store] {
        let before = storage.revision();
        let error =
            commit(store, |current| current.stage_insert(shell_answer.clone())).unwrap_err();
        assert!(
            error.starts_with("blueprint_capability_unavailable"),
            "{error}"
        );
        assert!(store.lock().get(&shell_answer.id).is_none());
        assert_eq!(storage.revision(), before, "nothing was submitted");
    }

    let mut answering = base.clone();

    answering.set_observe(Some(TabBinding::Tab("observed-agent".into())));
    answering.routes.insert(
        "done".into(),
        vec![Handler {
            id: "answer".into(),
            action: Route::RespondPrompt(SafeKind::Onboarding),
            target: None,
        }],
    );
    assert_eq!(answering.pending_capability(), None);
    for store in [&legacy, &typed_store] {
        commit(store, |current| current.stage_insert(answering.clone()))
            .expect("an observed tab can answer a prompt");
        assert!(store.lock().get(&answering.id).is_some());
    }

    let shell_prompt = legacy.lock().prepare_document(
        params(
            false,
            json!({"done": [{"id":"answer","uses":"respond_prompt","with":codeterm_sentinel::test_support::literal_params(json!({"target": "permission"}))}]}),
        ),
        owner(None),
        false,
    );
    assert!(
        shell_prompt.is_err(),
        "shell prompt responses have no tab source"
    );
    assert_eq!(base.pending_capability(), None);
    close(storage);
}

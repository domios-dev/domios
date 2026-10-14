//! Schema v4 graph fields: the guarded in-place upgrade, lossless round trips on both
//! backends, versioned lineage and observed-tab prompt-response admission.

use std::collections::BTreeSet;

use super::*;
use crate::amend::{Amendment, MAX_AMENDMENTS};
use crate::machine::SafeKind;
use crate::source::TabBinding;
use codeterm_storage::{SentinelId, StorageError, MAX_SENTINEL_LINEAGE};

/// Every graph field at once: cron-run source with a bound run, prompt answers, `other`
/// handlers with an exact escalation target, a hold and an imported amended head.
const GRAPH: &str = r#"{"id":"sentinel_graph","team_id":"team","creator_tab_id":"creator",
 "workspace_id":"workspace","cwd":"/projects/one","probe":"","question":"Which launch state?",
 "buckets":["trust","working"],"interval_ms":10000,
 "routes":{
  "trust":[{"id":"answer","action":{"kind":"respond_prompt","target":"directory_trust"},"target":null}],
  "working":[{"id":"permit","action":{"kind":"respond_prompt","target":"permission"},"target":null},
   {"id":"tool-permit","action":{"kind":"respond_prompt","target":"tool_permission_special"},"target":null}]},
 "matches":{},"until":null,"expires_at":9000000,"next_run":21000,
 "machine":{"last_hash":"abc123","last_bucket":"trust","stopped":false,"paused":false,
  "held":"eval-other"},
 "last_reason":"other","revision":4,"agent_authored":true,"name":"Launch gate",
 "scenario_id":null,"max_transitions":64,"max_actions":256,
 "transitions":[{"id":"eval-other","bucket":"other","accepted_at":1500,"terminal":false,
  "handlers":[{"handler_id":"ask","status":"unconfirmed","claim_id":"claim-1",
   "execution_id":null,"accepted_at":1501,"reason":null}]}],
 "observe":{"kind":"cron_run","id":"nightly"},
 "run":{"run_id":"run-7","tab_id":"run-tab","bound_at":1400},
 "on_other":[
  {"id":"ask","action":{"kind":"escalate","target":{"agent":"helper-tab"}},
   "target":{"kind":"worker","id":"helper-tab","workspace_id":"workspace","team_id":"team",
    "owner_tab_id":null}},
  {"id":"tell","action":{"kind":"notify"},"target":null},
  {"id":"note","action":{"kind":"notify"},"target":null}],
 "escalation_timeout_ms":1800000,"definition_version":3,
 "amendments":[{"version":3,"replaced_version":2,"author_tab_id":"helper-tab",
  "cause_transition_id":"eval-other","at":1450}]}"#;

/// A creation-shaped graph stage: tab source, one prompt answer and one escalation.
const FRESH: &str = r#"{"id":"sentinel_fresh","team_id":"team","creator_tab_id":"creator",
 "workspace_id":"workspace","cwd":"/projects/one","probe":"","question":"Which prompt?",
 "buckets":["trust"],"interval_ms":10000,
 "routes":{"trust":[{"id":"answer","action":{"kind":"respond_prompt","target":"onboarding"},
  "target":null}]},
 "matches":{},"until":null,"expires_at":null,"next_run":1000,
 "machine":{"last_hash":null,"last_bucket":null,"stopped":false,"paused":false},
 "last_reason":null,"revision":0,"agent_authored":true,"name":null,"scenario_id":null,
 "max_transitions":64,"max_actions":256,"transitions":[],
 "observe":{"kind":"tab","id":"agent-tab"},
 "on_other":[{"id":"ask","action":{"kind":"escalate","target":"creator"},"target":null}],
 "escalation_timeout_ms":600000}"#;

fn fixture(raw: &str) -> Sentinel {
    serde_json::from_str(raw).expect("graph fixture")
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
    let fresh = fixture(FRESH);
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
    let (fresh, graph) = (fixture(FRESH), fixture(GRAPH));
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
    let mut job = fixture(FRESH);
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
    assert!(!super::super::super::store::transition_budget_spent(&live));
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
    let mut job = fixture(FRESH);
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
        .prepare(params(false, json!({})), owner(None), false)
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

    let shell_prompt = legacy.lock().prepare(
        params(
            false,
            json!({"done": [{"id": "answer", "type": "respond_prompt", "parameters": {"target": "permission"}}]}),
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

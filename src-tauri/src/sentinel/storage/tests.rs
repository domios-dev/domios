//! Typed Storage adapter contracts, driven through the real Store, handler and scheduler
//! seams with a counting fake effect boundary; no probe, command, agent or model runs.

use std::cell::Cell;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::super::handlers::{Handler, Status};
use super::super::machine::{Route, Verdict};
use super::super::probe;
use super::super::provenance::{Classification, Scope};
use super::super::store::{Create, Lease, Owner, Sentinel, Store};
use super::{codec, commit, legacy_import};
use crate::cron::clock::MockClock;
use crate::storage::{
    Completion, DefinitionProvenance, DefinitionUpdate, Fence, MutationOutcome, StageRuntime,
    Storage, StorageConfig, StoredSentinel,
};

mod coherence;
mod decision_detail;
mod graph_fields;
mod pipeline_journal;
mod races;

/// Failure bound around event waits only; elapsed time never signals readiness.
const DEADLINE: Duration = Duration::from_secs(30);

const LEGACY: &str = r#"[
 {"id":"sentinel_root","team_id":"team","creator_tab_id":"creator","workspace_id":"workspace",
  "cwd":"/projects/one","probe":"status --short","question":"Which state?",
  "buckets":["done","failed","waiting"],"interval_ms":20000,
  "routes":{
   "done":[
    {"id":"join","action":{"kind":"blueprint","target":"sentinel_child"},
     "target":{"kind":"blueprint","id":"sentinel_child","workspace_id":"workspace","team_id":"team","owner_tab_id":"creator"}},
    {"id":"build","action":{"kind":"command","target":{"command":"make check","label":"Check","timeout_ms":1000}},"target":null},
    {"id":"tell","action":{"kind":"send","target":"worker-tab"},
     "target":{"kind":"worker","id":"worker-tab","workspace_id":"workspace","team_id":"team","owner_tab_id":null}}],
   "failed":[
    {"id":"nightly","action":{"kind":"cron","target":"nightly"},
     "target":{"kind":"cron","id":"nightly","workspace_id":"workspace","team_id":"team","owner_tab_id":"creator"}},
    {"id":"halt","action":{"kind":"stop"},"target":null}],
   "waiting":[]},
  "matches":{"done":"^done$"},"until":"failed","expires_at":9000000,"next_run":21000,
  "machine":{"last_hash":"abc123","last_bucket":"done","stopped":false,"paused":false},
  "last_reason":"transition","revision":7,"agent_authored":true,"name":"Supplied scenario",
  "scenario_id":"sentinel_root","max_transitions":8,"max_actions":16,
  "transitions":[
   {"id":"eval-1","bucket":"done","accepted_at":1500,"terminal":false,"handlers":[
    {"handler_id":"join","status":"noop","claim_id":"claim-1","execution_id":"claim-1","accepted_at":1501,"reason":null},
    {"handler_id":"build","status":"failed","claim_id":"claim-2","execution_id":"exec-2","accepted_at":1502,
     "command_result":{"status":"nonzero","exit_code":2,"resources_confirmed":true,
      "stdout":{"total_bytes":10,"retained_bytes":10,"truncated":false},
      "stderr":{"total_bytes":40000,"retained_bytes":32768,"truncated":true},
      "started_at":1502,"finished_at":1503,"primary":null,"cleanup":[]},
     "reason":"action_failed"},
    {"handler_id":"tell","status":"skipped","claim_id":null,"execution_id":null,"accepted_at":null,"reason":"batch_fenced"}]},
   {"id":"eval-2","bucket":"done","accepted_at":1600,"terminal":false,"handlers":[
    {"handler_id":"join","status":"unconfirmed","claim_id":"claim-3","execution_id":null,"accepted_at":1601,"reason":null}]}]},
 {"id":"sentinel_child","team_id":"team","creator_tab_id":"creator","workspace_id":"workspace",
  "cwd":"/projects/one","probe":"status","question":"Child state?","buckets":["done"],
  "interval_ms":10000,"routes":{"done":[]},"matches":{},"until":null,"expires_at":null,
  "next_run":1000,"machine":{"last_hash":null,"last_bucket":null,"stopped":false,"paused":true},
  "last_reason":null,"revision":0,"agent_authored":true,"name":null,
  "scenario_id":"sentinel_root","max_transitions":64,"max_actions":256,"transitions":[]},
 {"id":"sentinel_solo","team_id":null,"creator_tab_id":"solo-creator","workspace_id":"workspace",
  "cwd":"/projects/two","probe":"tail log","question":"Late?","buckets":["late"],
  "interval_ms":10000,
  "routes":{"late":[{"id":"probe-cmd","action":{"kind":"command","target":{"command":"printf late","label":"Late","timeout_ms":1000}},"target":null}]},
  "matches":{"late":"late"},"until":null,"expires_at":1200,"next_run":1000,
  "machine":{"last_hash":"def456","last_bucket":"late","stopped":true,"paused":false},
  "last_reason":"timeout","revision":3,"agent_authored":false,"name":null,"scenario_id":null,
  "max_transitions":64,"max_actions":256,
  "transitions":[{"id":"eval-9","bucket":"late","accepted_at":1100,"terminal":true,"handlers":[
   {"handler_id":"probe-cmd","status":"unconfirmed","claim_id":"claim-9","execution_id":"exec-9","accepted_at":1101,
    "command_result":{"status":"unconfirmed","exit_code":null,"resources_confirmed":false,"stdout":null,"stderr":null,
     "started_at":1101,"finished_at":1102,"primary":"probe_completion_unconfirmed",
     "cleanup":["child process still running","pipe still open"]},
    "reason":"effect_unconfirmed"}]}]}
]"#;

fn temp_root() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp root")
}

fn open(root: &Path) -> Storage {
    Storage::open(root, StorageConfig::default()).expect("open storage")
}

fn close(storage: Storage) {
    storage.shutdown().wait(DEADLINE).expect("storage shutdown");
}

fn done<T>(completion: Completion<T>) -> T {
    completion.wait(DEADLINE).expect("storage operation")
}

fn typed(storage: &Storage, clock: &Arc<MockClock>) -> Arc<Mutex<Store>> {
    let store = Store::with_storage(storage, clock.clone());
    store.ensure_available().expect("typed store available");
    Arc::new(Mutex::new(store))
}

fn owner(team: Option<&str>) -> Owner {
    Owner {
        creator_tab_id: "creator".into(),
        team_id: team.map(str::to_owned),
        workspace_id: "workspace".into(),
        cwd: "/tmp".into(),
    }
}

fn params(paused: bool, handlers: Value) -> Create {
    serde_json::from_value(json!({
        "name": "storage-state-watch",
        "probe": "status", "question": "state?", "buckets": ["done", "failed"], "every": "10s",
        "timeout": "1h", "matches": {"done": "^done$"}, "paused": paused, "handlers": handlers
    }))
    .expect("definition")
}

fn create(store: &Mutex<Store>, team: Option<&str>, paused: bool, handlers: Value) -> Sentinel {
    let job = store
        .lock()
        .prepare(params(paused, handlers), owner(team), false)
        .expect("admitted definition");
    commit(store, |current| current.stage_insert(job)).expect("committed creation")
}

fn due(store: &Mutex<Store>) -> Vec<Lease> {
    commit(store, Store::stage_due).expect("committed schedule")
}

fn output(text: &str) -> probe::Output {
    probe::Output {
        bytes: text.as_bytes().to_vec(),
        text: text.into(),
        ..Default::default()
    }
}

fn verdict(bucket: &str) -> Classification {
    Classification::regex(Verdict {
        bucket: bucket.into(),
        confidence: 1.0,
    })
}

/// Drives the production scheduler advance with durable typed claims and a fake effect.
fn run(
    store: &Arc<Mutex<Store>>,
    lease: &Lease,
    text: &str,
    bucket: &str,
    mut effect: impl FnMut(&Sentinel, &str, &Handler) -> Result<Status, String>,
) {
    super::super::scheduler::advance(
        store,
        lease,
        &output(text),
        || verdict(bucket),
        |job, transition, handler| {
            commit(store, |current| {
                current.stage_claim_handler(job, transition, handler)
            })
        },
        |job, transition, handler, _| effect(job, transition, handler).map(Into::into),
        |_, _, _| Ok(()),
    );
}

/// Accepts one transition for a lease and returns the evaluated job that owns it.
fn accept(store: &Mutex<Store>, lease: &Lease) -> (Sentinel, String) {
    let mut job = lease.job.clone();
    let outcome = super::super::machine::evaluate(
        &mut job.machine,
        b"done",
        &job.buckets,
        &job.routes,
        None,
        Scope::from(&lease.job),
        || verdict("done"),
    );
    let transition = outcome.evidence.evaluation_id;
    let accepted = commit(store, |current| {
        current.stage_accept_transition(&job, "done", &transition)
    });
    assert_eq!(accepted, Ok(true));
    (job, transition)
}

fn stored(storage: &Storage, id: &str) -> StoredSentinel {
    let id = crate::storage::SentinelId::new(id).expect("sentinel id");
    done(storage.sentinels().get(id))
        .value
        .expect("stored sentinel")
}

fn values(jobs: &[Sentinel]) -> Value {
    serde_json::to_value(jobs).expect("serializable blueprints")
}

fn statuses(store: &Mutex<Store>, id: &str) -> Vec<Status> {
    let store = store.lock();
    let job = store.get(id).expect("cached sentinel");
    job.transitions
        .last()
        .map(|transition| {
            transition
                .handlers
                .iter()
                .map(|result| result.status.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn import_legacy(storage: &Storage) -> Vec<Sentinel> {
    let records = legacy_import(LEGACY).expect("legacy decode");
    let committed = done(storage.sentinels().import(records));
    assert_eq!(committed.value.outcome, MutationOutcome::Applied);
    serde_json::from_str(LEGACY).expect("fixture blueprints")
}

#[test]
fn storage_sentinel_import_roundtrip_conserves_definition_runtime_and_results() {
    let root = temp_root();
    let storage = open(root.path());
    let original = import_legacy(&storage);
    close(storage);

    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(25_000));
    assert_eq!(values(store.lock().list()), values(&original));
    for (workspace, team) in [("workspace", Some("team")), ("workspace", None)] {
        assert_eq!(
            store.lock().committed_revision(workspace, team),
            Some(super::super::graph::scope_revision(&original, workspace, team).as_str()),
        );
    }
    let history = done(
        storage
            .sentinels()
            .history(crate::storage::SentinelId::new("sentinel_root").expect("sentinel id")),
    )
    .value;
    assert_eq!(history.len(), 1, "only the actually present version exists");
    assert_eq!(history[0].version, 1);
    assert_eq!(history[0].provenance, DefinitionProvenance::Imported);
    let root_record = stored(&storage, "sentinel_root");
    assert!(root_record
        .record
        .transitions
        .iter()
        .all(|transition| transition.definition_version.is_none()));
    assert_eq!(root_record.record.runtime.generation, 7);
    close(storage);
}

#[test]
fn storage_sentinel_reopen_never_replays_claims_or_rearms_stopped_work() {
    let root = temp_root();
    let storage = open(root.path());
    import_legacy(&storage);
    let clock = MockClock::starting_at(25_000);
    let store = typed(&storage, &clock);
    let (claims, effects) = (Cell::new(0), Cell::new(0));

    let leases = due(&store);
    assert_eq!(
        leases
            .iter()
            .map(|lease| lease.job.id.as_str())
            .collect::<Vec<_>>(),
        ["sentinel_root"],
        "paused and stopped stages are never leased"
    );
    super::super::scheduler::advance(
        &store,
        &leases[0],
        &output("done"),
        || verdict("done"),
        |_, _, _| {
            claims.set(claims.get() + 1);
            Ok(None)
        },
        |_, _, _, _| {
            effects.set(effects.get() + 1);
            Ok(Status::Completed.into())
        },
        |_, _, _| Ok(()),
    );
    store.lock().release("sentinel_root");
    let current = store.lock().get("sentinel_root").cloned().expect("root");
    let join = current.routes["done"][0].clone();
    let replay = commit(&store, |store| {
        store.stage_claim_handler(&current, "eval-2", &join)
    });
    assert!(
        replay.expect("refusal").is_none(),
        "unconfirmed claim is never replayed"
    );

    clock.set_ms(9_000_000);
    assert!(due(&store).is_empty());
    close(storage);

    let storage = open(root.path());
    let store = typed(&storage, &clock);
    let restored = store.lock().list().to_vec();
    let root_job = &restored[0];
    assert!(root_job.machine.stopped);
    assert_eq!(root_job.last_reason.as_deref(), Some("timeout"));
    assert_eq!(root_job.revision, 8);
    assert_eq!(root_job.next_run, 41_000);
    assert_eq!(
        root_job.transitions[1].handlers[0].status,
        Status::Unconfirmed
    );
    assert_eq!(
        root_job.transitions[1].handlers[0].claim_id.as_deref(),
        Some("claim-3")
    );
    assert!(restored[1].machine.paused && !restored[1].machine.stopped);
    assert_eq!(restored[2].revision, 3, "stopped work is never rearmed");
    assert!(due(&store).is_empty());
    assert_eq!((claims.get(), effects.get()), (0, 0));
    close(storage);
}

#[test]
fn storage_sentinel_definition_versions_are_independent_of_runtime_state() {
    let root = temp_root();
    let storage = open(root.path());
    let store = typed(&storage, &MockClock::starting_at(1000));
    let job = create(&store, None, false, json!({}));
    let id = crate::storage::SentinelId::new(job.id.clone()).expect("sentinel id");
    let repo = storage.sentinels();

    let first = stored(&storage, &job.id).record;
    assert_eq!(first.definition.version, 1);
    assert_eq!(first.definition.provenance, DefinitionProvenance::Authored);
    let mut revised = first.definition.definition.clone();
    revised.question = "Revised question?".into();
    let update = |definition| DefinitionUpdate {
        definition,
        author_tab_id: "creator".into(),
        cause_transition_id: None,
        authored_at: 1000,
    };
    let updated = done(repo.update_definition(id.clone(), 1, update(revised.clone())));
    assert_eq!(updated.value.outcome, MutationOutcome::Applied);
    let stale = done(repo.update_definition(id.clone(), 1, update(revised)));
    assert_eq!(stale.value.outcome, MutationOutcome::Fenced(Fence::Version));

    let after = stored(&storage, &job.id).record;
    assert_eq!(after.definition.version, 2);
    let generation = first.runtime.generation + 1;
    assert_eq!(
        after.runtime,
        StageRuntime {
            causal_run_json: None,
            generation,
            ..first.runtime.clone()
        },
        "a new version advances only the generation, exactly once"
    );
    let history = done(repo.history(id.clone())).value;
    assert_eq!(history.len(), 2);
    assert_eq!(
        history[0], first.definition,
        "older versions stay immutable"
    );

    let accepted = done(repo.accept_transition(
        id.clone(),
        generation,
        codec::outcome(&job),
        crate::storage::NewTransition {
            causal_run_json: None,
            decision_json: None,
            id: "eval-after-update".into(),
            bucket: "done".into(),
            accepted_at: 1000,
            terminal: false,
        },
    ));
    assert_eq!(accepted.value.outcome, MutationOutcome::Applied);
    let stopped = done(repo.stop(id.clone(), "explicit".into()));
    assert_eq!(stopped.value.outcome, MutationOutcome::Applied);
    let last = stored(&storage, &job.id).record;
    assert_eq!(last.transitions[0].definition_version, Some(2));
    assert_eq!(last.runtime.generation, generation + 1);
    assert_eq!(
        done(repo.history(id)).value.len(),
        2,
        "operational changes are not versions"
    );
    close(storage);
}

#[test]
fn storage_sentinel_rearm_configuration_survives_reopen_and_definition_history() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = MockClock::starting_at(1000);
    let store = typed(&storage, &clock);
    let mut definition = params(false, json!({}));
    definition.rearm_on_change.insert("done".into());
    let job = store
        .lock()
        .prepare(definition, owner(None), false)
        .expect("rearm definition");
    let id = job.id.clone();
    commit(&store, |current| current.stage_insert(job)).expect("rearm definition committed");
    close(storage);

    let storage = open(root.path());
    let restored = typed(&storage, &clock);
    assert!(restored
        .lock()
        .get(&id)
        .unwrap()
        .rearm_on_change
        .contains("done"));
    let history = done(
        storage
            .sentinels()
            .history(crate::storage::SentinelId::new(id).expect("sentinel id")),
    )
    .value;
    assert!(
        history[0]
            .definition
            .buckets
            .iter()
            .find(|bucket| bucket.name == "done")
            .expect("stored done bucket")
            .rearm_on_change
    );
    close(storage);
}

#[test]
fn storage_sentinel_two_parents_activate_child_once_and_survive_reopen() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = MockClock::starting_at(1000);
    let store = typed(&storage, &clock);
    let child = create(&store, Some("team"), true, json!({}));
    let join = json!({"done":[{"id":"join","type":"blueprint","parameters":{"target":child.id}}]});
    let first = create(&store, Some("team"), false, join.clone());
    let second = create(&store, Some("team"), false, join);
    let leases = due(&store);
    let activate =
        |job: &Sentinel, transition: &str, handler: &Handler| -> Result<Status, String> {
            let Route::Sentinel(target) = &handler.action else {
                panic!("only the join handler is declared");
            };
            let changed = commit(&store, |current| {
                current.stage_activate(job, transition, target)
            })?;
            Ok(if changed {
                Status::Completed
            } else {
                Status::Noop
            })
        };
    let one = leases
        .iter()
        .find(|lease| lease.job.id == first.id)
        .unwrap();
    run(&store, one, "first", "done", activate);
    let child_lease = due(&store).remove(0);
    assert_eq!(child_lease.job.id, child.id);
    let before = values(&[store.lock().get(&child.id).cloned().unwrap()]);
    let two = leases
        .iter()
        .find(|lease| lease.job.id == second.id)
        .unwrap();
    run(&store, two, "second", "done", activate);
    assert_eq!(
        values(&[store.lock().get(&child.id).cloned().unwrap()]),
        before
    );
    assert!(
        !child_lease.is_cancelled(),
        "the second parent never resets the child"
    );
    let mut child_effects = 0;
    run(&store, &child_lease, "child-done", "done", |_, _, _| {
        child_effects += 1;
        Ok(Status::Completed)
    });
    assert_eq!(child_effects, 1);
    close(storage);

    let storage = open(root.path());
    let store = typed(&storage, &clock);
    let restored = store.lock();
    let child = restored.get(&child.id).unwrap();
    assert_eq!(child.machine.last_bucket.as_deref(), Some("done"));
    assert_eq!(child.revision, 1, "activated exactly once");
    let first_run = restored.get(&first.id).unwrap().transitions[0]
        .causal_run
        .as_ref()
        .unwrap();
    let second_run = restored.get(&second.id).unwrap().transitions[0]
        .causal_run
        .as_ref()
        .unwrap();
    let child_run = child.transitions[0].causal_run.as_ref().unwrap();
    assert_ne!(first_run.id, second_run.id);
    assert_eq!(first_run.started_at, second_run.started_at);
    assert_eq!(child_run.id, first_run.id);
    assert_eq!(child.causal_run.as_ref(), Some(child_run));
    assert_eq!(child_run.parent.as_ref().unwrap().node, first.id);
    assert_eq!(child_run.parent.as_ref().unwrap().handler, "join");
    assert_eq!(
        child_run.parent.as_ref().unwrap().transition,
        restored.get(&first.id).unwrap().transitions[0].id
    );

    assert_eq!(
        restored.get(&second.id).unwrap().transitions[0].handlers[0].status,
        Status::Noop
    );
    assert_eq!(
        restored.get(&first.id).unwrap().transitions[0].handlers[0].status,
        Status::Completed
    );
    drop(restored);
    close(storage);
}

#[test]
fn storage_sentinel_partial_handler_results_survive_reopen_without_replay() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = MockClock::starting_at(1000);
    let store = typed(&storage, &clock);
    let parent = create(
        &store,
        Some("team"),
        false,
        json!({"done":[
            {"id":"first","type":"notify","parameters":{}},
            {"id":"second","type":"notify","parameters":{}},
            {"id":"third","type":"notify","parameters":{}}
        ]}),
    );
    let lease = due(&store).remove(0);
    let mut order = Vec::new();
    run(&store, &lease, "done", "done", |_, _, handler| {
        order.push(handler.id.clone());
        if handler.id == "second" {
            Err("ordinary refusal".into())
        } else {
            Ok(Status::Completed)
        }
    });
    assert_eq!(order, ["first", "second", "third"]);
    let expected = [Status::Completed, Status::Failed, Status::Completed];
    assert_eq!(statuses(&store, &parent.id), expected);
    let accepted = store.lock().get(&parent.id).cloned().unwrap();
    close(storage);

    let storage = open(root.path());
    let store = typed(&storage, &clock);
    assert_eq!(statuses(&store, &parent.id), expected);
    for handler in &accepted.routes["done"] {
        let replay = commit(&store, |current| {
            current.stage_claim_handler(&accepted, &accepted.transitions[0].id, handler)
        });
        assert!(replay.expect("refusal").is_none());
    }
    close(storage);
}

#[test]
fn storage_sentinel_crash_after_claim_never_reclassifies_or_replays() {
    let root = temp_root();
    let storage = open(root.path());
    let clock = MockClock::starting_at(1000);
    let store = typed(&storage, &clock);
    let job = create(
        &store,
        Some("team"),
        false,
        json!({"done":[{"id":"attempt","type":"notify","parameters":{}}]}),
    );
    let lease = due(&store).remove(0);
    super::super::scheduler::advance(
        &store,
        &lease,
        &output("done"),
        || verdict("done"),
        |job, transition, handler| {
            commit(&store, |current| {
                current.stage_claim_handler(job, transition, handler)
            })
        },
        |_, _, _, _| panic!("journal refusal prevents the effect"),
        |_, _, stage| {
            if matches!(stage, super::super::provenance::Stage::RouteClaim) {
                Err(std::io::Error::other("interrupted after durable claim"))
            } else {
                Ok(())
            }
        },
    );
    drop(store);
    close(storage);

    clock.advance_ms(10_000);
    let storage = open(root.path());
    let restored = typed(&storage, &clock);
    let lease = due(&restored).remove(0);
    super::super::scheduler::advance(
        &restored,
        &lease,
        &output("done"),
        || panic!("persisted observation must not reclassify"),
        |_, _, _| panic!("persisted claim must not replay"),
        |_, _, _, _| panic!("persisted effect must not replay"),
        |_, _, _| Ok(()),
    );
    assert_eq!(statuses(&restored, &job.id), [Status::Unconfirmed]);
    assert_eq!(restored.lock().get(&job.id).unwrap().transitions.len(), 1);
    close(storage);
}

#[test]
fn storage_sentinel_backends_stay_isolated_and_lock_held_calls_refuse() {
    let root = temp_root();
    let clock = MockClock::starting_at(1000);
    let legacy_path = root.path().join("sentinels.json");
    let legacy = Arc::new(Mutex::new(Store::new(legacy_path.clone(), clock.clone())));
    create(&legacy, None, false, json!({}));
    let legacy_bytes = std::fs::read(&legacy_path).expect("legacy file");

    let storage = open(root.path());
    assert!(done(storage.sentinels().inventory()).value.is_empty());
    let store = typed(&storage, &clock);
    let job = create(&store, None, false, json!({}));
    assert_eq!(std::fs::read(&legacy_path).unwrap(), legacy_bytes);
    assert_eq!(legacy.lock().list().len(), 1);
    assert!(legacy.lock().get(&job.id).is_none());

    let before = storage.revision();
    let refused = store.lock().stop(&job.id, "lock-held");
    assert_eq!(refused.unwrap_err(), super::GUARD_FREE_REQUIRED);
    assert_eq!(
        storage.revision(),
        before,
        "a refused lock-held call submits nothing"
    );
    assert!(!store.lock().get(&job.id).unwrap().machine.stopped);
    close(storage);
}

#[test]
fn storage_sentinel_codec_refuses_lossy_definitions() {
    let root = temp_root();
    let store = Arc::new(Mutex::new(Store::new(
        root.path().join("sentinels.json"),
        MockClock::starting_at(1000),
    )));
    let mut job = store
        .lock()
        .prepare(params(false, json!({})), owner(None), false)
        .expect("admitted definition");
    assert!(codec::import(&job).is_ok());
    job.routes.insert("undeclared".into(), Vec::new());
    assert!(codec::import(&job).is_err());
    job.routes.remove("undeclared");
    job.matches.insert("undeclared".into(), "x".into());
    assert!(codec::import(&job).is_err());
}

#[test]
fn sentinel_escalate_typed_outcomes_persist_hold_resolve_and_expiry() {
    let root = temp_root();
    let mut storage = open(root.path());
    let clock = MockClock::starting_at(1000);
    let store = typed(&storage, &clock);
    let params: Create = serde_json::from_value(json!({
        "name": "escalation-outcome-watch",
        "probe": "status",
        "question": "state?",
        "buckets": ["done", "failed"],
        "every": "10s",
        "timeout": "1h",
        "matches": {"done": "^done$"},
        "escalation_timeout_ms": 5000,
        "handlers": {
            "done": [{"id": "done_notice", "type": "notify", "parameters": {}}],
            "other": [{"id": "ask_creator", "type": "escalate", "parameters": {"target": "creator"}}]
        }
    }))
    .unwrap();
    let job = store
        .lock()
        .prepare(params, owner(Some("team")), false)
        .expect("escalation definition");
    let job = commit(&store, |current| current.stage_insert(job)).unwrap();
    let lease = due(&store).remove(0);
    run(&store, &lease, "unknown", "other", |_, _, _| {
        Ok(Status::Completed)
    });
    store.lock().release(&job.id);
    let held = store.lock().get(&job.id).unwrap().clone();
    let held_transition = held.machine.held.clone().expect("typed held state");
    let original_hash = held.machine.last_hash.clone().expect("observation hash");
    assert_eq!(
        codec::outcome(&held).held.as_deref(),
        Some(held_transition.as_str())
    );

    assert!(commit(&store, |current| {
        current.stage_resolve_escalation(&job.id, &held_transition)
    })
    .unwrap());
    let resolved = store.lock().get(&job.id).unwrap().clone();
    assert!(resolved.machine.held.is_none());
    assert!(resolved.machine.last_hash.is_none());
    assert_eq!(resolved.machine.last_bucket.as_deref(), Some("other"));
    assert!(store.lock().wake_stage(&job.id));

    let lease = due(&store).remove(0);
    run(&store, &lease, "unknown", "other", |_, _, _| {
        Ok(Status::Completed)
    });
    store.lock().release(&job.id);
    let held_again = store.lock().get(&job.id).unwrap().clone();
    let expired_transition = held_again.machine.held.clone().expect("second held state");
    let expired_hash = held_again
        .machine
        .last_hash
        .clone()
        .expect("second observation hash");
    assert_ne!(original_hash, "");

    clock.advance_ms(5000);
    let (_, staged) = store
        .lock()
        .stage_expire_escalation()
        .expect("expired hold");
    assert!(staged.settle(&store).unwrap());
    let expired = store.lock().get(&job.id).unwrap().clone();
    assert!(expired.machine.held.is_none());
    assert_eq!(
        expired.machine.last_hash.as_deref(),
        Some(expired_hash.as_str())
    );
    assert_eq!(expired.machine.last_bucket.as_deref(), Some("other"));
    assert_eq!(expired.last_reason.as_deref(), Some("escalation_expired"));
    assert!(codec::outcome(&expired).held.is_none());
    assert_eq!(
        codec::outcome(&expired).last_hash.as_deref(),
        Some(expired_hash.as_str())
    );
    assert_ne!(expired_transition, held_transition);

    drop(store);
    close(storage);
    storage = open(root.path());
    let reopened = typed(&storage, &clock);
    let restored = reopened.lock().get(&job.id).unwrap().clone();
    assert!(restored.machine.held.is_none());
    assert_eq!(
        restored.machine.last_hash.as_deref(),
        Some(expired_hash.as_str())
    );
    assert_eq!(restored.machine.last_bucket.as_deref(), Some("other"));
    assert_eq!(restored.last_reason.as_deref(), Some("escalation_expired"));
    drop(reopened);
    close(storage);
}

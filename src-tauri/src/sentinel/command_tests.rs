use super::store::{Owner, Store};
use serde_json::{json, Value};

fn owner() -> Owner {
    Owner {
        creator_tab_id: "creator".into(),
        team_id: Some("team".into()),
        workspace_id: "workspace".into(),
        cwd: "/tmp".into(),
    }
}

fn definition(kind: &str, parameters: Value) -> Value {
    json!({
        "probe":"status", "question":"state?", "buckets":["done"],
        "every":"10s", "matches":{"done":"done"}, "timeout":"1h",
        "handlers":{"done":[{"id":"action", "type":kind, "parameters":parameters}]}
    })
}

fn create(
    store: &mut Store,
    kind: &str,
    parameters: Value,
) -> Result<super::store::Sentinel, String> {
    serde_json::from_value(definition(kind, parameters))
        .map_err(|error| error.to_string())
        .and_then(|params| store.create(params, owner(), false))
}

#[test]
fn sentinel_command_existing_notify_creation_positive() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let job = create(&mut store, "notify", json!({})).unwrap();
    assert_eq!(job.routes["done"][0].id, "action");
}

#[test]
fn sentinel_command_fixed_definition_admitted_through_create() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let result = create(
        &mut store,
        "command",
        json!({"command":"printf marker", "label":"Marker", "timeout_ms":1000}),
    );
    assert!(
        result.is_ok(),
        "fixed creator command must be admitted: {result:?}"
    );
}

#[test]
fn sentinel_command_fixed_definition_persists_creator_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(path.clone(), clock.clone());
    let result = create(
        &mut store,
        "command",
        json!({"command":"printf marker", "label":"Marker", "timeout_ms":1000}),
    );
    assert!(
        result.is_ok(),
        "fixed command creation must reach durable storage: {result:?}"
    );
    let job = result.unwrap();
    let restored = Store::new(path, clock);
    assert_eq!(restored.get(&job.id).unwrap().cwd, owner().cwd);
}

#[test]
fn sentinel_command_creation_reaches_independent_persisted_execution_seam() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        dir.path().join("sentinels.json"),
        crate::cron::clock::MockClock::starting_at(1000),
    );
    let created = create(
        &mut store,
        "command",
        json!({"command":"printf marker", "label":"Marker", "timeout_ms":1000}),
    );
    assert!(
        created.is_ok(),
        "fixed command must reach the settled executor seam: {created:?}"
    );
    let job = created.unwrap();
    let lease = store.due().unwrap().remove(0);
    let store = std::sync::Arc::new(parking_lot::Mutex::new(store));
    let owner = std::sync::Arc::new(super::probe::owner::ProbeResourceOwner::new());
    let output = super::probe::run(
        "printf done",
        dir.path(),
        super::probe::PROBE_RUNTIME,
        &owner,
    )
    .unwrap();
    let mut called = 0;
    super::scheduler::advance(
        &store,
        &lease,
        &output,
        || {
            super::provenance::Classification::regex(super::machine::Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            })
        },
        |job, transition, handler| store.lock().claim_handler(job, transition, handler),
        |job, transition, handler, _| {
            assert_eq!(super::routes::action_name(&handler.action), "command");
            let current = store.lock();
            let accepted = current
                .get(&job.id)
                .unwrap()
                .transitions
                .iter()
                .find(|accepted| accepted.id == transition)
                .unwrap();
            assert!(accepted
                .handlers
                .iter()
                .any(|result| result.handler_id == handler.id && result.claim_id.is_some()));
            called += 1;
            Ok(super::handlers::Status::Completed.into())
        },
        |_, _, _| Ok(()),
    );
    assert_eq!(called, 1);
    assert_eq!(
        store.lock().get(&job.id).unwrap().transitions[0].handlers[0].status,
        super::handlers::Status::Completed
    );
}

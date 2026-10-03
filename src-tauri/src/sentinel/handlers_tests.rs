use super::machine::{self, Route};
use super::provenance::{Classification, Scope};
use super::store::{Create, Owner, Store};
use serde_json::{json, Value};

fn owner() -> Owner {
    Owner {
        creator_tab_id: "creator".into(),
        team_id: Some("team".into()),
        workspace_id: "workspace".into(),
        cwd: "/tmp".into(),
    }
}

fn definition() -> Value {
    json!({
        "probe": "status", "question": "state?", "buckets": ["done"],
        "every": "10s", "matches": {"done": "^done$"}, "timeout": "1h"
    })
}

#[test]
fn sentinel_handlers_current_create_evaluate_and_reload_positive() {
    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let path = dir.path().join("sentinels.json");
    let mut store = Store::new(path.clone(), clock.clone());
    let mut job = store.create(serde_json::from_value(definition()).unwrap(), owner(), false).unwrap();
    assert_eq!(Route::parse("notify").unwrap(), Route::Notify);
    let scope = Scope::from(&job);
    let outcome = machine::evaluate(
        &mut job.machine, b"done", &job.buckets, &job.routes, None, scope,
        || Classification::regex(machine::Verdict { bucket: "done".into(), confidence: 1.0 }),
    );
    assert_eq!(outcome.reason, "transition");
    assert!(store.finish(&job).unwrap());
    let restored = Store::new(path, clock);
    assert_eq!(restored.get(&job.id).unwrap().machine.last_bucket.as_deref(), Some("done"));
}

#[test]
fn sentinel_handlers_paused_creation_is_persisted_and_never_due() {
    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let path = dir.path().join("sentinels.json");
    let mut store = Store::new(path.clone(), clock.clone());
    let mut value = definition();
    value["paused"] = json!(true);
    let params: Create = serde_json::from_value(value).expect("paused definition must be admitted");
    let job = store.create(params, owner(), false).unwrap();
    assert!(store.due().unwrap().is_empty());
    assert_eq!(serde_json::to_value(store.get(&job.id).unwrap()).unwrap()["machine"]["paused"], true);
    assert!(Store::new(path, clock).due().unwrap().is_empty());
}

#[test]
fn sentinel_handlers_declared_array_roundtrips_order_and_ids() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(dir.path().join("sentinels.json"), crate::cron::clock::MockClock::starting_at(1000));
    let mut value = definition();
    value["handlers"] = json!({"done": [
        {"id": "notice", "type": "notify", "parameters": {}},
        {"id": "finish", "type": "stop", "parameters": {}}
    ]});
    let params: Create = serde_json::from_value(value).expect("declared handlers must be admitted");
    let job = store.create(params, owner(), false).unwrap();
    let stored = serde_json::to_value(&job).unwrap();
    assert_eq!(stored["routes"]["done"][0]["id"], "notice");
    assert_eq!(stored["routes"]["done"][1]["id"], "finish");
}

#[test]
fn sentinel_handlers_exact_sentinel_route_is_a_supported_closed_action() {
    assert!(Route::parse("notify").is_ok());
    assert!(Route::parse("sentinel:sentinel_exact").is_ok());
    for value in ["command:echo", "sentinel:", "sentinel:peer/remote", "sentinel:peer::remote"] {
        assert!(Route::parse(value).is_err(), "{value}");
    }
}

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

fn create(store: &mut Store, kind: &str, parameters: Value) -> Result<super::store::Sentinel, String> {
    serde_json::from_value(definition(kind, parameters))
        .map_err(|error| error.to_string())
        .and_then(|params| store.create(params, owner(), false))
}

#[test]
fn sentinel_command_existing_notify_creation_positive() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(dir.path().join("sentinels.json"), crate::cron::clock::MockClock::starting_at(1000));
    let job = create(&mut store, "notify", json!({})).unwrap();
    assert_eq!(job.routes["done"][0].id, "action");
}

#[test]
fn sentinel_command_fixed_definition_admitted_through_create() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(dir.path().join("sentinels.json"), crate::cron::clock::MockClock::starting_at(1000));
    let result = create(&mut store, "command", json!({"command":"printf marker", "label":"Marker", "timeout_ms":1000}));
    assert!(result.is_ok(), "fixed creator command must be admitted: {result:?}");
}

#[test]
fn sentinel_command_fixed_definition_persists_creator_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sentinels.json");
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(path.clone(), clock.clone());
    let result = create(&mut store, "command", json!({"command":"printf marker", "label":"Marker", "timeout_ms":1000}));
    assert!(result.is_ok(), "fixed command creation must reach durable storage: {result:?}");
    let job = result.unwrap();
    let restored = Store::new(path, clock);
    assert_eq!(restored.get(&job.id).unwrap().cwd, owner().cwd);
}

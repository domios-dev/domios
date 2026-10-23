use super::{close, ServiceInstance};
use crate::{BlueprintApplyExt, SentinelExt};
use codeterm_service_core::cron::clock;

#[test]
fn sentinel_create_update_and_stop_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    let workspace = mux.find_workspace_for_tab(&tab_id).unwrap();
    let team = codeterm_types::team::Team::new(&workspace, 1);
    mux.upsert_team(team.clone());
    mux.get_tab_mut(&tab_id).unwrap().team_id = Some(team.id);
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let store = crate::store::Store::with_storage(&instance.storage, clock::system_clock());
    let handle =
        crate::scheduler::fixture_handle_with_ui(store, instance.fixture.service.ui.clone());
    crate::manage_fixture_ports(&instance.fixture.service, &handle);
    let caller = instance
        .fixture
        .service
        .sentinel_caller(&tab_id, None)
        .expect("resolve caller through ServiceContext");
    let mut document = serde_json::json!({"schema_version":2,"kind":"watch","name":"round-trip sentinel","steps":[
        {"id":"source","uses":"probe","with":{"command":{"lit":"status"}},"every":{"lit":"1m"},"timeout":{"lit":"1h"}},
        {"id":"decide","uses":"ask","with":{"question":{"lit":"initial question"}},"buckets":["done"],"matches":{"done":{"lit":"done"}},"routes":{"done":"notice"}},
        {"id":"notice","uses":"notify"}
    ]});
    let applied = instance
        .fixture
        .service
        .blueprint_apply(&caller, document.clone(), false)
        .unwrap();
    let created = instance
        .fixture
        .service
        .sentinel_show(&caller, applied["id"].as_str().unwrap())
        .unwrap();
    document["name"] = serde_json::json!("updated sentinel");
    document["steps"][0]["every"] = serde_json::json!({"lit":"2m"});
    document["steps"][1]["with"]["question"] = serde_json::json!({"lit":"updated question"});
    instance
        .fixture
        .service
        .blueprint_apply_document(
            &caller,
            document,
            false,
            None,
            Some((created.id.clone(), created.definition_version, None)),
        )
        .unwrap();
    instance
        .fixture
        .service
        .sentinel_stop(&caller, &created.id)
        .expect("stop sentinel through ServiceContext");

    let restarted = instance.restart(root.path());
    let handle = crate::scheduler::fixture_handle_with_ui(
        crate::store::Store::with_storage(&restarted.storage, clock::system_clock()),
        restarted.fixture.service.ui.clone(),
    );
    let restored = handle
        .store
        .lock()
        .get(&created.id)
        .expect("sentinel survives restart")
        .clone();
    assert_eq!(restored.name.as_deref(), Some("updated sentinel"));
    assert_eq!(restored.question, "updated question");
    assert!(restored.machine.stopped);
    assert_eq!(restored.last_reason.as_deref(), Some("stopped_by_creator"));
    crate::manage_fixture_ports(&restarted.fixture.service, &handle);
    close(restarted);
}

use super::{close, ServiceInstance};
use crate::SentinelExt;
use codeterm_service_core::cron::clock;
use tauri::Manager;

#[test]
fn sentinel_create_update_and_stop_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab_id) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let store = crate::store::Store::with_storage(&instance.storage, clock::system_clock());
    let handle = crate::scheduler::fixture_handle(store, instance.fixture._app.handle().clone());
    assert!(instance.fixture.service.app_handle.manage(handle.clone()));
    crate::manage_fixture_ports(&instance.fixture.service, &handle);
    let caller = instance
        .fixture
        .service
        .sentinel_caller(&tab_id, None)
        .expect("resolve caller through ServiceContext");
    let created = instance
        .fixture
        .service
        .sentinel_create(
            &caller,
            serde_json::from_value(serde_json::json!({
                "name": "round-trip sentinel",
                "probe": "status",
                "question": "initial question",
                "buckets": ["done"],
                "every": "1m",
                "matches": {"done": "done"}
            }))
            .unwrap(),
        )
        .expect("create sentinel through ServiceContext");
    instance
        .fixture
        .service
        .sentinel_update(
            &caller,
            &created.id,
            created.definition_version,
            serde_json::from_value(serde_json::json!({
                "name": "updated sentinel",
                "question": "updated question",
                "buckets": ["done"],
                "every": "2m",
                "matches": {"done": "done"},
                "max_transitions": created.max_transitions,
                "max_actions": created.max_actions
            }))
            .unwrap(),
        )
        .expect("update sentinel through ServiceContext");
    instance
        .fixture
        .service
        .sentinel_stop(&caller, &created.id)
        .expect("stop sentinel through ServiceContext");

    let restarted = instance.restart(root.path());
    let handle = crate::scheduler::fixture_handle(
        crate::store::Store::with_storage(&restarted.storage, clock::system_clock()),
        restarted.fixture._app.handle().clone(),
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
    assert!(restarted.fixture.service.app_handle.manage(handle.clone()));
    crate::manage_fixture_ports(&restarted.fixture.service, &handle);
    close(restarted);
}

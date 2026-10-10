use std::sync::Arc;

use tauri::Manager;

use crate::api::cron_routes::off_runtime_gate::StorageCron;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

/// The toggle and the unrelated command share one task on one runtime thread: the
/// unrelated branch can run only while the toggle is parked off that thread.
#[tokio::test]
#[cfg_attr(not(windows), ignore = "requires the desktop Tauri test runtime")]
async fn cron_entry_off_runtime_command_toggle_parks_commit_while_unrelated_command_completes() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let storage_cron = StorageCron::open();
    let held = storage_cron.create("held", "ws-held");
    let app = fixture.service.app_handle.clone();
    app.manage(storage_cron.cron.clone());
    app.manage(storage_cron.wake.clone());
    app.manage::<crate::service::ServiceState>(Arc::new(fixture.service.clone()));
    let mut gate = storage_cron.gate_next_commit();

    let toggle = super::cron_toggle(
        app.state(),
        app.state(),
        app.state(),
        held.id.clone(),
        false,
    );
    let unrelated = async {
        gate.parked().await;
        let listed = super::cron_list(app.state(), app.state(), Some("ws-held".into()))
            .expect("list completes while the commit is parked");
        let fetched =
            super::cron_get(app.state(), app.state(), held.id.clone()).expect("get completes");
        (listed, fetched, gate.release())
    };
    let (toggled, (listed, fetched, released)) = tokio::join!(toggle, unrelated);

    assert!(released, "the commit stayed parked until released");
    assert_eq!(
        listed[0]["enabled"], true,
        "the parked commit is not yet published"
    );
    assert_eq!(fetched["enabled"], true);
    assert_eq!(toggled.expect("toggle command")["enabled"], false);
    assert_eq!(storage_cron.enabled(&held.id), Some(false));
}

use super::{close, started};
use crate::cron::clock;
use crate::cron::store::{CronCreateParams, CronStorageOptions, CronStore};
use crate::service::ServiceContext;
use crate::storage::Storage;
use parking_lot::Mutex;
use std::sync::Arc;

fn cron_for(
    service: &ServiceContext,
    storage: &Storage,
    root: &std::path::Path,
) -> (crate::CronState, crate::CronWake) {
    service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_reasoning_efforts(
            "round-trip-agent",
            &["low", "medium", "high"],
        )
        .with_launch_binary(if cfg!(windows) { "cmd.exe" } else { "sh" }),
    ));
    let store = CronStore::open_storage(
        storage,
        root.join("crons"),
        clock::system_clock(),
        CronStorageOptions::default(),
        crate::config::schema::AppConfig::default()
            .general_agent
            .agent_provider,
    )
    .expect("startup hydrates cron from Storage");
    let cron = Arc::new(Mutex::new(store));
    let wake = Arc::new(std::sync::Condvar::new());
    assert!(service.set_ext(cron.clone()).is_ok());
    assert!(service.set_ext(wake.clone()).is_ok());
    (cron, wake)
}

fn cron_params(root: &std::path::Path, workspace_id: String) -> CronCreateParams {
    serde_json::from_value(serde_json::json!({
        "name": "round-trip schedule",
        "workspace_id": workspace_id,
        "schedule": "0 * * * *",
        "task": "round-trip task",
        "agent": "round-trip-agent",
        "cwd": root,
        "enabled": false,
        "jitter_max_ms": 0
    }))
    .expect("valid cron parameters")
}

#[test]
fn cron_create_update_delete_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let (cron, wake) = cron_for(&instance.fixture.service, &instance.storage, root.path());
    let created = instance
        .fixture
        .service
        .cron_create(&cron, &wake, cron_params(root.path(), workspace_id))
        .expect("create cron through ServiceContext");
    let updated = instance
        .fixture
        .service
        .cron_update(
            &cron,
            &wake,
            &created.id,
            serde_json::from_value(serde_json::json!({
                "name": "updated schedule",
                "task": "updated task"
            }))
            .unwrap(),
        )
        .expect("update cron through ServiceContext");
    assert_eq!(updated.task, "updated task");
    drop(cron);
    drop(wake);

    let restarted = instance.restart(root.path());
    let (cron, wake) = cron_for(&restarted.fixture.service, &restarted.storage, root.path());
    let restored = restarted
        .fixture
        .service
        .cron_get(&cron, &created.id)
        .expect("updated cron definition survives restart");
    assert_eq!(restored.name, "updated schedule");
    assert_eq!(restored.task, "updated task");
    restarted
        .fixture
        .service
        .cron_delete(&cron, &wake, &created.id)
        .expect("delete cron through ServiceContext");
    drop(cron);
    drop(wake);

    let restarted = restarted.restart(root.path());
    let (cron, wake) = cron_for(&restarted.fixture.service, &restarted.storage, root.path());
    assert!(restarted
        .fixture
        .service
        .cron_get(&cron, &created.id)
        .is_err());
    drop(cron);
    drop(wake);
    close(restarted);
}

#[test]
fn cron_run_start_finish_and_history_round_trip_expected_pass() {
    let root = tempfile::tempdir().unwrap();
    let instance = started(root.path());
    let workspace_id = instance.fixture.service.mux.read().list_workspaces()[0]
        .id
        .clone();
    let (cron, wake) = cron_for(&instance.fixture.service, &instance.storage, root.path());
    let created = instance
        .fixture
        .service
        .cron_create(&cron, &wake, cron_params(root.path(), workspace_id))
        .expect("create cron through ServiceContext");
    match instance.fixture.service.cron_trigger(&cron, &created.id) {
        Ok(run) => {
            instance
                .fixture
                .service
                .cron_complete(
                    &cron,
                    &wake,
                    &run.run_id,
                    Some("finished"),
                    None,
                    None,
                    true,
                )
                .expect("finish triggered cron run through ServiceContext");
        }
        Err(_) => {
            let history = instance
                .fixture
                .service
                .cron_history(&cron, &created.id)
                .expect("failed trigger still records its run");
            assert!(history.iter().any(|run| run.finished_at.is_some()));
        }
    }
    drop(cron);
    drop(wake);

    let restarted = instance.restart(root.path());
    let (cron, wake) = cron_for(&restarted.fixture.service, &restarted.storage, root.path());
    let history = restarted
        .fixture
        .service
        .cron_history(&cron, &created.id)
        .expect("cron history survives restart");
    assert!(!history.is_empty());
    assert!(history.iter().all(|run| run.finished_at.is_some()));
    drop(cron);
    drop(wake);
    close(restarted);
}

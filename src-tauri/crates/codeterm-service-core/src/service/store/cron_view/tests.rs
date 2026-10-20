use super::*;
use crate::service::store::ServicesStore;
#[test]
fn list_local_includes_cron_jobs_as_virtual_services() {
    use crate::cron::store::{CronCreateParams, CronStore};
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let cron_path = dir.path().join("crons.json");
    let mut cron_store = CronStore::new_with_path(cron_path);
    let cron = cron_store
        .create(CronCreateParams {
            name: "test-cron".into(),
            workspace_id: "ws".into(),
            team_id: None,
            detach_from_team: false,
            schedule: "0 * * * *".into(),
            task: "echo".into(),
            description: None,
            agent: None,
            model: None,
            reasoning_effort: None,
            cwd: None,
            role: None,
            skill: None,
            creator_tab_id: None,
            report_target_tab_id: None,
            enabled: Some(true),
            max_history: None,
            oneshot: None,
            jitter_max_ms: Some(0),
            tz: None,
            timeout_seconds: None,
        })
        .unwrap();
    cron_store.set_protected(&cron.id, true).unwrap();

    let svc_store = ServicesStore::open_in_memory().unwrap();
    svc_store
        .create(ServiceCreate {
            name: "test-daemon".into(),
            kind: ServiceKind::Daemon,
            description: None,
            host: None,
            url: Some("http://127.0.0.1:0".into()),
            port: None,
            process_label: None,
            script_path: None,
            health_mode: HealthMode::Http,
            health_path: None,
            health_interval: None,
            field_mapping: None,
            push_enabled: None,
            health_json_field: None,
            health_ok_values: None,
            health_degraded_values: None,
        })
        .unwrap();

    let unified = list_local(&svc_store, &cron_store).unwrap();
    assert_eq!(unified.len(), 2, "expected 1 cron + 1 daemon");

    let cron_svc = unified
        .iter()
        .find(|s| s.name == "test-cron")
        .expect("cron present");
    assert_eq!(cron_svc.kind, ServiceKind::Cron);
    assert_eq!(cron_svc.schedule.as_deref(), Some("0 * * * *"));
    assert!(cron_svc.next_run.is_some());
    assert!(cron_svc.protected);

    let daemon_svc = unified
        .iter()
        .find(|s| s.name == "test-daemon")
        .expect("daemon present");
    assert_eq!(daemon_svc.kind, ServiceKind::Daemon);
}

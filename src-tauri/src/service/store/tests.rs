use super::*;

#[test]
fn schema_initializes_in_memory() {
    let store = ServicesStore::open_in_memory().expect("schema init");
    let conn = store.conn();
    let conn = conn.lock();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM services", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM service_health", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM activity", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn create_and_get_daemon_service() {
    let store = ServicesStore::open_in_memory().unwrap();
    let id = store
        .create(ServiceCreate {
            name: "voice-transcribe".into(),
            kind: ServiceKind::Daemon,
            description: Some("local whisper".into()),
            host: None,
            url: Some("http://127.0.0.1:7891".into()),
            port: None,
            process_label: None,
            script_path: None,
            health_mode: HealthMode::Http,
            health_path: Some("/healthz".into()),
            health_interval: Some(30),
            field_mapping: None,
            push_enabled: Some(true),
            health_json_field: None,
            health_ok_values: None,
            health_degraded_values: None,
        })
        .unwrap();

    let svc = store.get(&id).unwrap().expect("service exists");
    assert_eq!(svc.name, "voice-transcribe");
    assert_eq!(svc.kind, ServiceKind::Daemon);
    assert_eq!(svc.url.as_deref(), Some("http://127.0.0.1:7891"));
    assert_eq!(svc.health_mode, HealthMode::Http);
    assert!(svc.push_enabled);
    assert!(svc.enabled);
    assert_eq!(svc.description.as_deref(), Some("local whisper"));
}

#[test]
fn list_returns_all_services() {
    let store = ServicesStore::open_in_memory().unwrap();
    store.create(make_input("alpha")).unwrap();
    store.create(make_input("beta")).unwrap();
    let all = store.list(None, None).unwrap();
    assert_eq!(all.len(), 2);
}

#[test]
fn health_history_is_capped_and_list_reads_latest_snapshot() {
    use super::schema::SERVICE_HEALTH_RETENTION;

    let store = ServicesStore::open_in_memory().unwrap();
    let id = store.create(make_input("history")).unwrap();
    for probed_at in 1..=SERVICE_HEALTH_RETENTION + 25 {
        store
            .record_health_snapshot(
                &id,
                &HealthSnapshot {
                    status: if probed_at == SERVICE_HEALTH_RETENTION + 25 {
                        HealthStatus::Down
                    } else {
                        HealthStatus::Healthy
                    },
                    probed_at: Some(probed_at),
                    ..Default::default()
                },
            )
            .unwrap();
    }

    let conn = store.conn();
    let conn = conn.lock();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM service_health", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, SERVICE_HEALTH_RETENTION);
    let mut plan = conn
        .prepare(
            "EXPLAIN QUERY PLAN
             SELECT rowid FROM service_health
             WHERE service_id = ?1
             ORDER BY probed_at DESC
             LIMIT 1",
        )
        .unwrap();
    let plan_details = plan
        .query_map([&id], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(
        plan_details
            .iter()
            .any(|detail| detail.contains("idx_health_service_time")),
        "latest-row query must use the service/time index: {plan_details:?}"
    );
    drop(plan);
    drop(conn);

    let service = store.list(None, None).unwrap().pop().unwrap();
    assert_eq!(service.health.status, HealthStatus::Down);
    assert_eq!(
        service.health.probed_at,
        Some(SERVICE_HEALTH_RETENTION + 25)
    );
}

#[test]
fn opening_an_existing_store_compacts_oversized_health_history() {
    use super::schema::SERVICE_HEALTH_RETENTION;
    use tempfile::tempdir;

    let directory = tempdir().unwrap();
    let path = directory.path().join("services.db");
    let store = ServicesStore::open(&path).unwrap();
    let id = store.create(make_input("oversized-history")).unwrap();
    let conn = store.conn();
    let mut conn = conn.lock();
    let tx = conn.transaction().unwrap();
    for probed_at in 1..=SERVICE_HEALTH_RETENTION + 25 {
        tx.execute(
            "INSERT INTO service_health (service_id, probed_at, status) VALUES (?1, ?2, 'healthy')",
            rusqlite::params![id, probed_at],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    drop(conn);
    drop(store);

    let store = ServicesStore::open(&path).unwrap();
    let conn = store.conn();
    let conn = conn.lock();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM service_health", [], |row| row.get(0))
        .unwrap();
    let oldest: i64 = conn
        .query_row("SELECT MIN(probed_at) FROM service_health", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, SERVICE_HEALTH_RETENTION);
    assert_eq!(oldest, 26);
}

#[test]
fn list_filters_by_kind() {
    let store = ServicesStore::open_in_memory().unwrap();
    store.create(make_input("daemon-svc")).unwrap();
    let daemons = store.list(Some(ServiceKind::Daemon), None).unwrap();
    assert_eq!(daemons.len(), 1);
    let crons = store.list(Some(ServiceKind::Cron), None).unwrap();
    assert_eq!(crons.len(), 0);
}

#[test]
fn update_changes_fields_and_bumps_updated_at() {
    let store = ServicesStore::open_in_memory().unwrap();
    let id = store.create(make_input("orig")).unwrap();
    let before = store.get(&id).unwrap().unwrap().updated_at;
    std::thread::sleep(std::time::Duration::from_millis(2));
    store
        .update(
            &id,
            ServiceUpdate {
                name: Some("renamed".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let after = store.get(&id).unwrap().unwrap();
    assert_eq!(after.name, "renamed");
    assert!(after.updated_at > before);
}

#[test]
fn delete_removes_service() {
    let store = ServicesStore::open_in_memory().unwrap();
    let id = store.create(make_input("doomed")).unwrap();
    store.delete(&id).unwrap();
    assert!(store.get(&id).unwrap().is_none());
}

#[test]
fn list_local_includes_cron_jobs_as_virtual_services() {
    use crate::cron::store::{CronCreateParams, CronStore};
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let cron_path = dir.path().join("crons.json");
    let mut cron_store = CronStore::new_with_path(cron_path);
    cron_store
        .create(CronCreateParams {
            name: "test-cron".into(),
            workspace_id: "ws".into(),
            schedule: "0 * * * *".into(),
            task: "echo".into(),
            description: None,
            agent: None,
            model: None,
            cwd: None,
            role: None,
            skill: None,
            creator_pane_id: None,
            enabled: Some(true),
            max_history: None,
            oneshot: None,
            jitter_max_ms: Some(0),
            tz: None,
            timeout_seconds: None,
        })
        .unwrap();

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

    let daemon_svc = unified
        .iter()
        .find(|s| s.name == "test-daemon")
        .expect("daemon present");
    assert_eq!(daemon_svc.kind, ServiceKind::Daemon);
}

fn make_input(name: &str) -> ServiceCreate {
    ServiceCreate {
        name: name.into(),
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
    }
}

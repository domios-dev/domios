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
    }
}

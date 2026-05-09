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

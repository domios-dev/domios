use super::*;
use crate::service::store::ServicesStore;

#[test]
fn list_caps_unsigned_limits_before_sqlite_conversion() {
    let store = ServicesStore::open_in_memory().unwrap();
    {
        let connection = store.conn();
        let connection = connection.lock();
        connection
            .execute_batch(
                "WITH RECURSIVE rows(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM rows WHERE n<10001)
                 INSERT INTO activity(occurred_at,source,message,level)
                 SELECT n,'peer','bounded','info' FROM rows;",
            )
            .unwrap();
    }
    let activity = ActivityWriter::new(store.conn());
    assert_eq!(
        activity.list(None, None, Some(usize::MAX)).unwrap().len(),
        MAX_ROWS
    );
    assert_eq!(activity.list(None, None, None).unwrap().len(), 500);
    assert!(activity.list(None, None, Some(0)).unwrap().is_empty());
    assert_eq!(
        activity
            .list(Some(ActivitySource::Peer), Some(0), Some(usize::MAX))
            .unwrap()
            .len(),
        MAX_ROWS
    );
}

#[test]
fn record_push_event_persists_with_source_metadata() {
    let store = ServicesStore::open_in_memory().unwrap();
    let activity = ActivityWriter::new(store.conn());
    activity
        .record(ActivityEvent::push(
            "svc_voice",
            "tab_a3f1",
            "transcript: hello",
        ))
        .unwrap();
    let events = activity.list(None, None, Some(10)).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].source, ActivitySource::Push);
    assert_eq!(events[0].source_id.as_deref(), Some("svc_voice"));
    assert_eq!(events[0].target.as_deref(), Some("tab_a3f1"));
}

#[test]
fn eviction_keeps_recent_drops_old() {
    let store = ServicesStore::open_in_memory().unwrap();
    let activity = ActivityWriter::new(store.conn());
    for i in 0..10003 {
        activity
            .record(ActivityEvent::peer("svc", "info", &format!("e{}", i)))
            .unwrap();
    }
    activity.evict_if_needed().unwrap();
    let count = activity.count().unwrap();
    assert!(count <= 10000, "got {}", count);
}

#[test]
fn list_filters_by_source() {
    let store = ServicesStore::open_in_memory().unwrap();
    let activity = ActivityWriter::new(store.conn());
    activity
        .record(ActivityEvent::push("svc1", "tab1", "x"))
        .unwrap();
    activity
        .record(ActivityEvent::peer("peer1", "info", "msg"))
        .unwrap();
    let pushes = activity
        .list(Some(ActivitySource::Push), None, None)
        .unwrap();
    assert_eq!(pushes.len(), 1);
    assert_eq!(pushes[0].source, ActivitySource::Push);
}

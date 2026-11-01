use super::*;
use parking_lot::Mutex;
use std::sync::Arc;

#[test]
fn blueprint_library_changes_follow_commits_without_connection_or_listener_locks() {
    let mem = MemManager::open_in_memory().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let weak = Arc::downgrade(&mem);
    let observed = events.clone();
    mem.set_blueprint_library_listener(Arc::new(move |area, revision| {
        let mem = weak.upgrade().unwrap();
        assert!(mem.fixture_conn().try_lock().is_some());
        assert!(mem.blueprint.changes.try_write().is_some());
        observed.lock().push((area.to_owned(), revision));
    }));
    let area = "blueprints:workspace";
    let saved = mem
        .save_blueprint(
            area,
            document("done"),
            &when("Watch CI failure"),
            evidence(),
        )
        .unwrap();
    assert_eq!(*events.lock(), vec![(area.into(), 1)]);
    assert_eq!(mem.list_blueprint_solutions(area).unwrap().len(), 1);
    mem.get(&saved.id).unwrap();
    assert_eq!(events.lock().len(), 1);
    mem.archive_blueprint_solution(area, &saved.id).unwrap();
    assert_eq!(*events.lock(), vec![(area.into(), 1), (area.into(), 2)]);
    assert_archived(&mem, &saved.id);
    assert!(mem.delete(&saved.id).unwrap());
    assert_eq!(events.lock().last(), Some(&(area.into(), 3)));
    assert!(!mem.delete(&saved.id).unwrap());
    assert_eq!(events.lock().len(), 3);
    assert!(mem.archive_blueprint_solution(area, &saved.id).is_err());
    assert_eq!(events.lock().len(), 3);
}

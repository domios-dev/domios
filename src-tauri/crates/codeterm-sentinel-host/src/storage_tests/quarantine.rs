use super::*;
use crate::storage::quarantine::INVALID_DEFINITION;

#[test]
fn storage_sentinel_invalid_blueprint_at_load_is_quarantined_and_the_rest_keep_running() {
    let root = temp_root();
    let clock = MockClock::starting_at(1000);
    let storage = open(root.path());
    let store = typed(&storage, &clock);
    let broken = create(&store, None, false, json!({}));
    let healthy = create(&store, None, false, json!({}));
    drop(store);
    close(storage);
    rusqlite::Connection::open(Storage::database_path(root.path()))
        .and_then(|conn| {
            conn.execute(
                "UPDATE blueprint_runtime SET run_transitions = 60000 WHERE blueprint_id = ?1",
                [&broken.id],
            )
        })
        .expect("plant one invalid committed Blueprint");

    let storage = open(root.path());
    let store = typed(&storage, &clock);
    {
        let current = store.lock();
        assert_eq!(
            current
                .list()
                .iter()
                .map(|job| job.id.as_str())
                .collect::<Vec<_>>(),
            vec![healthy.id.as_str()]
        );
        let quarantined = current.quarantined_in("workspace", None);
        assert_eq!(quarantined.len(), 1);
        assert_eq!(quarantined[0].id, broken.id);
        assert_eq!(quarantined[0].state, "quarantined");
        assert_eq!(quarantined[0].code, INVALID_DEFINITION);
        assert!(
            quarantined[0].reason.contains("budgets"),
            "{}",
            quarantined[0].reason
        );
    }
    let (scoped, _) = crate::storage::committed_scope(&store, "workspace", None).unwrap();
    assert_eq!(
        scoped.iter().map(|job| job.id.as_str()).collect::<Vec<_>>(),
        vec![healthy.id.as_str()]
    );
    let leases = due(&store);
    assert_eq!(
        leases
            .iter()
            .map(|lease| lease.job.id.as_str())
            .collect::<Vec<_>>(),
        vec![healthy.id.as_str()]
    );
    let added = create(&store, None, false, json!({}));
    assert!(store.lock().get(&added.id).is_some());
    assert_eq!(store.lock().quarantined_ids().len(), 1);
    drop(leases);
    drop(store);
    close(storage);
}

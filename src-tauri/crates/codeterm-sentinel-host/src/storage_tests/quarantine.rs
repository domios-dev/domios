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

#[test]
fn storage_quarantined_blueprint_delete_is_scoped_persisted_and_frees_name() {
    use crate::delete::{stage_delete, DeleteCaller, DeleteTarget};
    let root = temp_root();
    let clock = MockClock::starting_at(1000);
    let storage = open(root.path());
    let store = typed(&storage, &clock);
    let broken = create(&store, Some("team"), false, json!({}));
    drop(store);
    close(storage);
    rusqlite::Connection::open(Storage::database_path(root.path()))
        .unwrap()
        .execute(
            "UPDATE blueprint_runtime SET run_transitions = 60000 WHERE blueprint_id = ?1",
            [&broken.id],
        )
        .unwrap();
    let storage = open(root.path());
    let store = typed(&storage, &clock);
    let mut replacement = broken.clone();
    replacement.id = uuid::Uuid::new_v4().to_string();
    assert!(
        commit(&store, |store| store.stage_insert(replacement.clone()))
            .unwrap_err()
            .contains("blueprint_quarantined")
    );
    for caller in [
        DeleteCaller {
            tab: "stranger",
            workspace: "workspace",
            team: Some("team"),
            team_owner: false,
        },
        DeleteCaller {
            tab: "creator",
            workspace: "workspace",
            team: Some("other"),
            team_owner: true,
        },
    ] {
        assert!(stage_delete(
            &mut store.lock(),
            &caller,
            DeleteTarget::One(&broken.id),
            false
        )
        .is_err());
    }
    let caller = DeleteCaller {
        tab: "owner",
        workspace: "workspace",
        team: Some("team"),
        team_owner: true,
    };
    let staged = stage_delete(
        &mut store.lock(),
        &caller,
        DeleteTarget::One(&broken.id),
        false,
    )
    .unwrap();
    for (_, staged) in staged {
        staged.settle(&store).unwrap();
    }
    assert!(store.lock().quarantined.is_empty());
    assert!(store.lock().list().is_empty());
    assert!(done(storage.sentinels().inventory()).value.is_empty());
    commit(&store, |store| store.stage_insert(replacement.clone())).unwrap();
    drop(store);
    close(storage);
    let storage = open(root.path());
    let store = typed(&storage, &clock);
    assert_eq!(store.lock().list()[0].name, replacement.name);
    assert!(store.lock().quarantined.is_empty());
    drop(store);
    close(storage);
}

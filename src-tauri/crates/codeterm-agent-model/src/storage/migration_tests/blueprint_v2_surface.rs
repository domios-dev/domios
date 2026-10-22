use super::*;
#[test]
fn blueprint_v2_surface_startup_import_discards_old_graphs_and_keeps_other_domains_and_receipt() {
    let fixture = Fixture::seeded_with_desk();
    let originals = tree(&fixture.source());
    let storage = fixture.open();
    let receipt = committed(import_copied_root(&storage, &fixture.request()));
    assert!(sentinel_inventory(&storage).is_empty());
    let expected = decoded_cron();
    let found = cron_snapshot(&storage);
    assert_eq!(
        found
            .states
            .iter()
            .filter(|state| state.job.is_some())
            .count(),
        expected.jobs.len()
    );
    for original in expected.jobs {
        let stored = found
            .states
            .iter()
            .find_map(|state| {
                state
                    .job
                    .as_ref()
                    .filter(|job| job.record.id == original.record.id)
            })
            .unwrap();
        assert_eq!(stored.record, original.record);
        assert_eq!(stored.last_run, original.last_run);
    }
    let snapshot = storage.desk().snapshot().wait(DEADLINE).unwrap().value;
    assert!(snapshot
        .snapshot
        .workspaces
        .iter()
        .any(|workspace| workspace.record.id.as_str() == "ws-a"));
    assert!(marker(&storage).is_some());
    assert_eq!(marker(&storage).unwrap().revision, receipt.revision);
    assert_eq!(tree(&fixture.source()), originals);
    assert!(
        tree(&fixture.backup())
            .values()
            .any(|bytes| bytes == SENTINELS.as_bytes()),
        "raw obsolete graph receipt is retained"
    );
    close(storage);
    let reopened = fixture.open();
    assert!(sentinel_inventory(&reopened).is_empty());
    assert_eq!(cron_snapshot(&reopened).states.len(), found.states.len());
    close(reopened);
}

use super::*;

#[test]
fn cloud_hours_warn_once_per_crossing_and_rearm_after_top_up() {
    let mut observed = WireCloudCreditWarnings::default();
    let mut backend = WireCloudCreditWarnings {
        epoch: 1,
        ..Default::default()
    };
    assert!(newly_observed(&mut observed, backend).is_empty());
    backend.pct80_at = Some(101);
    assert_eq!(
        newly_observed(&mut observed, backend),
        [(Threshold::Percent80, 101)]
    );
    backend.pct95_at = Some(102);
    assert_eq!(
        newly_observed(&mut observed, backend),
        [(Threshold::Percent95, 102)]
    );
    backend.exhausted_at = Some(103);
    assert_eq!(
        newly_observed(&mut observed, backend),
        [(Threshold::Exhausted, 103)]
    );
    assert!(newly_observed(&mut observed, backend).is_empty());

    let next = WireCloudCreditWarnings {
        epoch: 2,
        ..Default::default()
    };
    assert!(newly_observed(&mut observed, next).is_empty());
    assert!(newly_observed(&mut observed, backend).is_empty());
    assert_eq!(
        newly_observed(
            &mut observed,
            WireCloudCreditWarnings {
                pct80_at: Some(103),
                ..next
            }
        ),
        [(Threshold::Percent80, 103)]
    );
}

#[test]
fn cloud_hours_stamps_are_shared_across_observations_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("warnings.json");
    let backend = WireCloudCreditWarnings {
        epoch: 17,
        pct80_at: Some(101),
        pct95_at: Some(102),
        exhausted_at: Some(103),
    };
    let mut ledger = WarningLedger::load(&path).unwrap();
    assert!(ledger.stage("owner-a", backend, "remaining hours"));
    assert_eq!(ledger.pending.len(), 3);
    ledger.save(&path).unwrap();
    let mut bell = NotificationStore::open_persisted(dir.path().join("notifications.json"), 10);
    assert_eq!(
        deliver_pending(&mut ledger, &mut bell, |ledger| ledger.save(&path))
            .unwrap()
            .len(),
        3
    );

    let mut ledger = WarningLedger::load(&path).unwrap();
    assert!(ledger.pending.is_empty());
    assert!(!ledger.stage("owner-a", backend, "updated hours"));
    assert!(ledger.stage("owner-b", backend, "remaining hours"));
    assert_eq!(ledger.pending.len(), 3);
}

#[test]
fn cloud_hours_restart_after_bell_write_before_mark_keeps_one_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("warnings.json");
    let bell_path = dir.path().join("notifications.json");
    let warnings = WireCloudCreditWarnings {
        epoch: 3,
        pct80_at: Some(101),
        ..Default::default()
    };
    let mut ledger = WarningLedger::default();
    assert!(ledger.stage("owner", warnings, "4.00 cloud hours left."));
    ledger.save(&path).unwrap();
    let id = Threshold::Percent80.notice_id("owner", 3);
    let mut bell = NotificationStore::open_persisted(bell_path.clone(), 10);
    let interrupted = deliver_pending(&mut ledger, &mut bell, |_| {
        Err(io::Error::other("interrupted before ledger mark"))
    });
    assert!(interrupted.is_err());
    assert_eq!(bell.list(false).len(), 1);
    assert_eq!(bell.list(false)[0].id, id);
    drop(bell);
    drop(ledger);

    let mut restarted = NotificationStore::open_persisted(bell_path, 10);
    let mut ledger = WarningLedger::load(&path).unwrap();
    assert_eq!(ledger.pending.len(), 1);
    assert!(
        deliver_pending(&mut ledger, &mut restarted, |ledger| ledger.save(&path))
            .unwrap()
            .is_empty()
    );
    let rows = restarted.list(false);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].title, "80% of cloud hours used");
    assert_eq!(rows[0].actions.as_ref().unwrap().len(), 2);
    assert!(WarningLedger::load(&path).unwrap().pending.is_empty());
}

#[test]
fn cloud_hours_restart_delivers_a_staged_warning_before_any_bell_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("warnings.json");
    let mut ledger = WarningLedger::default();
    assert!(ledger.stage(
        "owner",
        WireCloudCreditWarnings {
            epoch: 3,
            exhausted_at: Some(101),
            ..Default::default()
        },
        "0.00 cloud hours left."
    ));
    ledger.save(&path).unwrap();

    let mut restarted =
        NotificationStore::open_persisted(dir.path().join("notifications.json"), 10);
    let mut ledger = WarningLedger::load(&path).unwrap();
    let delivered =
        deliver_pending(&mut ledger, &mut restarted, |ledger| ledger.save(&path)).unwrap();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].id, Threshold::Exhausted.notice_id("owner", 3));
    assert_eq!(restarted.list(false).len(), 1);
    assert!(WarningLedger::load(&path).unwrap().pending.is_empty());
}

#[test]
fn cloud_hours_ledger_keeps_preexisting_delivered_stamps() {
    let mut ledger: WarningLedger = serde_json::from_value(serde_json::json!({
        "owner": { "epoch": 3, "pct80_at": 101 }
    }))
    .unwrap();
    let warnings = WireCloudCreditWarnings {
        epoch: 3,
        pct80_at: Some(101),
        ..Default::default()
    };
    assert!(!ledger.stage("owner", warnings, "4.00 cloud hours left."));
    assert!(ledger.pending.is_empty());
}

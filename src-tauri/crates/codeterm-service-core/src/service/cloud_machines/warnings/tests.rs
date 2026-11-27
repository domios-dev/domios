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
    let expected = [
        (Threshold::Percent80, 101),
        (Threshold::Percent95, 102),
        (Threshold::Exhausted, 103),
    ];
    assert_eq!(claim_warnings(&path, "owner-a", backend).unwrap(), expected);
    assert!(claim_warnings(&path, "owner-a", backend)
        .unwrap()
        .is_empty());
    assert_eq!(claim_warnings(&path, "owner-b", backend).unwrap(), expected);
}

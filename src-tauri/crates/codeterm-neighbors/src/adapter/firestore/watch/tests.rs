use super::*;
use fs::listen_response::ResponseType;

#[test]
fn repeated_existence_filter_resets_stop_at_the_explicit_retry_boundary() {
    let mut budget = ResetBudget::default();
    for _ in 0..3 {
        assert_eq!(budget.consume(), Ok(()));
    }
    for _ in 0..3 {
        assert_eq!(
            budget.consume(),
            Err(Error::ListenProtocol {
                kind: "existence_filter_retry_limit"
            })
        );
    }
}

fn response(kind: ResponseType) -> fs::ListenResponse {
    fs::ListenResponse {
        response_type: Some(kind),
    }
}

fn count(value: i32) -> fs::ListenResponse {
    response(ResponseType::Filter(fs::ExistenceFilter {
        target_id: 1,
        count: value,
        ..Default::default()
    }))
}

#[test]
fn pending_membership_caps_the_badge_and_bounds_transient_document_names() {
    let mut membership = Membership::new(true);
    assert_eq!(membership.pending_count(), Ok(0));
    for id in 0..500 {
        membership
            .observe(&response(ResponseType::DocumentChange(
                fs::DocumentChange {
                    document: Some(fs::Document {
                        name: format!("documents/{id}"),
                        ..Default::default()
                    }),
                    target_ids: vec![1],
                    ..Default::default()
                },
            )))
            .unwrap();
    }
    assert_eq!(membership.pending_count(), Ok(100));
    assert_eq!(membership.observe(&count(500)), Ok(false));
    let added = |name: &str| {
        response(ResponseType::DocumentChange(fs::DocumentChange {
            document: Some(fs::Document {
                name: name.into(),
                ..Default::default()
            }),
            target_ids: vec![1],
            ..Default::default()
        }))
    };
    assert_eq!(membership.observe(&added("documents/0")), Ok(false));
    assert_eq!(
        membership.observe(&added("documents/overflow")),
        Err(Error::ListenProtocol {
            kind: "pending_membership_limit"
        })
    );
    assert_eq!(membership.documents.len(), 500);
}

#[test]
fn pending_badge_tracks_duplicates_removals_resets_and_target_membership() {
    let mut membership = Membership::new(true);
    let mut change = fs::DocumentChange {
        document: Some(fs::Document {
            name: "documents/one".into(),
            ..Default::default()
        }),
        target_ids: vec![2],
        ..Default::default()
    };
    membership
        .observe(&response(ResponseType::DocumentChange(change.clone())))
        .unwrap();
    assert_eq!(membership.pending_count(), Ok(0));
    change.target_ids = vec![1];
    for _ in 0..2 {
        membership
            .observe(&response(ResponseType::DocumentChange(change.clone())))
            .unwrap();
    }
    assert_eq!(membership.pending_count(), Ok(1));
    membership.observe(&count(1)).unwrap();
    assert_eq!(
        membership.observe(&count(0)),
        Err(Error::ListenProtocol {
            kind: "existence_filter_mismatch"
        })
    );
    membership
        .observe(&response(ResponseType::DocumentRemove(
            fs::DocumentRemove {
                document: "documents/one".into(),
                removed_target_ids: vec![2],
                ..Default::default()
            },
        )))
        .unwrap();
    assert_eq!(membership.pending_count(), Ok(1));
    membership
        .observe(&response(ResponseType::DocumentRemove(
            fs::DocumentRemove {
                document: "documents/one".into(),
                removed_target_ids: vec![1],
                ..Default::default()
            },
        )))
        .unwrap();
    assert_eq!(membership.pending_count(), Ok(0));
    membership
        .observe(&response(ResponseType::DocumentChange(change.clone())))
        .unwrap();
    membership
        .observe(&response(ResponseType::DocumentDelete(
            fs::DocumentDelete {
                document: "documents/one".into(),
                removed_target_ids: vec![1],
                ..Default::default()
            },
        )))
        .unwrap();
    assert_eq!(membership.pending_count(), Ok(0));
    membership
        .observe(&response(ResponseType::DocumentChange(change.clone())))
        .unwrap();
    change.target_ids.clear();
    change.removed_target_ids = vec![1];
    membership
        .observe(&response(ResponseType::DocumentChange(change.clone())))
        .unwrap();
    assert_eq!(membership.pending_count(), Ok(0));
    assert_eq!(Membership::default().pending_count(), Err(Error::Gateway));
    change.target_ids = vec![1];
    change.removed_target_ids.clear();
    membership
        .observe(&response(ResponseType::DocumentChange(change)))
        .unwrap();
    assert_eq!(membership.pending_count(), Ok(1));
    assert!(!membership
        .observe(&response(ResponseType::TargetChange(fs::TargetChange {
            target_change_type: fs::target_change::TargetChangeType::Reset as i32,
            target_ids: vec![2],
            ..Default::default()
        })))
        .unwrap());
    assert_eq!(membership.pending_count(), Ok(1));
    assert!(membership
        .observe(&response(ResponseType::TargetChange(fs::TargetChange {
            target_change_type: fs::target_change::TargetChangeType::Reset as i32,
            target_ids: vec![1],
            ..Default::default()
        })))
        .unwrap());
    assert_eq!(membership.pending_count(), Ok(0));
}

#[test]
fn current_then_matching_existence_filter_keeps_empty_watch_alive() {
    let mut membership = Membership::default();
    let current = response(ResponseType::TargetChange(fs::TargetChange {
        target_change_type: fs::target_change::TargetChangeType::Current as i32,
        target_ids: vec![1],
        ..Default::default()
    }));
    assert!(!membership.observe(&current).unwrap());
    assert!(!membership.observe(&count(0)).unwrap());
    assert!(!membership.observe(&count(0)).unwrap());
}

#[test]
fn existence_filter_tracks_membership_through_updates_deletes_and_resets() {
    let mut membership = Membership::default();
    let added = response(ResponseType::DocumentChange(fs::DocumentChange {
        document: Some(fs::Document {
            name: "documents/one".into(),
            ..Default::default()
        }),
        target_ids: vec![1],
        ..Default::default()
    }));
    membership.observe(&added).unwrap();
    membership.observe(&added).unwrap();
    membership.observe(&count(1)).unwrap();
    assert_eq!(
        membership.observe(&count(0)),
        Err(Error::ListenProtocol {
            kind: "existence_filter_mismatch"
        })
    );
    let deleted = response(ResponseType::DocumentDelete(fs::DocumentDelete {
        document: "documents/one".into(),
        removed_target_ids: vec![1],
        ..Default::default()
    }));
    membership.observe(&deleted).unwrap();
    membership.observe(&count(0)).unwrap();
    membership.observe(&added).unwrap();
    let reset = response(ResponseType::TargetChange(fs::TargetChange {
        target_change_type: fs::target_change::TargetChangeType::Reset as i32,
        target_ids: vec![1],
        ..Default::default()
    }));
    assert!(membership.observe(&reset).unwrap());
    membership.observe(&count(0)).unwrap();
    membership.observe(&added).unwrap();
    membership.observe(&count(1)).unwrap();
}

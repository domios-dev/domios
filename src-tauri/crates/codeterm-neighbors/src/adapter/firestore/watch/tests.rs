use super::*;
use fs::listen_response::ResponseType;

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

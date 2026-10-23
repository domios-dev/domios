use super::support::{free, paid, with, LocalSigner, PERIOD_END, T};
use crate::floor::RevisionFloor;
use crate::verify::Refusal;
use serde_json::json;

#[test]
fn floor_refuses_lower_and_conflicting_revisions_and_admits_higher() {
    let signer = LocalSigner::new();
    let paid10 = signer.accept(&paid(), T);
    let floor = RevisionFloor::of(&signer.accept(&free(), T));
    assert_eq!(floor.revision, 11);
    assert_eq!(floor.admits(&paid10), Err(Refusal::Revision));
    assert_eq!(floor.admits(&signer.accept(&free(), T)), Ok(()));
    let other = signer.accept(&with(paid(), json!({"revision": 11})), T);
    assert_eq!(floor.admits(&other), Err(Refusal::RevisionConflict));
    let higher = signer.accept(&with(paid(), json!({"revision": 12})), T);
    assert_eq!(floor.admits(&higher), Ok(()));
}

#[test]
fn floor_only_rises() {
    let signer = LocalSigner::new();
    let high = RevisionFloor::of(&signer.accept(&free(), T));
    let low = signer.accept(&paid(), T);
    assert_eq!(RevisionFloor::raised(Some(&high), &low), high);
    let higher = signer.accept(&with(paid(), json!({"revision": 12})), T);
    assert_eq!(RevisionFloor::raised(Some(&high), &higher).revision, 12);
    assert_eq!(RevisionFloor::raised(None, &low).revision, 10);
}

#[test]
fn next_transition_walks_refresh_then_effective_end_then_none() {
    let signer = LocalSigner::new();
    let grant = signer.accept(&paid(), T);
    assert_eq!(grant.next_transition(T), Some(T + 3_600));
    assert_eq!(grant.next_transition(T + 3_600), Some(T + 86_400));
    assert_eq!(grant.next_transition(T + 86_400), None);
    let ending = signer.accept(&with(paid(), json!({"period_end": T + 7_200})), T);
    assert_eq!(ending.next_transition(T + 3_600), Some(T + 7_200));
    assert!(PERIOD_END > T + 86_400);
}

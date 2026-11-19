use super::*;

#[test]
fn retired_cleanup_cannot_remove_a_newer_or_failed_launch_profile() {
    let mut model = LeaseModel::default();
    model.install("old");
    let generation = model.generation();
    model.retire();
    assert!(model.permits_cleanup(generation));
    model.begin_launch();
    assert!(model.handle().is_none());
    assert!(!model.permits_cleanup(generation));
    model.install("new");
    assert!(!model.permits_cleanup(generation));
}

#[test]
fn two_acquires_share_one_handle_and_last_release_ends_ownership() {
    for reverse in [false, true] {
        let mut model = LeaseModel::default();
        model.install(42);
        let first = model.acquire(false).unwrap();
        let second = model.acquire(false).unwrap();
        assert_eq!((first.1, second.1), (42, 42));
        assert_ne!(first.0, second.0);
        let (early, last) = if reverse {
            (second.0, first.0)
        } else {
            (first.0, second.0)
        };
        assert_eq!(model.release(early), None);
        assert_eq!(model.handle(), Some(42));
        assert_eq!(model.release(last), Some(42));
        assert_eq!(model.handle(), None);
        assert_eq!(model.acquire(false), None);
    }
}

#[test]
fn stale_or_duplicate_release_cannot_close_a_new_generation() {
    let mut model = LeaseModel::default();
    model.install(10);
    let old = model.acquire(false).unwrap().0;
    assert_eq!(model.retire(), Some(10));
    model.install(20);
    let current = model.acquire(false).unwrap().0;
    assert_eq!(model.release(old), None);
    assert_eq!(model.handle(), Some(20));
    assert_eq!(model.release(current), Some(20));
    assert_eq!(model.release(current), None);
}

#[test]
fn exclusive_and_shared_profile_leases_never_overlap() {
    let mut model = LeaseModel::default();
    model.install(42);
    let (shared, _) = model.acquire(false).unwrap();
    assert_eq!(model.acquire(true), None);
    assert_eq!(model.release(shared), Some(42));

    model.install(43);
    let (exclusive, _) = model.acquire(true).unwrap();
    assert_eq!(model.acquire(true), None);
    assert_eq!(model.acquire(false), None);
    assert_eq!(model.release(exclusive), Some(43));

    model.install(44);
    assert!(model.acquire(false).is_some());
}

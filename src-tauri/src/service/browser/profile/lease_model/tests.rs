use super::*;

#[test]
fn two_acquires_share_one_handle_and_last_release_ends_ownership() {
    for reverse in [false, true] {
        let mut model = LeaseModel::default();
        model.install(42);
        let first = model.acquire().unwrap();
        let second = model.acquire().unwrap();
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
        assert_eq!(model.acquire(), None);
    }
}

#[test]
fn stale_or_duplicate_release_cannot_close_a_new_generation() {
    let mut model = LeaseModel::default();
    model.install(10);
    let old = model.acquire().unwrap().0;
    assert_eq!(model.retire(), Some(10));
    model.install(20);
    let current = model.acquire().unwrap().0;
    assert_eq!(model.release(old), None);
    assert_eq!(model.handle(), Some(20));
    assert_eq!(model.release(current), Some(20));
    assert_eq!(model.release(current), None);
}

use super::*;

#[test]
fn presence_notices_deduplicate_coalesce_and_bound_each_sender() {
    let mut notices = RegistryNotices::default();
    assert_eq!(notices.claim(1, "peer", 1, 100), Claim::Run);
    assert_eq!(notices.claim(1, "peer", 1, 100), Claim::Ignore);
    assert_eq!(notices.claim(1, "peer", 2, 100), Claim::Queued);
    assert_eq!(notices.claim(1, "peer", 3, 100), Claim::Ignore);
    assert!(notices.next(1));
    assert!(!notices.next(1));
    assert_eq!(notices.claim(1, "peer", 3, 110), Claim::Run);
    notices.stop(1);
    assert_eq!(notices.claim(2, "peer", 1, 110), Claim::Run);
}

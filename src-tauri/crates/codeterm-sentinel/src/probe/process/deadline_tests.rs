use super::*;

#[test]
fn receipt_clamps_runtime_and_reserves_cleanup_time() {
    let admitted = Instant::now();
    let deadlines =
        Deadlines::at(Duration::from_secs(10), Duration::from_secs(3), admitted).unwrap();
    let receipt = deadlines.receipt();

    assert_eq!(receipt.admitted, admitted);
    assert_eq!(receipt.duration, Duration::from_secs(3));
    assert_eq!(receipt.hard, admitted + Duration::from_millis(3_500));
    assert_eq!(receipt.operation, receipt.hard - Duration::from_millis(500));
    assert_eq!(
        deadlines.remaining_operation_at(admitted),
        Duration::from_secs(3)
    );
    assert!(deadlines.expired_operation_at(receipt.operation));
    assert_eq!(deadlines.remaining_hard_at(receipt.hard), Duration::ZERO);
}

#[test]
fn short_deadline_uses_quarter_reserve_and_records_injected_lateness() {
    let admitted = Instant::now();
    let deadlines =
        Deadlines::at(Duration::from_millis(200), Duration::from_secs(3), admitted).unwrap();
    let receipt = deadlines.receipt();
    assert_eq!(receipt.duration, Duration::from_millis(200));
    assert_eq!(receipt.operation, admitted + Duration::from_millis(150));
    assert_eq!(receipt.hard, admitted + Duration::from_millis(200));

    let mut failure = ProbeFailure::new("probe_timeout");
    deadlines.record_lateness_at(&mut failure, receipt.hard + Duration::from_millis(7));
    assert_eq!(failure.late_by, Some(Duration::from_millis(7)));
    assert!(matches!(
        Deadlines::at(Duration::MAX, Duration::MAX, admitted),
        Err(failure) if failure.primary == "probe_deadline_unavailable"
    ));
}

use super::*;

#[test]
fn target_current_signals_ready_even_for_an_empty_inbox() {
    let mut current = fs::TargetChange {
        target_change_type: fs::target_change::TargetChangeType::Current as i32,
        target_ids: vec![1],
        ..Default::default()
    };
    let checkpoint = target_checkpoint(&current).unwrap();
    assert!(checkpoint.ready);
    assert!(!checkpoint.flush);
    current.read_time = Some(prost_types::Timestamp {
        seconds: 10,
        nanos: 0,
    });
    let checkpoint = target_checkpoint(&current).unwrap();
    assert!(checkpoint.ready && checkpoint.flush);
    current.target_ids = vec![2];
    let checkpoint = target_checkpoint(&current).unwrap();
    assert!(!checkpoint.ready && !checkpoint.flush);
    current.target_ids = vec![1];
    current.target_change_type = fs::target_change::TargetChangeType::NoChange as i32;
    let checkpoint = target_checkpoint(&current).unwrap();
    assert!(!checkpoint.ready && checkpoint.flush);
}

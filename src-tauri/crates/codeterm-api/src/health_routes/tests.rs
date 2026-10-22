use super::IDENTIFY_FEATURES;

#[test]
fn identify_advertises_remote_task_dispatch_capability() {
    assert!(IDENTIFY_FEATURES.contains(&super::REMOTE_TASK_DISPATCH_FEATURE));
}

#[test]
fn identify_advertises_delayed_recurrence_capability() {
    assert!(IDENTIFY_FEATURES.contains(&"delayed_recurrence"));
}

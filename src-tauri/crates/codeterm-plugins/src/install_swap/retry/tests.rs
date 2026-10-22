use super::*;

#[test]
fn a_change_permits_retry_only_while_activation_is_possible() {
    let root = tempfile::tempdir().unwrap();
    let staging = root.path().join("staging");
    let destination = root.path().join("destination");
    std::fs::create_dir(&staging).unwrap();
    let (sender, events) = mpsc::channel();
    let event = || {
        notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Any))
            .add_path(staging.clone())
    };
    sender.send(Ok(event())).unwrap();
    assert!(changed(
        &events,
        Instant::now() + Duration::from_secs(1),
        &staging,
        &destination
    ));
    sender.send(Ok(event())).unwrap();
    std::fs::create_dir(&destination).unwrap();
    assert!(!changed(
        &events,
        Instant::now() + Duration::from_secs(1),
        &staging,
        &destination
    ));
    std::fs::remove_dir(&destination).unwrap();
    std::fs::remove_dir(&staging).unwrap();
    assert!(!changed(
        &events,
        Instant::now() + Duration::from_secs(1),
        &staging,
        &destination
    ));
}

#[test]
fn no_retry_after_deadline_or_watcher_failure() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("destination");
    let (sender, events) = mpsc::channel();
    assert!(!changed(&events, Instant::now(), root.path(), &destination));
    drop(sender);
    assert!(!changed(
        &events,
        Instant::now() + Duration::from_secs(1),
        root.path(),
        &destination
    ));
}

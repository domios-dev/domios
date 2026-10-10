use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;

fn event_handle(offset: u64) -> (TailerHandle, Arc<AtomicBool>) {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let (wake, events) = mpsc::sync_channel(1);
    let handle = TailerHandle::event_driven(
        stop.clone(),
        Arc::new(AtomicU64::new(offset)),
        thread::spawn(move || {
            while events.recv().is_ok() {
                if worker_stop.load(Ordering::Acquire) {
                    return;
                }
            }
        }),
        wake,
    );
    (handle, stop)
}

#[test]
fn resumed_session_replaces_persistent_stream_with_previous_launch_nonce() {
    let registry = ChatRegistry::default();
    let (old, old_stop) = event_handle(42);
    assert!(registry.insert(
        "tab".into(),
        "session".into(),
        Some("previous-launch".into()),
        "desktop-old".into(),
        old,
    ));
    registry.stop("tab", "desktop-old");
    assert!(registry.is_running("tab"));
    assert!(!registry.acquire("tab", "session", Some("current-launch"), "desktop-current"));
    assert!(old_stop.load(Ordering::Acquire));
    assert!(!registry.is_running("tab"));

    let (current, current_stop) = event_handle(84);
    assert!(registry.insert(
        "tab".into(),
        "session".into(),
        Some("current-launch".into()),
        "desktop-current".into(),
        current,
    ));
    assert!(registry.acquire("tab", "session", Some("current-launch"), "web-current"));
    registry.stop("tab", "desktop-old");
    assert!(!current_stop.load(Ordering::Acquire));
    assert_eq!(registry.current_offset("tab"), Some(84));
    registry.shutdown_tab("tab");
    assert!(current_stop.load(Ordering::Acquire));
}

#[test]
fn insert_replaces_previous_binding_and_deduplicates_current_binding() {
    let registry = ChatRegistry::default();
    let (old, old_stop) = event_handle(42);
    assert!(registry.insert("tab".into(), "session".into(), None, "old".into(), old));
    let (current, current_stop) = event_handle(84);
    assert!(registry.insert(
        "tab".into(),
        "session".into(),
        Some("bound-launch".into()),
        "current".into(),
        current,
    ));
    assert!(old_stop.load(Ordering::Acquire));
    let (duplicate, duplicate_stop) = event_handle(126);
    assert!(!registry.insert(
        "tab".into(),
        "session".into(),
        Some("bound-launch".into()),
        "web".into(),
        duplicate,
    ));
    assert!(duplicate_stop.load(Ordering::Acquire));
    assert!(!current_stop.load(Ordering::Acquire));
    assert_eq!(registry.current_offset("tab"), Some(84));
    registry.shutdown_tab("tab");
}

#[test]
fn previous_launch_store_promotion_cannot_replace_current_launch_stream() {
    let registry = ChatRegistry::default();
    let (current, current_stop) = event_handle(84);
    assert!(registry.insert(
        "tab".into(),
        "session".into(),
        Some("current-launch".into()),
        "desktop".into(),
        current,
    ));
    let (stale_promotion, stale_stop) = event_handle(0);
    assert!(!registry.replace_if_current(
        "tab",
        "session",
        Some("previous-launch"),
        stale_promotion
    ));
    assert!(stale_stop.load(Ordering::Acquire));
    assert!(!current_stop.load(Ordering::Acquire));
    assert_eq!(registry.current_offset("tab"), Some(84));
    registry.shutdown_tab("tab");
}

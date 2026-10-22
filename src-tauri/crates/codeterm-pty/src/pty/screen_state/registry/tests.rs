use super::*;
use std::sync::{mpsc, Barrier};
use std::time::Duration;

#[test]
fn concurrent_handle_drops_release_pins_before_retirement_lookup() {
    let screens = ScreenState::new();
    screens.bind_pty_generation("tab", 1);
    let first = screens.existing("tab").unwrap();
    let second = screens.existing("tab").unwrap();
    screens.remove("tab");
    let registry = screens.screens.lock();
    let start = Arc::new(Barrier::new(3));
    let (tx, rx) = mpsc::channel();
    let workers = [first, second].map(|handle| {
        let start = start.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            start.wait();
            drop(handle);
            tx.send(()).unwrap();
        })
    });
    drop(tx);
    start.wait();
    let first_drop = rx.recv_timeout(Duration::from_secs(1));
    drop(registry);
    for worker in workers {
        worker.join().unwrap();
    }
    first_drop.expect("a non-final pin must release without taking the registry lock");
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(!screens.contains_pty("tab"));
}

#[test]
fn reading_missing_or_removed_pty_never_creates_or_revives_an_entry() {
    let screens = ScreenState::new();
    assert!(screens.existing("missing").is_none());
    assert_eq!(screens.get_screen_text("missing"), None);
    assert!(!screens.contains_pty("missing"));

    screens.bind_pty_generation("tab", 1);
    screens.process("tab", b"current");
    let reader = screens.existing("tab").unwrap();
    screens.remove("tab");
    assert!(screens.existing("tab").is_none());
    assert_eq!(reader.lock().get_screen_text(), None);
    assert!(reader.try_lock().is_some());
    assert_eq!(screens.get_screen_text("tab"), None);
    assert_eq!(screens.get_content_hash("tab", 0), None);
    drop(reader);
    assert!(!screens.contains_pty("tab"));
    assert!(screens.existing("tab").is_none());
    assert!(!screens.contains_pty("tab"));
}

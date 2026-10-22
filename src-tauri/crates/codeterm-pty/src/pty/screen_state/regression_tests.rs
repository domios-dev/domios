use super::*;
use sha2::{Digest, Sha256};
use std::sync::mpsc;

#[test]
fn locked_parser_does_not_block_other_tabs_or_new_tab_insertion() {
    let screens = Arc::new(ScreenState::new());
    screens.bind_pty_generation("blocked", 1);
    let blocked_signal = screens.arm_screen_change_signal("blocked");
    let generation = blocked_signal.generation();
    let blocked = screens.pty("blocked");
    let guard = blocked.lock();
    let (tx, rx) = mpsc::channel();
    let worker_screens = screens.clone();
    let worker = std::thread::spawn(move || {
        worker_screens.bind_pty_generation("other", 2);
        let changed = worker_screens.arm_screen_change_signal("other");
        assert!(worker_screens.process_if_generation("other", 2, b"independent"));
        worker_screens.resize("other", 80, 24);
        let text = worker_screens.get_screen_text("other");
        let hash = worker_screens.get_content_hash("other", 0);
        tx.send((text, hash, changed.generation())).unwrap();
    });

    let result = rx.recv_timeout(Duration::from_secs(1));
    assert_eq!(blocked_signal.generation(), generation);
    drop(guard);
    worker.join().unwrap();
    let (text, hash, changed) =
        result.expect("another tab must finish while the first stays locked");
    assert_eq!(text.as_deref(), Some("independent"));
    assert_eq!(hash, Some(Sha256::digest(b"independent").into()));
    assert_eq!(changed, 1);
    assert!(screens.process_if_generation("blocked", 1, b"released"));
    assert_eq!(
        screens.get_screen_text("blocked").as_deref(),
        Some("released")
    );
}

#[test]
fn retained_parser_handle_cannot_process_a_replaced_or_removed_generation() {
    let screens = ScreenState::new();
    screens.bind_pty_generation("tab", 1);
    let old = screens.pty("tab");
    let signal = screens.arm_screen_change_signal("tab");
    let generation = signal.generation();
    assert!(screens.bind_pty_generation("tab", 2));
    assert!(!signal.wait_after(generation, Duration::ZERO));
    assert!(!old.lock().process_if_generation(1, b"stale"));
    assert!(screens.process_if_generation("tab", 2, b"current"));
    assert!(!screens.remove_if_generation("tab", 1));
    assert_eq!(screens.get_screen_text("tab").as_deref(), Some("current"));
    assert!(screens.remove_if_generation("tab", 2));
    assert!(!old.lock().process_if_generation(2, b"removed"));
    assert_eq!(screens.get_screen_text("tab"), None);
}

#[test]
fn pinned_waiter_stays_attached_when_armed_after_removal() {
    let screens = ScreenState::new();
    screens.bind_pty_generation("tab", 1);
    let pinned = screens.pty("tab");
    screens.remove("tab");
    let signal = pinned.lock_for_update().arm_screen_change_signal();
    let generation = signal.generation();
    drop(pinned);
    screens.bind_pty_generation("tab", 2);
    assert!(screens.process_if_generation("tab", 2, b"replacement"));
    assert!(signal.wait_after(generation, Duration::ZERO));
    screens.remove("tab");
    assert!(!signal.wait_after(signal.generation(), Duration::ZERO));
}

fn reference_hash(screen: &vt100::Screen, excluded: u16) -> [u8; 32] {
    let (rows, cols) = screen.size();
    let mut hash = Sha256::new();
    for row in 0..rows.saturating_sub(excluded) {
        let text = screen.contents_between(row, 0, row, cols);
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            hash.update(trimmed.as_bytes());
        }
    }
    hash.finalize().into()
}

#[test]
fn hash_equality_keeps_trimmed_row_concatenation_semantics() {
    let screens = ScreenState::new();
    for (tab, bytes) in [
        ("first", b"  ab  \r\n\r\n c  ".as_slice()),
        ("second", b" a \r\n bc ".as_slice()),
    ] {
        screens.process(tab, bytes);
        assert_eq!(
            screens.get_content_hash(tab, 0),
            Some(Sha256::digest(b"abc").into())
        );
    }
}

#[test]
fn incremental_hash_matches_original_bytes_across_terminal_operations() {
    let screens = ScreenState::new();
    let mut reference = vt100::Parser::new(8, 16, 500);
    screens.resize("tab", 16, 8);
    let chunks: &[&[u8]] = &[
        b"  alpha  \r\n\r\n beta \t",
        b"\x1b[2;4H\x1b[31mcolored\x1b[0m",
        "\r\nwide 界 and e\u{301}".as_bytes(),
        b"\xe2\x96",
        b"\x8c\x1b[",
        b"?25l\x1b[?2004h",
        b"\x1b[8;1Hbottom",
        b"\r\nscroll\r\nagain",
        b"\x1b[?1049halternate\x1b[2;1Hrows",
        b"\x1b[?1049l",
        b"\x1b[H\x1b[2K   replaced  ",
        b"\x1b[2J\x1b[Hclear",
        b"\x1bcreset",
    ];
    for chunk in chunks {
        screens.process("tab", chunk);
        reference.process(chunk);
        for excluded in [3, 0, 1, 8, u16::MAX, 0] {
            assert_eq!(
                screens.get_content_hash("tab", excluded),
                Some(reference_hash(reference.screen(), excluded))
            );
        }
    }
    for (cols, rows) in [(10, 4), (24, 12), (16, 8)] {
        screens.resize("tab", cols, rows);
        reference.screen_mut().set_size(rows, cols);
        for excluded in [0, 1, 3, u16::MAX] {
            assert_eq!(
                screens.get_content_hash("tab", excluded),
                Some(reference_hash(reference.screen(), excluded))
            );
        }
    }
}

#[test]
fn hash_is_lazy_and_reuses_unchanged_row_prefixes() {
    let screens = ScreenState::new();
    screens.resize("tab", 120, 50);
    for row in 1..=50 {
        screens.process("tab", format!("\x1b[{row};1Hrow-{row:02}").as_bytes());
    }
    let tab = screens.pty("tab");
    assert_eq!(tab.lock().content_hash.rows_hashed, 0);
    let first = screens.get_content_hash("tab", 0).unwrap();
    assert_eq!(tab.lock().content_hash.rows_hashed, 50);
    assert_eq!(screens.get_content_hash("tab", 0), Some(first));
    assert_eq!(tab.lock().content_hash.rows_hashed, 50);

    screens.process("tab", b"\x1b[50;1Hchanged");
    assert_eq!(tab.lock().content_hash.rows_hashed, 50);
    let second = screens.get_content_hash("tab", 0).unwrap();
    assert_ne!(first, second);
    assert_eq!(tab.lock().content_hash.rows_hashed, 51);

    screens.process("tab", b"\x1b[50;1H\x1b[31mchanged\x1b[0m\x1b[?25l");
    assert_eq!(screens.get_content_hash("tab", 0), Some(second));
    assert_eq!(tab.lock().content_hash.rows_hashed, 51);
    let excluded = screens.get_content_hash("tab", 1).unwrap();
    screens.process("tab", b"\x1b[50;1Hstatus!");
    assert_eq!(screens.get_content_hash("tab", 1), Some(excluded));
    assert_eq!(tab.lock().content_hash.rows_hashed, 51);
    assert_ne!(screens.get_content_hash("tab", 0), Some(second));
    assert_eq!(tab.lock().content_hash.rows_hashed, 52);
}

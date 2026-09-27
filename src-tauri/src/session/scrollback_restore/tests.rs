use super::{
    load_restored_scrollback_tails, seed_scrollback_tail_before_link, ScrollbackRestoreOutcome,
};
use crate::lock_hold::TimedMutex;
use crate::multiplexer::state::MultiplexerState;
use crate::pty::manager::OutputBuffer;
use crate::session::scrollback::DEFAULT_SCROLLBACK_TAIL_BYTES;
use crate::session::store::SessionStore;
use crate::session::SessionState;
use crate::OutputBufferState;
use parking_lot::Mutex;
use std::sync::Arc;

fn pending_shell() -> (MultiplexerState, String) {
    let mut mux = MultiplexerState::empty();
    let workspace = mux.create_workspace("restore-test".into(), None);
    let tab_id = mux.create_tab(&workspace.id).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.scrollback_seed_pending = true;
    (mux, tab_id)
}

#[test]
fn restored_snapshot_marks_plain_shells_for_scrollback_seeding() {
    let (mut mux, shell_id) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let agent_id = mux.create_tab(&workspace_id).unwrap();
    mux.get_tab_mut(&agent_id).unwrap().agent_type = Some("codex".into());

    let restored = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());

    assert!(restored.get_tab(&shell_id).unwrap().scrollback_seed_pending);
    assert!(!restored.get_tab(&agent_id).unwrap().scrollback_seed_pending);
}

#[test]
fn restored_tail_is_loaded_once_into_pending_tab_state() {
    let (mut mux, tab_id) = pending_shell();
    let directory = tempfile::tempdir().unwrap();
    let store = SessionStore::for_test_dir(directory.path().to_path_buf());
    let tail = b"prior shell history\r\n";
    store
        .save_scrollback_tail(&tab_id, tail, DEFAULT_SCROLLBACK_TAIL_BYTES)
        .unwrap();
    let session: SessionState = Arc::new(Mutex::new(store));

    assert!(load_restored_scrollback_tails(&mut mux, &session).is_empty());

    let tab = mux.get_tab(&tab_id).unwrap();
    assert!(tab.scrollback_seed_pending);
    assert_eq!(
        tab.pending_scrollback_tail.as_deref(),
        Some(tail.as_slice())
    );
    assert!(load_restored_scrollback_tails(&mut mux, &session).is_empty());
}

#[test]
fn missing_sidecar_clears_pending_seed_so_new_output_can_be_saved() {
    let (mut mux, tab_id) = pending_shell();
    let directory = tempfile::tempdir().unwrap();
    let session: SessionState = Arc::new(Mutex::new(SessionStore::for_test_dir(
        directory.path().to_path_buf(),
    )));

    assert_eq!(
        load_restored_scrollback_tails(&mut mux, &session),
        vec![(tab_id.clone(), "sidecar_missing_or_empty")]
    );
    assert!(!mux.get_tab(&tab_id).unwrap().scrollback_seed_pending);
}

#[test]
fn attached_stream_does_not_observe_shifted_offsets_when_seed_is_deferred() {
    let output_buf: OutputBufferState =
        Arc::new(TimedMutex::new("output_buf", OutputBuffer::new(65_536)));
    let pty_id = "pty-with-attached-stream";
    let fresh_banner = b"fresh banner\r\n";
    output_buf.lock().append(pty_id, fresh_banner);
    let held_offset = output_buf.lock().total_written(pty_id);

    let result = seed_scrollback_tail_before_link(pty_id, b"prior history\r\n", true, &output_buf);

    assert!(matches!(
        result,
        ScrollbackRestoreOutcome::Skipped {
            reason: "backend_reader_already_attached"
        }
    ));
    let buffer = output_buf.lock();
    assert_eq!(buffer.total_written(pty_id), held_offset);
    assert_eq!(
        buffer.read_since(pty_id, held_offset),
        (Vec::new(), held_offset)
    );
}

#[test]
fn seed_precedes_first_live_read_and_keeps_stream_offsets_monotonic() {
    let output_buf: OutputBufferState =
        Arc::new(TimedMutex::new("output_buf", OutputBuffer::new(65_536)));
    let pty_id = "pty-seeded-before-link";
    let prior = b"prior history\r\n";

    assert!(matches!(
        seed_scrollback_tail_before_link(pty_id, prior, false, &output_buf),
        ScrollbackRestoreOutcome::Seeded { bytes } if bytes == prior.len()
    ));
    output_buf.lock().append(pty_id, b"fresh banner\r\n");
    let buffer = output_buf.lock();
    let (all, total) = buffer.read_since(pty_id, 0);
    assert_eq!(all.as_slice(), b"prior history\r\nfresh banner\r\n");
    assert_eq!(total, all.len() as u64);
}

#[test]
fn link_seeding_uses_the_same_absolute_offset_as_native_history_reads() {
    let output_buf: OutputBufferState =
        Arc::new(TimedMutex::new("output_buf", OutputBuffer::new(65_536)));
    let pty_id = "pty-native-and-web";
    let prior = b"prior history\r\n";
    assert!(matches!(
        seed_scrollback_tail_before_link(pty_id, prior, false, &output_buf),
        ScrollbackRestoreOutcome::Seeded { .. }
    ));
    output_buf.lock().append(pty_id, b"fresh banner\r\n");
    let buf = output_buf.lock();
    let native_history = buf.get_full(pty_id).unwrap();
    let web_history = buf.read_since(pty_id, 0);
    assert_eq!(native_history, web_history.0.as_slice());
    assert_eq!(buf.total_written(pty_id), web_history.1);
}

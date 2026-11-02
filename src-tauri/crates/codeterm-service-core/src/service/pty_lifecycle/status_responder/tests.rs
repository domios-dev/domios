use super::*;
use std::sync::mpsc;
use std::time::Instant;

const DEADLINE: Duration = Duration::from_secs(5);

fn wait_for(owner: &ReplyOwner, predicate: impl Fn(&QueueState) -> bool) {
    let deadline = Instant::now() + DEADLINE;
    let mut state = owner.shared.state.lock();
    while !predicate(&state) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "reply owner did not reach expected state"
        );
        owner.shared.changed.wait_for(&mut state, remaining);
    }
}

#[test]
fn held_writer_preserves_fifo_and_bounds_inflight_plus_pending() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let written = bytes.clone();
    let mut first = true;
    let owner = ReplyOwner::with_writer("fifo", 7, move |reply| {
        if first {
            first = false;
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(DEADLINE).unwrap();
        }
        written.lock().extend_from_slice(reply);
        Ok(())
    });
    assert!(owner.enqueue(b"a"));
    entered_rx.recv_timeout(DEADLINE).unwrap();
    let mut expected = vec![b'a'];
    for index in 1..CAPACITY {
        let reply = if index % 2 == 0 { b"b" } else { b"c" };
        assert!(owner.enqueue(reply));
        expected.extend_from_slice(reply);
    }
    assert!(!owner.enqueue(b"discarded"));
    assert!(!owner.enqueue(b"discarded"));
    {
        let state = owner.shared.state.lock();
        assert!(state.in_flight);
        assert_eq!(state.pending.len(), CAPACITY - 1);
        assert_eq!(state.overflow, 2);
        assert!(!state.closed);
        assert!(bytes.lock().is_empty());
    }
    release_tx.send(()).unwrap();
    wait_for(&owner, |state| state.pending.is_empty() && !state.in_flight);
    assert_eq!(*bytes.lock(), expected);
    assert!(owner.enqueue(b"d"));
    wait_for(&owner, |state| state.pending.is_empty() && !state.in_flight);
    expected.push(b'd');
    assert_eq!(*bytes.lock(), expected);
}

#[test]
fn writer_failure_and_uncertain_ack_close_without_retry() {
    for failure in ["broken pipe", "PTY write acknowledgement timed out"] {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let owner = ReplyOwner::with_writer("failure", 8, move |reply| {
            observed.lock().push(reply.to_vec());
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(DEADLINE).unwrap();
            Err(failure.into())
        });
        assert!(owner.enqueue(b"first"));
        entered_rx.recv_timeout(DEADLINE).unwrap();
        assert!(owner.enqueue(b"pending"));
        release_tx.send(()).unwrap();
        wait_for(&owner, |state| state.closed);
        assert!(!owner.enqueue(b"later"));
        assert!(owner.shared.state.lock().pending.is_empty());
        assert_eq!(*calls.lock(), vec![b"first".to_vec()]);
    }
}

#[test]
fn teardown_discards_pending_and_cannot_target_replacement_generation() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (old_done_tx, old_done_rx) = mpsc::channel();
    let old_bytes = Arc::new(Mutex::new(Vec::new()));
    let old_written = old_bytes.clone();
    struct Ended(mpsc::Sender<()>);
    impl Drop for Ended {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let ended = Ended(old_done_tx);
    let old = ReplyOwner::with_writer("reused-id", 9, move |reply| {
        let _keep = &ended;
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(DEADLINE).unwrap();
        old_written.lock().extend_from_slice(reply);
        Ok(())
    });
    assert!(old.enqueue(b"in-flight-old"));
    entered_rx.recv_timeout(DEADLINE).unwrap();
    assert!(old.enqueue(b"pending-old"));
    drop(old);
    let replacement_bytes = Arc::new(Mutex::new(Vec::new()));
    let replacement_written = replacement_bytes.clone();
    let replacement = ReplyOwner::with_writer("reused-id", 10, move |reply| {
        replacement_written.lock().extend_from_slice(reply);
        Ok(())
    });
    assert!(replacement.enqueue(b"replacement"));
    wait_for(&replacement, |state| {
        state.pending.is_empty() && !state.in_flight
    });
    release_tx.send(()).unwrap();
    old_done_rx.recv_timeout(DEADLINE).unwrap();
    assert_eq!(*old_bytes.lock(), b"in-flight-old");
    assert_eq!(*replacement_bytes.lock(), b"replacement");
}

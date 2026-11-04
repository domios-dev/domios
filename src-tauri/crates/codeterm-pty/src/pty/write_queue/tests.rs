use super::*;
use std::sync::{Arc, Mutex};

struct MockWriter {
    data: Arc<Mutex<Vec<u8>>>,
}

impl Write for MockWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.data.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct FirstWriteBarrier {
    data: Arc<Mutex<Vec<u8>>>,
    first_entered: mpsc::SyncSender<()>,
    release_first: mpsc::Receiver<()>,
    first: bool,
}

impl Write for FirstWriteBarrier {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.first {
            self.first = false;
            self.first_entered.send(()).unwrap();
            self.release_first
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
        self.data.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn enqueue_callbacks_keep_payload_order_under_contention() {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let queue = Arc::new(WriteQueue::new(Box::new(MockWriter {
        data: writes.clone(),
    })));
    let callbacks = Arc::new(Mutex::new(Vec::new()));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let first_queue = queue.clone();
    let first_callbacks = callbacks.clone();
    let first = std::thread::spawn(move || {
        first_queue
            .send_and_wait_with_enqueued(b"first".to_vec(), Duration::from_secs(2), || {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                first_callbacks.lock().unwrap().push(1);
            })
            .unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let second_queue = queue.clone();
    let second_callbacks = callbacks.clone();
    let second = std::thread::spawn(move || {
        second_queue
            .send_and_wait_with_enqueued(b"second".to_vec(), Duration::from_secs(2), || {
                second_callbacks.lock().unwrap().push(2);
            })
            .unwrap();
    });
    release_tx.send(()).unwrap();
    first.join().unwrap();
    second.join().unwrap();
    assert_eq!(*callbacks.lock().unwrap(), [1, 2]);
    assert_eq!(*writes.lock().unwrap(), b"firstsecond");
}

#[derive(Debug, PartialEq, Eq)]
enum RawWriteEvent {
    Write(Vec<u8>),
    Flush,
}

struct RawBoundaryWriter {
    events: Arc<Mutex<Vec<RawWriteEvent>>>,
    flushed: mpsc::SyncSender<usize>,
    flushes: usize,
}

impl Write for RawBoundaryWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.events
            .lock()
            .unwrap()
            .push(RawWriteEvent::Write(bytes.to_vec()));
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.events.lock().unwrap().push(RawWriteEvent::Flush);
        self.flushes += 1;
        self.flushed.send(self.flushes).unwrap();
        Ok(())
    }
}

#[test]
fn sequential_raw_sends_preserve_write_and_flush_boundaries_in_order() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let (flushed, observed) = mpsc::sync_channel(2);
    let queue = WriteQueue::new(Box::new(RawBoundaryWriter {
        events: events.clone(),
        flushed,
        flushes: 0,
    }));

    queue.send(b"/model".to_vec()).unwrap();
    queue.send(b"\r".to_vec()).unwrap();

    assert_eq!(observed.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
    assert_eq!(observed.recv_timeout(Duration::from_secs(1)).unwrap(), 2);
    assert_eq!(
        *events.lock().unwrap(),
        [
            RawWriteEvent::Write(b"/model".to_vec()),
            RawWriteEvent::Flush,
            RawWriteEvent::Write(b"\r".to_vec()),
            RawWriteEvent::Flush,
        ]
    );
}

#[test]
fn send_and_receive() {
    let collected = Arc::new(Mutex::new(Vec::new()));
    let writer = MockWriter {
        data: collected.clone(),
    };
    let queue = WriteQueue::new(Box::new(writer));

    queue.send(b"hello".to_vec()).unwrap();
    queue.send(b" world".to_vec()).unwrap();

    queue
        .send_and_wait(Vec::new(), Duration::from_secs(1))
        .unwrap();
    drop(queue);

    assert_eq!(&*collected.lock().unwrap(), b"hello world");
}

#[test]
fn fifo_ordering() {
    let collected = Arc::new(Mutex::new(Vec::new()));
    let writer = MockWriter {
        data: collected.clone(),
    };
    let queue = WriteQueue::new(Box::new(writer));

    for i in 0..10u8 {
        queue.send(vec![i]).unwrap();
    }
    queue
        .send_and_wait(Vec::new(), Duration::from_secs(1))
        .unwrap();
    drop(queue);

    let data = collected.lock().unwrap();
    let expected: Vec<u8> = (0..10).collect();
    assert_eq!(&*data, &expected);
}

#[test]
fn send_after_close_returns_error() {
    let queue = WriteQueue::new(Box::new(std::io::sink()));
    queue.close();

    let result = queue.send(b"test".to_vec());
    assert!(result.is_err());
}

#[test]
fn expired_ack_wait_is_typed_and_distinct_from_a_closed_queue() {
    let data = Arc::new(Mutex::new(Vec::new()));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let queue = WriteQueue::new(Box::new(FirstWriteBarrier {
        data: data.clone(),
        first_entered: entered_tx,
        release_first: release_rx,
        first: true,
    }));
    queue.send(b"blocker".to_vec()).unwrap();
    entered_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("first queue request must block at the writer");

    let error = queue
        .send_and_wait(b"queued".to_vec(), Duration::ZERO)
        .unwrap_err();
    assert!(is_write_ack_timeout(&error), "got: {error}");
    assert!(!is_stale_agent_input(&error));

    // The request was enqueued: once the writer unblocks, the bytes land.
    release_tx.send(()).unwrap();
    queue
        .send_and_wait(b"-after".to_vec(), Duration::from_secs(1))
        .unwrap();
    assert_eq!(&*data.lock().unwrap(), b"blockerqueued-after");
}

#[test]
fn enqueue_notification_precedes_the_writer_acknowledgement() {
    let data = Arc::new(Mutex::new(Vec::new()));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let queue = Arc::new(WriteQueue::new(Box::new(FirstWriteBarrier {
        data,
        first_entered: entered_tx,
        release_first: release_rx,
        first: true,
    })));
    queue.send(b"blocker".to_vec()).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();

    let writer_queue = queue.clone();
    let (queued_tx, queued_rx) = mpsc::sync_channel(1);
    let (finished_tx, finished_rx) = mpsc::sync_channel(1);
    let writer = std::thread::spawn(move || {
        let result = writer_queue.send_and_wait_with_enqueued(
            b"payload".to_vec(),
            Duration::from_secs(2),
            || queued_tx.send(()).unwrap(),
        );
        finished_tx.send(result).unwrap();
    });

    queued_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        finished_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    release_tx.send(()).unwrap();
    finished_rx
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    writer.join().unwrap();
}

#[test]
fn frame_batch_prevents_another_request_from_splitting_paste_and_submit() {
    let data = Arc::new(Mutex::new(Vec::new()));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let queue = Arc::new(WriteQueue::new(Box::new(FirstWriteBarrier {
        data: data.clone(),
        first_entered: entered_tx,
        release_first: release_rx,
        first: true,
    })));
    let initial_queue = queue.clone();
    let initial = std::thread::spawn(move || {
        initial_queue.send_frames_and_wait(
            vec![b"paste".to_vec(), b"\r".to_vec()],
            Duration::from_secs(5),
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("paste frame must reach the writer");

    queue.send(b"next".to_vec()).unwrap();
    release_tx.send(()).unwrap();
    initial.join().unwrap().unwrap();
    queue
        .send_and_wait(b"tail".to_vec(), Duration::from_secs(1))
        .unwrap();

    assert_eq!(&*data.lock().unwrap(), b"paste\rnexttail");
}

#[test]
fn revoked_permit_writes_no_bytes_and_keeps_queue_live() {
    let data = Arc::new(Mutex::new(Vec::new()));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let queue = WriteQueue::new(Box::new(FirstWriteBarrier {
        data: data.clone(),
        first_entered: entered_tx,
        release_first: release_rx,
        first: true,
    }));
    queue.send(b"first".to_vec()).unwrap();
    entered_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("first queue request must block at the writer");

    let gate = GuardedInputGate::default();
    gate.rearm();
    let permit = gate.issue().unwrap();
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    queue
        .enqueue(
            vec![b"envelope".to_vec()],
            Some(permit),
            Some(done_tx),
            || {},
        )
        .unwrap();
    gate.invalidate();
    release_tx.send(()).unwrap();

    assert_eq!(
        done_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        Err(STALE_AGENT_INPUT.into())
    );
    queue
        .send_and_wait(b"after".to_vec(), Duration::from_secs(1))
        .expect("stale guarded request must not close the write queue");
    assert_eq!(&*data.lock().unwrap(), b"firstafter");
}

#[test]
fn held_write_bounds_request_admission_before_enqueue() {
    let data = Arc::new(Mutex::new(Vec::new()));
    let (entered, observed) = mpsc::sync_channel(1);
    let (release, held) = mpsc::sync_channel(1);
    let queue = WriteQueue::new(Box::new(FirstWriteBarrier {
        data,
        first_entered: entered,
        release_first: held,
        first: true,
    }));
    queue.send(vec![1]).unwrap();
    observed.recv_timeout(Duration::from_secs(1)).unwrap();
    for _ in 1..MAX_REQUESTS {
        queue.send(Vec::new()).unwrap();
    }
    let callback = std::cell::Cell::new(false);
    let error = queue
        .send_and_wait_with_enqueued(vec![2], Duration::ZERO, || callback.set(true))
        .unwrap_err();
    assert!(is_write_queue_full(&error));
    assert!(!callback.get());
    assert_eq!(queue.shared.state.lock().requests, MAX_REQUESTS);
    queue.close();
    assert_eq!(queue.shared.state.lock().requests, 1);
    assert_eq!(queue.send(vec![3]), Err(WRITE_QUEUE_CLOSED.into()));
    release.send(()).unwrap();
}

#[test]
fn held_write_bounds_bytes_and_frames_before_enqueue() {
    let (entered, observed) = mpsc::sync_channel(1);
    let (release, held) = mpsc::sync_channel(1);
    let queue = WriteQueue::new(Box::new(FirstWriteBarrier {
        data: Arc::default(),
        first_entered: entered,
        release_first: held,
        first: true,
    }));
    queue.send(vec![1]).unwrap();
    observed.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(is_write_queue_full(
        &queue
            .send_frames_and_wait(vec![vec![]; MAX_FRAMES + 1], Duration::ZERO)
            .unwrap_err()
    ));
    let frame_storage = std::mem::size_of::<Vec<u8>>();
    queue
        .send(vec![0; MAX_BYTES - 2 * frame_storage - 1])
        .unwrap();
    assert!(is_write_queue_full(&queue.send(vec![2]).unwrap_err()));
    assert_eq!(queue.shared.state.lock().bytes, MAX_BYTES);
    queue.close();
    assert_eq!(queue.shared.state.lock().bytes, frame_storage + 1);
    release.send(()).unwrap();
}

struct CancelledWriter {
    entered: mpsc::SyncSender<()>,
    cancelled: mpsc::Receiver<()>,
}

impl Write for CancelledWriter {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        self.entered.send(()).unwrap();
        self.cancelled.recv_timeout(Duration::from_secs(2)).unwrap();
        Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "cancelled",
        ))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn cancellation_releases_a_write_held_authority_gate_without_replaying_pending_input() {
    let gate = GuardedInputGate::default();
    gate.rearm();
    let permit = gate.issue().unwrap();
    let (entered, observed) = mpsc::sync_channel(1);
    let (cancel, cancelled) = mpsc::sync_channel(1);
    let queue = Arc::new(WriteQueue::new_cancellable(
        portable_pty::CancellableWriter {
            writer: Box::new(CancelledWriter { entered, cancelled }),
            cancel: Arc::new(move || {
                let _ = cancel.try_send(());
            }),
        },
    ));
    let active = queue.clone();
    let writer = std::thread::spawn(move || {
        active.send_and_wait_guarded(vec![1], permit, Duration::from_secs(2))
    });
    observed.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(gate.state.writer.try_lock().is_err());
    assert!(gate.issue().is_some());
    let (completion, result) = mpsc::sync_channel(1);
    queue
        .enqueue(vec![vec![2]], None, Some(completion), || {})
        .unwrap();
    let other = WriteQueue::new(Box::new(std::io::sink()));
    other
        .send_and_wait(vec![3], Duration::from_secs(1))
        .unwrap();
    queue.close();
    assert_eq!(
        result.recv_timeout(Duration::from_secs(1)).unwrap(),
        Err(WRITE_CANCELLED.into())
    );
    gate.invalidate();
    assert!(gate.issue().is_none());
    assert!(writer.join().unwrap().is_err());
    assert!(matches!(
        observed.try_recv(),
        Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected)
    ));
}

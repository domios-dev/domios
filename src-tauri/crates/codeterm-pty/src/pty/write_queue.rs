use std::io::Write;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const STALE_AGENT_INPUT: &str = "stale agent input permit";
const WRITE_ACK_TIMEOUT: &str = "PTY write acknowledgement timed out";

pub fn is_stale_agent_input(error: &str) -> bool {
    error == STALE_AGENT_INPUT
}

/// The request was enqueued on the serialized writer but the caller's bounded
/// ack wait expired first — the bytes may still reach the PTY. Never a proof
/// that the PTY is closed or that nothing was written.
pub fn is_write_ack_timeout(error: &str) -> bool {
    error == WRITE_ACK_TIMEOUT
}

#[derive(Debug)]
struct GuardedInputState {
    epoch: u64,
    accepting: bool,
}

#[derive(Clone, Debug)]
pub struct GuardedInputPermit {
    epoch: u64,
    gate: Arc<Mutex<GuardedInputState>>,
}

#[derive(Debug)]
pub struct GuardedInputGate {
    state: Arc<Mutex<GuardedInputState>>,
}

impl Default for GuardedInputGate {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(GuardedInputState {
                epoch: 0,
                accepting: false,
            })),
        }
    }
}

impl GuardedInputGate {
    pub fn issue(&self) -> Option<GuardedInputPermit> {
        let state = self.state.lock().ok()?;
        state.accepting.then(|| GuardedInputPermit {
            epoch: state.epoch,
            gate: self.state.clone(),
        })
    }

    pub fn rearm(&self) {
        if let Ok(mut state) = self.state.lock() {
            if !state.accepting {
                state.epoch = state.epoch.wrapping_add(1);
                state.accepting = true;
            }
        }
    }

    pub fn invalidate(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.epoch = state.epoch.wrapping_add(1);
            state.accepting = false;
        }
    }
}

pub struct WriteRequest {
    pub chunks: Vec<Vec<u8>>,
    permit: Option<GuardedInputPermit>,
    completed: Option<mpsc::SyncSender<Result<(), String>>>,
}

pub struct WriteQueue {
    sender: Mutex<Option<mpsc::Sender<WriteRequest>>>,
}

impl WriteQueue {
    pub fn new(mut writer: Box<dyn Write + Send>) -> Self {
        let (sender, receiver) = mpsc::channel::<WriteRequest>();

        std::thread::spawn(move || {
            while let Ok(req) = receiver.recv() {
                let result = match req.permit {
                    Some(permit) => match permit.gate.lock() {
                        Ok(state) if state.accepting && state.epoch == permit.epoch => {
                            write_chunks(&mut writer, &req.chunks)
                        }
                        Ok(_) => Err(STALE_AGENT_INPUT.to_string()),
                        Err(error) => Err(error.to_string()),
                    },
                    None => write_chunks(&mut writer, &req.chunks),
                };
                if let Some(completed) = req.completed {
                    let _ = completed.send(result.clone());
                }
                if let Err(error) = result {
                    if error == STALE_AGENT_INPUT {
                        log::debug!("Dropped PTY write after agent input permit was revoked");
                        continue;
                    }
                    log::warn!("PTY write failed: {error}");
                    break;
                }
            }
        });

        Self {
            sender: Mutex::new(Some(sender)),
        }
    }

    pub fn send(&self, data: Vec<u8>) -> Result<(), String> {
        let sender = self.sender.lock().map_err(|e| e.to_string())?;
        sender
            .as_ref()
            .ok_or_else(|| "PTY write channel closed".to_string())?
            .send(WriteRequest {
                chunks: vec![data],
                permit: None,
                completed: None,
            })
            .map_err(|_| "PTY write channel closed".to_string())
    }

    pub fn send_and_wait(&self, data: Vec<u8>, timeout: Duration) -> Result<(), String> {
        self.send_batch_and_wait(vec![data], timeout)
    }

    pub fn send_batch_and_wait(
        &self,
        chunks: Vec<Vec<u8>>,
        timeout: Duration,
    ) -> Result<(), String> {
        let (completed, receiver) = mpsc::sync_channel(0);
        let sender = self.sender.lock().map_err(|error| error.to_string())?;
        sender
            .as_ref()
            .ok_or_else(|| "PTY write channel closed".to_string())?
            .send(WriteRequest {
                chunks,
                permit: None,
                completed: Some(completed),
            })
            .map_err(|_| "PTY write channel closed".to_string())?;
        receiver
            .recv_timeout(timeout)
            .map_err(|_| WRITE_ACK_TIMEOUT.to_string())?
    }

    pub fn send_and_wait_guarded(
        &self,
        data: Vec<u8>,
        permit: GuardedInputPermit,
        timeout: Duration,
    ) -> Result<(), String> {
        self.send_batch_and_wait_guarded(vec![data], permit, timeout)
    }

    pub fn send_batch_and_wait_guarded(
        &self,
        chunks: Vec<Vec<u8>>,
        permit: GuardedInputPermit,
        timeout: Duration,
    ) -> Result<(), String> {
        let (completed, receiver) = mpsc::sync_channel(0);
        let sender = self.sender.lock().map_err(|error| error.to_string())?;
        sender
            .as_ref()
            .ok_or_else(|| "PTY write channel closed".to_string())?
            .send(WriteRequest {
                chunks,
                permit: Some(permit),
                completed: Some(completed),
            })
            .map_err(|_| "PTY write channel closed".to_string())?;
        receiver
            .recv_timeout(timeout)
            .map_err(|_| WRITE_ACK_TIMEOUT.to_string())?
    }

    pub fn close(&self) {
        if let Ok(mut sender) = self.sender.lock() {
            let _ = sender.take();
        }
    }
}

fn write_chunks(writer: &mut Box<dyn Write + Send>, chunks: &[Vec<u8>]) -> Result<(), String> {
    for chunk in chunks {
        writer
            .write_all(chunk)
            .and_then(|()| writer.flush())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct MockWriter {
        data: Arc<Mutex<Vec<u8>>>,
    }

    struct ChunkWriter {
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
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

    impl Write for ChunkWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.writes.lock().unwrap().push(buf.to_vec());
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn batch_keeps_paste_and_enter_adjacent_but_separate() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let queue = WriteQueue::new(Box::new(ChunkWriter {
            writes: writes.clone(),
        }));

        queue
            .send_batch_and_wait(
                vec![b"\x1b[200~message\x1b[201~".to_vec(), b"\r".to_vec()],
                Duration::from_secs(1),
            )
            .unwrap();

        assert_eq!(
            &*writes.lock().unwrap(),
            &[b"\x1b[200~message\x1b[201~".to_vec(), b"\r".to_vec()]
        );
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
                self.release_first.recv().unwrap();
            }
            self.data.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
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

        drop(queue);
        std::thread::sleep(std::time::Duration::from_millis(50));

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
        drop(queue);
        std::thread::sleep(std::time::Duration::from_millis(50));

        let data = collected.lock().unwrap();
        let expected: Vec<u8> = (0..10).collect();
        assert_eq!(&*data, &expected);
    }

    #[test]
    fn send_after_close_returns_error() {
        let collected = Arc::new(Mutex::new(Vec::new()));
        let _writer = MockWriter {
            data: collected.clone(),
        };
        let (sender, _receiver) = mpsc::channel::<WriteRequest>();
        let queue = WriteQueue {
            sender: Mutex::new(Some(sender)),
        };
        drop(_receiver);

        std::thread::sleep(std::time::Duration::from_millis(10));

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
            .sender
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send(WriteRequest {
                chunks: vec![b"envelope".to_vec()],
                permit: Some(permit),
                completed: Some(done_tx),
            })
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
}

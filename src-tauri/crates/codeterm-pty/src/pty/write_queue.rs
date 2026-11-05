use parking_lot::{Condvar, Mutex as QueueMutex};
use std::collections::VecDeque;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    epoch: AtomicU64,
    retired: AtomicBool,
    writer: Mutex<()>,
}

#[derive(Clone, Debug)]
pub struct GuardedInputPermit {
    epoch: u64,
    gate: Arc<GuardedInputState>,
}

#[derive(Clone, Debug)]
pub struct GuardedInputGate {
    state: Arc<GuardedInputState>,
}

impl Default for GuardedInputGate {
    fn default() -> Self {
        Self {
            state: Arc::new(GuardedInputState {
                epoch: AtomicU64::new(0),
                retired: AtomicBool::new(false),
                writer: Mutex::new(()),
            }),
        }
    }
}

impl GuardedInputGate {
    pub fn issue(&self) -> Option<GuardedInputPermit> {
        let epoch = self.state.epoch.load(Ordering::Acquire);
        (epoch & 1 != 0 && !self.state.retired.load(Ordering::Acquire)).then(|| {
            GuardedInputPermit {
                epoch,
                gate: self.state.clone(),
            }
        })
    }

    pub fn rearm(&self) {
        if self.state.retired.load(Ordering::Acquire) {
            return;
        }
        let _ = self
            .state
            .epoch
            .try_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
                (epoch & 1 == 0).then(|| epoch.wrapping_add(1))
            });
    }

    pub(crate) fn retire(&self) {
        self.state.retired.store(true, Ordering::Release);
    }

    pub fn invalidate(&self) {
        if let Ok(_writer) = self.state.writer.lock() {
            let _ = self
                .state
                .epoch
                .try_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
                    Some(epoch.wrapping_add(if epoch & 1 == 0 { 2 } else { 1 }))
                });
        }
    }
}

const WRITE_QUEUE_FULL: &str = "tab_busy_refused: PTY write queue capacity refused before enqueue";
const WRITE_CANCELLED: &str = "PTY write cancelled after enqueue";
const WRITE_QUEUE_CLOSED: &str = "PTY write channel closed";
const MAX_REQUESTS: usize = 64;
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_FRAMES: usize = 64;

pub fn is_write_queue_full(error: &str) -> bool {
    error == WRITE_QUEUE_FULL
}

struct WriteRequest {
    frames: Vec<Vec<u8>>,
    bytes: usize,
    permit: Option<GuardedInputPermit>,
    completed: Option<mpsc::SyncSender<Result<(), String>>>,
}

#[derive(Default)]
struct QueueState {
    pending: VecDeque<WriteRequest>,
    closed: bool,
    requests: usize,
    bytes: usize,
}

#[derive(Default)]
struct Shared {
    state: QueueMutex<QueueState>,
    changed: Condvar,
}

impl Shared {
    fn close(&self) {
        let pending = {
            let mut state = self.state.lock();
            state.closed = true;
            let pending = std::mem::take(&mut state.pending);
            for request in &pending {
                state.requests -= 1;
                state.bytes -= request.bytes;
            }
            self.changed.notify_all();
            pending
        };
        for request in pending {
            if let Some(completed) = request.completed {
                let _ = completed.send(Err(WRITE_CANCELLED.into()));
            }
        }
    }
}

pub struct WriteQueue {
    shared: Arc<Shared>,
    cancel: Arc<dyn Fn() + Send + Sync>,
}

impl WriteQueue {
    pub fn new(writer: Box<dyn Write + Send>) -> Self {
        Self::new_cancellable(portable_pty::CancellableWriter {
            writer,
            cancel: Arc::new(|| {}),
        })
    }

    pub fn new_cancellable(writer: portable_pty::CancellableWriter) -> Self {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        let portable_pty::CancellableWriter { mut writer, cancel } = writer;
        codeterm_util::named_thread::spawn("pty-write", move || loop {
            let request = {
                let mut state = worker.state.lock();
                while state.pending.is_empty() && !state.closed {
                    worker.changed.wait(&mut state);
                }
                let Some(request) = state.pending.pop_front() else {
                    break;
                };
                request
            };
            let write = |writer: &mut Box<dyn Write + Send>| {
                request
                    .frames
                    .iter()
                    .try_for_each(|frame| writer.write_all(frame).and_then(|()| writer.flush()))
                    .map_err(|error| error.to_string())
            };
            let result = match request.permit.as_ref() {
                Some(permit) => match permit.gate.writer.lock() {
                    Ok(_guard) if permit.gate.epoch.load(Ordering::Acquire) == permit.epoch => {
                        write(&mut writer)
                    }
                    Ok(_) => Err(STALE_AGENT_INPUT.into()),
                    Err(error) => Err(error.to_string()),
                },
                None => write(&mut writer),
            };
            {
                let mut state = worker.state.lock();
                state.requests -= 1;
                state.bytes -= request.bytes;
            }
            if let Some(completed) = request.completed {
                let _ = completed.send(result.clone());
            }
            if let Err(error) = result {
                if is_stale_agent_input(&error) {
                    continue;
                }
                log::debug!("PTY writer stopped: {error}");
                worker.close();
                break;
            }
        });
        Self { shared, cancel }
    }

    fn enqueue(
        &self,
        frames: Vec<Vec<u8>>,
        permit: Option<GuardedInputPermit>,
        completed: Option<mpsc::SyncSender<Result<(), String>>>,
        on_enqueued: impl FnOnce(),
    ) -> Result<(), String> {
        let mut state = self.shared.state.lock();
        if state.closed {
            return Err(WRITE_QUEUE_CLOSED.into());
        }
        let frame_storage = frames
            .capacity()
            .checked_mul(std::mem::size_of::<Vec<u8>>())
            .ok_or(WRITE_QUEUE_FULL)?;
        let bytes = frames
            .iter()
            .try_fold(frame_storage, |sum, frame| {
                sum.checked_add(frame.capacity())
            })
            .ok_or(WRITE_QUEUE_FULL)?;
        if frames.len() > MAX_FRAMES
            || state.requests >= MAX_REQUESTS
            || bytes > MAX_BYTES.saturating_sub(state.bytes)
        {
            return Err(WRITE_QUEUE_FULL.into());
        }
        state.requests += 1;
        state.bytes += bytes;
        state.pending.push_back(WriteRequest {
            frames,
            bytes,
            permit,
            completed,
        });
        on_enqueued();
        self.shared.changed.notify_one();
        Ok(())
    }

    pub fn send(&self, data: Vec<u8>) -> Result<(), String> {
        self.enqueue(vec![data], None, None, || {})
    }

    pub fn send_and_wait(&self, data: Vec<u8>, timeout: Duration) -> Result<(), String> {
        self.send_frames_and_wait(vec![data], timeout)
    }

    pub fn send_and_wait_with_enqueued(
        &self,
        data: Vec<u8>,
        timeout: Duration,
        on_enqueued: impl FnOnce(),
    ) -> Result<(), String> {
        self.send_frames_and_wait_with_enqueued(vec![data], timeout, on_enqueued)
    }

    pub fn send_frames_and_wait(
        &self,
        frames: Vec<Vec<u8>>,
        timeout: Duration,
    ) -> Result<(), String> {
        self.send_frames_and_wait_with_enqueued(frames, timeout, || {})
    }

    pub fn send_frames_and_wait_with_enqueued(
        &self,
        frames: Vec<Vec<u8>>,
        timeout: Duration,
        on_enqueued: impl FnOnce(),
    ) -> Result<(), String> {
        self.enqueue_and_wait(frames, None, timeout, on_enqueued)
    }

    pub fn send_and_wait_guarded(
        &self,
        data: Vec<u8>,
        permit: GuardedInputPermit,
        timeout: Duration,
    ) -> Result<(), String> {
        self.send_frames_and_wait_guarded(vec![data], permit, timeout)
    }

    pub fn send_and_wait_guarded_with_enqueued(
        &self,
        data: Vec<u8>,
        permit: GuardedInputPermit,
        timeout: Duration,
        on_enqueued: impl FnOnce(),
    ) -> Result<(), String> {
        self.send_frames_and_wait_guarded_with_enqueued(vec![data], permit, timeout, on_enqueued)
    }

    pub fn send_frames_and_wait_guarded(
        &self,
        frames: Vec<Vec<u8>>,
        permit: GuardedInputPermit,
        timeout: Duration,
    ) -> Result<(), String> {
        self.send_frames_and_wait_guarded_with_enqueued(frames, permit, timeout, || {})
    }

    pub fn send_frames_and_wait_guarded_with_enqueued(
        &self,
        frames: Vec<Vec<u8>>,
        permit: GuardedInputPermit,
        timeout: Duration,
        on_enqueued: impl FnOnce(),
    ) -> Result<(), String> {
        self.enqueue_and_wait(frames, Some(permit), timeout, on_enqueued)
    }

    fn enqueue_and_wait(
        &self,
        frames: Vec<Vec<u8>>,
        permit: Option<GuardedInputPermit>,
        timeout: Duration,
        on_enqueued: impl FnOnce(),
    ) -> Result<(), String> {
        let (completed, receiver) = mpsc::sync_channel(1);
        self.enqueue(frames, permit, Some(completed), on_enqueued)?;
        receiver
            .recv_timeout(timeout)
            .map_err(|_| WRITE_ACK_TIMEOUT.to_string())?
    }

    pub fn close(&self) {
        self.shared.close();
        (self.cancel)();
    }
}

impl Drop for WriteQueue {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests;

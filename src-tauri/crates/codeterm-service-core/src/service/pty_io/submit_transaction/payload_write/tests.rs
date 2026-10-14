use super::super::{
    claim_submit_cycle, runner, InjectionError, Observation, SubmitTransport, VerifyOpts,
    VerifySubmitStrategy,
};
use super::*;
use std::cell::{Cell, RefCell};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct RecordingWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for RecordingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn message_payload_constructor_requires_a_submit_reservation_and_arms_it_on_enqueue() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let queue = WriteQueue::new(Box::new(RecordingWriter(bytes.clone())));
    let claim = claim_submit_cycle("message-construction-guard");
    let (sender, _) = sync_channel(1);
    let message = MessagePayload::new(b"opaque".to_vec(), claim.clone(), sender);
    assert!(!claim.cycle.is_current(claim.generation));
    let queued = message
        .enqueue(&queue, None, Duration::from_secs(1), || {})
        .unwrap();
    assert_eq!(queued.len(), 6);
    assert!(claim.cycle.is_current(claim.generation));
    assert_eq!(bytes.lock().unwrap().as_slice(), b"opaque");
}

#[test]
fn closed_queue_refusal_cannot_take_the_written_payloads_cycle() {
    let queue = WriteQueue::new(Box::new(RecordingWriter(Arc::default())));
    let first = claim_submit_cycle("message-closed-queue");
    let (sender, _) = sync_channel(1);
    MessagePayload::new(b"first".to_vec(), first.clone(), sender)
        .enqueue(&queue, None, Duration::from_secs(1), || {})
        .unwrap();
    queue.close();
    let refused = claim_submit_cycle(first.pty_id());
    let (sender, _) = sync_channel(1);
    assert!(MessagePayload::new(b"refused".to_vec(), refused, sender)
        .enqueue(&queue, None, Duration::from_secs(1), || panic!(
            "nothing was enqueued"
        ))
        .is_err());
    assert!(first.is_current());
}

struct OccupiedWriter {
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
    occupied: std::sync::mpsc::SyncSender<()>,
    release: std::sync::mpsc::Receiver<()>,
}

impl std::io::Write for OccupiedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.writes.lock().unwrap().is_empty() {
            self.occupied.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        self.writes.lock().unwrap().push(bytes.to_vec());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn user_message_queued_behind_agent_payload_writes_once_after_release_and_reconciles() {
    use crate::service::chat_send_transaction::{ChatSendLedger, ChatSendState, SendLiveBinding};
    let writes = Arc::new(Mutex::new(Vec::new()));
    let (occupied_tx, occupied_rx) = sync_channel(1);
    let (release_tx, release_rx) = sync_channel(1);
    let queue = Arc::new(WriteQueue::new(Box::new(OccupiedWriter {
        writes: writes.clone(),
        occupied: occupied_tx,
        release: release_rx,
    })));
    let agent = claim_submit_cycle("user-agent-queue-contention");
    let agent_queue = queue.clone();
    let (sender, _) = sync_channel(1);
    let agent_thread = std::thread::spawn(move || {
        MessagePayload::new(b"agent-report".to_vec(), agent, sender)
            .enqueue(&agent_queue, None, Duration::from_secs(2), || {})
            .unwrap();
    });
    occupied_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let ledger = Arc::new(ChatSendLedger::default());
    let status = ledger
        .try_start_scoped(
            "owner",
            "user-message",
            None,
            Some(SendLiveBinding {
                session_id: Some("own".into()),
                binding_nonce: Some("launch".into()),
            }),
        )
        .unwrap();
    let user = claim_submit_cycle("user-agent-queue-contention");
    let user_queue = queue.clone();
    let (sender, _) = sync_channel(1);
    let (queued_tx, queued_rx) = sync_channel(1);
    let user_thread = std::thread::spawn(move || {
        MessagePayload::new(b"user-message".to_vec(), user, sender)
            .enqueue(&user_queue, None, Duration::from_secs(2), || {
                queued_tx.send(()).unwrap()
            })
            .unwrap();
    });
    queued_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(writes.lock().unwrap().is_empty());
    release_tx.send(()).unwrap();
    agent_thread.join().unwrap();
    user_thread.join().unwrap();
    assert_eq!(
        *writes.lock().unwrap(),
        [b"agent-report".to_vec(), b"user-message".to_vec()]
    );
    ledger.finish(
        &status.send_id,
        Err("submit_unconfirmed: awaiting own-session evidence".into()),
    );
    let delivered =
        ledger.confirm_delivered_user_turns_scoped("owner", "own", Some("launch"), "user-message");
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].send_id, status.send_id);
    assert_eq!(delivered[0].state, ChatSendState::Sent);
}

#[test]
fn revoked_guard_after_enqueue_is_uncertainty_and_never_a_zero_write_refusal() {
    use crate::pty::write_queue::GuardedInputGate;
    let queue = WriteQueue::new(Box::new(RecordingWriter(Arc::default())));
    let gate = GuardedInputGate::default();
    gate.rearm();
    let permit = gate.issue().unwrap();
    gate.invalidate();
    let claim = claim_submit_cycle("queued-revoked-guard");
    let (sender, _) = sync_channel(1);
    let result = MessagePayload::new(b"opaque".to_vec(), claim.clone(), sender).enqueue(
        &queue,
        Some(permit),
        Duration::from_secs(1),
        || {},
    );
    assert!(matches!(result, Err(PayloadWriteFailure::Queued(_))));
    assert!(claim.cycle.is_current(claim.generation));
}

struct AckTimeoutInput<'a> {
    queue: &'a WriteQueue,
    claim: SubmitCycleClaim,
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
    occupied: std::sync::mpsc::Receiver<()>,
    release: RefCell<Option<std::sync::mpsc::SyncSender<()>>>,
    keys: Cell<usize>,
}

impl SubmitTransport for AckTimeoutInput<'_> {
    fn observe(&self) -> Option<Observation> {
        None
    }
    fn draft_evidence_count(&self, _: &str) -> usize {
        self.writes.lock().unwrap().len()
    }
    fn generation(&self) -> u64 {
        0
    }
    fn wait_change(&self, _: u64, _: Instant) -> bool {
        false
    }
    fn turn_started(&self) -> bool {
        false
    }
    fn exact_user_turn_present(&self, text: &str) -> bool {
        self.keys.get() > 0
            && self
                .writes
                .lock()
                .unwrap()
                .first()
                .is_some_and(|payload| payload.as_slice() == text.as_bytes())
    }
    fn retry_active(&self) -> bool {
        self.claim.is_current()
    }
    fn write_paste(&self, text: &str, _: Instant) -> Result<usize, InjectionError> {
        let (sender, _) = sync_channel(1);
        let queued = MessagePayload::new(text.as_bytes().to_vec(), self.claim.clone(), sender)
            .enqueue(self.queue, None, Duration::ZERO, || {})
            .unwrap();
        self.occupied.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(self.claim.cycle.is_current(self.claim.generation));
        assert!(self.writes.lock().unwrap().is_empty());
        Ok(queued.len())
    }
    fn write_submit_key(&self, key: &[u8], deadline: Instant) -> Result<(), InjectionError> {
        assert_eq!(key, b"\r");
        self.release.borrow_mut().take().unwrap().send(()).unwrap();
        self.queue
            .send_and_wait(
                key.to_vec(),
                deadline.saturating_duration_since(Instant::now()),
            )
            .unwrap();
        self.keys.set(self.keys.get() + 1);
        Ok(())
    }
}

#[test]
fn queued_ack_timeout_keeps_the_payload_armed_for_its_enter_schedule() {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let (occupied_tx, occupied_rx) = sync_channel(1);
    let (release_tx, release_rx) = sync_channel(1);
    let queue = WriteQueue::new(Box::new(OccupiedWriter {
        writes: writes.clone(),
        occupied: occupied_tx,
        release: release_rx,
    }));
    let transport = AckTimeoutInput {
        queue: &queue,
        claim: claim_submit_cycle("queued-ack-timeout-remains-armed"),
        writes: writes.clone(),
        occupied: occupied_rx,
        release: RefCell::new(Some(release_tx)),
        keys: Cell::new(0),
    };
    let result = runner::run(
        &transport,
        "opaque",
        true,
        VerifySubmitStrategy::TuiInputDrained {
            initial_key_delay: Duration::ZERO,
            retry_offsets: [Duration::ZERO; 6],
        },
        VerifyOpts {
            require_prompt_first: false,
            verify_echo: false,
            verify_submit: true,
            submit_timeout: Duration::from_secs(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.bytes_written, 6);
    assert_eq!(result.submit_key_writes, 1);
    assert!(result.submit_observed);
    assert_eq!(
        *writes.lock().unwrap(),
        [b"opaque".to_vec(), b"\r".to_vec()]
    );
}

#[test]
fn payload_only_message_construction_is_not_part_of_the_write_door() {
    let _: fn(Vec<u8>, SubmitCycleClaim, SyncSender<SubmitCycleSignal>) -> MessagePayload =
        MessagePayload::new;
}

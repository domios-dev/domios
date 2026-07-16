use super::lifecycle::restore_materialization_core;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};
use crate::service::boot_stage::{BootStage, BootStageSink};
use crate::service::launch_delivery::{
    begin_fallback_attempt, confirm_fallback, dispatch_initial_delivery, next_attempt,
    DeliveryError, LaunchDeliveryMode,
};
use crate::service::pending_delivery_payloads::PendingDeliveryPayloadStore;
use codeterm_util::lock_hold::TimedRwLock;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

#[derive(Default)]
struct ReplayRecording {
    stages: Mutex<Vec<BootStage>>,
    delivered_payloads: Mutex<Vec<String>>,
    delivery_writes: AtomicUsize,
}

impl BootStageSink for ReplayRecording {
    fn record(&self, _: &str, stage: BootStage, _: Option<&str>) {
        self.stages.lock().push(stage);
    }
}

struct RestoreReplayFixture {
    _temp: tempfile::TempDir,
    mux: MuxState,
    pane_id: String,
    store: Arc<PendingDeliveryPayloadStore>,
    scheduled_payload: Arc<Mutex<Option<Option<String>>>>,
    recording: Arc<ReplayRecording>,
}

impl RestoreReplayFixture {
    fn restored(confirmed: bool) -> Self {
        let payload = "byte-identical restored launch task";
        let digest = format!("{:x}", Sha256::digest(payload.as_bytes()));
        let temp = tempfile::tempdir().unwrap();
        let store =
            Arc::new(PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap());
        store.write(&digest, payload).unwrap();

        let (mut state, pane_id) = MultiplexerState::new();
        let pane = state.get_pane_mut(&pane_id).unwrap();
        pane.agent_type = Some("fixture-provider".into());
        pane.cwd = Some("fixture-cwd".into());
        pane.pending_delivery = Some(PendingDeliveryRecord::new(
            digest,
            PendingDeliveryChannel::InteractiveInput,
        ));
        if confirmed {
            pane.pending_delivery.as_mut().unwrap().confirm().unwrap();
        }
        let snapshot = state.to_session_snapshot();
        let restored = MultiplexerState::from_session_snapshot(&snapshot);

        Self {
            _temp: temp,
            mux: Arc::new(TimedRwLock::new("restore-replay-test", restored)),
            pane_id,
            store,
            scheduled_payload: Arc::new(Mutex::new(None)),
            recording: Arc::new(ReplayRecording::default()),
        }
    }

    fn restore(&self) -> Result<bool, String> {
        let pending = self
            .mux
            .read()
            .get_pane(&self.pane_id)
            .and_then(|pane| pane.pending_delivery.clone());
        let store = self.store.clone();
        let scheduled_payload = self.scheduled_payload.clone();
        let recording = self.recording.clone();
        let result = restore_materialization_core(
            &self.mux,
            &self.pane_id,
            "fixture-provider",
            None,
            Some("fixture-cwd"),
            false,
            move || match pending {
                Some(record)
                    if record.state() == PendingDeliveryState::Pending
                        && record.channel == PendingDeliveryChannel::InteractiveInput =>
                {
                    store
                        .resolve(&record.content_digest)
                        .map(Some)
                        .map_err(|error| error.to_string())
                }
                _ => Ok(None),
            },
            move |_| recording.record("fixture-pane", BootStage::SpawnFailed, None),
            move |_, payload| *scheduled_payload.lock() = Some(payload),
        )?;
        Ok(result.is_some())
    }

    fn record_stage(&self, stage: BootStage) {
        self.recording.record(&self.pane_id, stage, None);
    }

    fn consume_replay(&self, submission_observed: bool) -> Result<(), DeliveryError> {
        self.consume_replay_with(|| submission_observed)
    }

    fn consume_replay_with(&self, submission: impl FnOnce() -> bool) -> Result<(), DeliveryError> {
        let Some(payload) = self.scheduled_payload.lock().take().flatten() else {
            return Ok(());
        };
        let should_submit = {
            let mut mux = self.mux.write();
            let pending = mux
                .get_pane_mut(&self.pane_id)
                .and_then(|pane| pane.pending_delivery.as_mut())
                .expect("restored replay owns its pending record");
            if pending.state() == PendingDeliveryState::Confirmed {
                false
            } else {
                begin_fallback_attempt(pending, next_attempt()).is_ok()
            }
        };
        if !should_submit {
            return Ok(());
        }

        let observed = dispatch_initial_delivery(
            LaunchDeliveryMode::ConfirmedFallback,
            self.recording.as_ref(),
            &self.pane_id,
            Some(|| {
                self.recording.delivered_payloads.lock().push(payload);
                self.recording
                    .delivery_writes
                    .fetch_add(1, Ordering::SeqCst);
                submission()
            }),
        )
        .expect("fallback delivery dispatches its transaction");
        let mut mux = self.mux.write();
        let pending = mux
            .get_pane_mut(&self.pane_id)
            .and_then(|pane| pane.pending_delivery.as_mut())
            .expect("restored replay retains its pending record");
        confirm_fallback(pending, observed)
    }

    fn delivery_writes(&self) -> usize {
        self.recording.delivery_writes.load(Ordering::SeqCst)
    }

    fn delivered_payloads(&self) -> Vec<String> {
        self.recording.delivered_payloads.lock().clone()
    }

    fn pending_state(&self) -> PendingDeliveryState {
        self.mux
            .read()
            .get_pane(&self.pane_id)
            .and_then(|pane| pane.pending_delivery.as_ref())
            .map(PendingDeliveryRecord::state)
            .unwrap()
    }
}

#[test]
fn restore_replays_pending_exactly_once() {
    let fixture = RestoreReplayFixture::restored(false);

    assert!(fixture.restore().unwrap());
    fixture.record_stage(BootStage::PtyStarting);
    fixture.record_stage(BootStage::ShellReady);
    fixture.record_stage(BootStage::AgentLaunching);
    fixture.consume_replay(true).unwrap();
    assert!(!fixture.restore().unwrap());

    assert_eq!(fixture.delivery_writes(), 1);
    assert_eq!(
        fixture.delivered_payloads(),
        ["byte-identical restored launch task"]
    );
    assert_eq!(fixture.pending_state(), PendingDeliveryState::Confirmed);
}

#[test]
fn restore_confirmed_never_redelivers() {
    let fixture = RestoreReplayFixture::restored(true);

    assert!(fixture.restore().unwrap());
    fixture.record_stage(BootStage::PtyStarting);
    fixture.record_stage(BootStage::ShellReady);
    fixture.record_stage(BootStage::AgentLaunching);
    fixture.consume_replay(true).unwrap();
    assert!(!fixture.restore().unwrap());

    assert_eq!(fixture.delivery_writes(), 0);
    assert!(fixture.delivered_payloads().is_empty());
    assert_eq!(fixture.pending_state(), PendingDeliveryState::Confirmed);
}

#[test]
fn duplicate_restore_single_replay() {
    let fixture = Arc::new(RestoreReplayFixture::restored(false));
    let barrier = Arc::new(Barrier::new(3));
    let attempts = (0..2)
        .map(|_| {
            let fixture = fixture.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                fixture.restore().unwrap()
            })
        })
        .collect::<Vec<_>>();

    barrier.wait();
    let winners = attempts
        .into_iter()
        .map(|attempt| attempt.join().unwrap())
        .filter(|won| *won)
        .count();
    fixture.consume_replay(true).unwrap();

    assert_eq!(winners, 1);
    assert_eq!(fixture.delivery_writes(), 1);
    assert_eq!(fixture.pending_state(), PendingDeliveryState::Confirmed);
}

#[test]
fn relink_racing_consumption_no_double_delivery() {
    let fixture = Arc::new(RestoreReplayFixture::restored(false));
    assert!(fixture.restore().unwrap());
    let barrier = Arc::new(Barrier::new(2));
    let consumer = {
        let fixture = fixture.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            fixture.consume_replay_with(|| {
                barrier.wait();
                barrier.wait();
                true
            })
        })
    };

    barrier.wait();
    assert!(!fixture.restore().unwrap());
    barrier.wait();
    consumer.join().unwrap().unwrap();
    assert!(!fixture.restore().unwrap());

    assert_eq!(fixture.delivery_writes(), 1);
    assert_eq!(fixture.pending_state(), PendingDeliveryState::Confirmed);
}

#[test]
fn replay_respects_readiness_gates() {
    let fixture = RestoreReplayFixture::restored(false);
    assert!(fixture.restore().unwrap());

    fixture.record_stage(BootStage::PtyStarting);
    assert_eq!(fixture.delivery_writes(), 0);
    fixture.record_stage(BootStage::ShellReady);
    assert_eq!(fixture.delivery_writes(), 0);
    fixture.record_stage(BootStage::AgentLaunching);
    assert_eq!(fixture.delivery_writes(), 0);
    fixture.consume_replay(true).unwrap();

    assert_eq!(fixture.delivery_writes(), 1);
    assert_eq!(
        fixture.recording.stages.lock().as_slice(),
        [
            BootStage::PtyStarting,
            BootStage::ShellReady,
            BootStage::AgentLaunching,
            BootStage::DeliveringInstructions,
        ]
    );
}

#[test]
fn replay_deadline_leaves_pending() {
    let fixture = RestoreReplayFixture::restored(false);
    assert!(fixture.restore().unwrap());
    fixture.record_stage(BootStage::AgentLaunching);

    let error = fixture.consume_replay(false).unwrap_err();

    assert_eq!(error, DeliveryError::DeliveryUnconfirmed);
    assert_eq!(fixture.delivery_writes(), 1);
    assert_eq!(fixture.pending_state(), PendingDeliveryState::Pending);
}

use super::lifecycle::restore_materialization_core;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};
use crate::service::boot_stage::{BootStage, BootStageSink};
use crate::service::launch_delivery::{
    LaunchConsumptionError, LaunchConsumptionOutcome, LaunchDeliveryGate,
};
use crate::service::pending_delivery_payloads::PendingDeliveryPayloadStore;
use crate::service::pty_io::test_support::{join, DeadlineBarrier};
use codeterm_util::lock_hold::TimedRwLock;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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
    tab_id: String,
    store: Arc<PendingDeliveryPayloadStore>,
    scheduled_payload: Arc<Mutex<Option<Option<String>>>>,
    recording: Arc<ReplayRecording>,
    delivery_gate: Mutex<LaunchDeliveryGate>,
}

impl RestoreReplayFixture {
    fn restored(confirmed: bool) -> Self {
        let payload = "byte-identical restored launch task";
        let digest = format!("{:x}", Sha256::digest(payload.as_bytes()));
        let temp = tempfile::tempdir().unwrap();
        let store =
            Arc::new(PendingDeliveryPayloadStore::open(temp.path(), &HashSet::new()).unwrap());
        store.write(&digest, payload).unwrap();

        let (mut state, tab_id) = MultiplexerState::new();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.cwd = Some("fixture-cwd".into());
        tab.pending_delivery = Some(PendingDeliveryRecord::new(
            digest,
            PendingDeliveryChannel::InteractiveInput,
        ));
        if confirmed {
            tab.pending_delivery.as_mut().unwrap().confirm().unwrap();
        }
        let snapshot = state.to_session_snapshot();
        let restored = MultiplexerState::from_session_snapshot(&snapshot);

        Self {
            _temp: temp,
            mux: Arc::new(TimedRwLock::new("restore-replay-test", restored)),
            tab_id,
            store,
            scheduled_payload: Arc::new(Mutex::new(None)),
            recording: Arc::new(ReplayRecording::default()),
            delivery_gate: Mutex::new(LaunchDeliveryGate::new()),
        }
    }

    fn restore(&self) -> Result<bool, String> {
        let pending = self
            .mux
            .read()
            .get_tab(&self.tab_id)
            .and_then(|tab| tab.pending_delivery.clone());
        let store = self.store.clone();
        let scheduled_payload = self.scheduled_payload.clone();
        let recording = self.recording.clone();
        let result = restore_materialization_core(
            &self.mux,
            &self.tab_id,
            "fixture-provider",
            None,
            Some("fixture-cwd"),
            false,
            false,
            move || match pending {
                Some(record)
                    if record.state().needs_delivery()
                        && record.channel == PendingDeliveryChannel::InteractiveInput =>
                {
                    store
                        .resolve(&record.content_digest)
                        .map(Some)
                        .map_err(|error| error.to_string())
                }
                _ => Ok(None),
            },
            move |stage, _| recording.record("fixture-tab", stage, None),
            move |_, payload| *scheduled_payload.lock() = Some(payload),
        )?;
        Ok(result.is_some())
    }

    fn mark_shell_ready(&self) {
        self.delivery_gate.lock().mark_shell_ready();
    }

    fn mark_provider_ready(&self) {
        self.delivery_gate.lock().mark_provider_ready();
    }

    fn consume_replay(
        &self,
        submission_observed: bool,
    ) -> Result<LaunchConsumptionOutcome, LaunchConsumptionError> {
        self.consume_replay_with(|| submission_observed)
    }

    fn consume_replay_with(
        &self,
        submission: impl FnOnce() -> bool,
    ) -> Result<LaunchConsumptionOutcome, LaunchConsumptionError> {
        let payload = self.scheduled_payload.lock().clone().flatten();
        let outcome = self.delivery_gate.lock().consume(
            &self.mux,
            &self.tab_id,
            self.recording.as_ref(),
            || {
                let payload = payload.expect("a delivery transaction requires restored payload");
                self.recording.delivered_payloads.lock().push(payload);
                self.recording
                    .delivery_writes
                    .fetch_add(1, Ordering::SeqCst);
                Ok(submission())
            },
        );
        if matches!(
            &outcome,
            Ok(LaunchConsumptionOutcome::AlreadyConfirmed)
                | Ok(LaunchConsumptionOutcome::Confirmed)
        ) {
            self.scheduled_payload.lock().take();
        }
        outcome
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
            .get_tab(&self.tab_id)
            .and_then(|tab| tab.pending_delivery.as_ref())
            .map(PendingDeliveryRecord::state)
            .unwrap()
    }
}

#[test]
fn restore_replays_pending_exactly_once() {
    let fixture = RestoreReplayFixture::restored(false);

    assert!(fixture.restore().unwrap());
    fixture.mark_shell_ready();
    fixture.mark_provider_ready();
    assert_eq!(
        fixture.consume_replay(true).unwrap(),
        LaunchConsumptionOutcome::Confirmed
    );
    assert!(!fixture.restore().unwrap());

    assert_eq!(fixture.delivery_writes(), 1);
    assert_eq!(
        fixture.delivered_payloads(),
        ["byte-identical restored launch task"]
    );
    assert_eq!(fixture.pending_state(), PendingDeliveryState::AcceptedWeak);
}

#[test]
fn restore_confirmed_never_redelivers() {
    let fixture = RestoreReplayFixture::restored(true);

    assert!(fixture.restore().unwrap());
    fixture.mark_shell_ready();
    fixture.mark_provider_ready();
    assert_eq!(
        fixture.consume_replay(true).unwrap(),
        LaunchConsumptionOutcome::AlreadyConfirmed
    );
    assert!(!fixture.restore().unwrap());

    assert_eq!(fixture.delivery_writes(), 0);
    assert!(fixture.delivered_payloads().is_empty());
    assert_eq!(fixture.pending_state(), PendingDeliveryState::AcceptedWeak);
}

#[test]
fn duplicate_restore_single_replay() {
    let fixture = Arc::new(RestoreReplayFixture::restored(false));
    let barrier = Arc::new(DeadlineBarrier::new(3));
    let attempts = (0..2)
        .map(|_| {
            let fixture = fixture.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait("duplicate restore worker");
                fixture.restore().unwrap()
            })
        })
        .collect::<Vec<_>>();

    barrier.wait("duplicate restore caller");
    let winners = attempts
        .into_iter()
        .map(|attempt| join(attempt, "duplicate restore attempt").unwrap())
        .filter(|won| *won)
        .count();
    fixture.mark_shell_ready();
    fixture.mark_provider_ready();
    assert_eq!(
        fixture.consume_replay(true).unwrap(),
        LaunchConsumptionOutcome::Confirmed
    );

    assert_eq!(winners, 1);
    assert_eq!(fixture.delivery_writes(), 1);
    assert_eq!(fixture.pending_state(), PendingDeliveryState::AcceptedWeak);
}

#[test]
fn relink_racing_consumption_no_double_delivery() {
    let fixture = Arc::new(RestoreReplayFixture::restored(false));
    assert!(fixture.restore().unwrap());
    fixture.mark_shell_ready();
    fixture.mark_provider_ready();
    let barrier = Arc::new(DeadlineBarrier::new(2));
    let consumer = {
        let fixture = fixture.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            fixture.consume_replay_with(|| {
                barrier.wait("replay consumption start");
                barrier.wait("replay consumption release");
                true
            })
        })
    };

    barrier.wait("relink restore start");
    assert!(!fixture.restore().unwrap());
    barrier.wait("relink restore release");
    assert_eq!(
        join(consumer, "relink replay consumer").unwrap().unwrap(),
        LaunchConsumptionOutcome::Confirmed
    );
    assert!(!fixture.restore().unwrap());

    assert_eq!(fixture.delivery_writes(), 1);
    assert_eq!(fixture.pending_state(), PendingDeliveryState::AcceptedWeak);
}

#[test]
fn replay_respects_readiness_gates() {
    let fixture = RestoreReplayFixture::restored(false);
    assert!(fixture.restore().unwrap());

    assert_eq!(
        fixture.consume_replay(true).unwrap(),
        LaunchConsumptionOutcome::NotReady
    );
    assert_eq!(fixture.delivery_writes(), 0);
    fixture.mark_shell_ready();
    assert_eq!(
        fixture.consume_replay(true).unwrap(),
        LaunchConsumptionOutcome::NotReady
    );
    assert_eq!(fixture.delivery_writes(), 0);
    fixture.mark_provider_ready();
    assert_eq!(
        fixture.consume_replay(true).unwrap(),
        LaunchConsumptionOutcome::Confirmed
    );

    assert_eq!(fixture.delivery_writes(), 1);
    assert_eq!(
        fixture.recording.stages.lock().as_slice(),
        [BootStage::DeliveringInstructions]
    );
}

#[test]
fn replay_deadline_leaves_pending() {
    let fixture = RestoreReplayFixture::restored(false);
    assert!(fixture.restore().unwrap());
    fixture.mark_shell_ready();
    fixture.mark_provider_ready();

    let error = fixture.consume_replay(false).unwrap_err();

    assert_eq!(error.delivery.code(), "submit_unconfirmed");
    assert_eq!(fixture.delivery_writes(), 1);
    assert_eq!(fixture.pending_state(), PendingDeliveryState::Pending);
}

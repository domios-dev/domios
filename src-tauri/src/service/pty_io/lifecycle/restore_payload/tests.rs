use super::{requires_pending_input, resolve_restored_payload};
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    DeliveryAttemptProvenance, PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};
use crate::service::boot_stage::{apply_boot_stage, commit_boot_stage_transition, BootStage};
use crate::service::pty_io::test_support::{join, DeadlineBarrier};
use codeterm_util::lock_hold::TimedRwLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn record(
    channel: PendingDeliveryChannel,
    state: PendingDeliveryState,
    attempted: bool,
) -> PendingDeliveryRecord {
    let mut record = PendingDeliveryRecord::new("historical-body-digest".into(), channel)
        .with_send_id("historical-send".into());
    if attempted {
        record
            .begin_attempt(DeliveryAttemptProvenance {
                attempt_id: "retired-attempt".into(),
                started_at_ms: 17,
            })
            .unwrap();
    }
    match state {
        PendingDeliveryState::Pending => {}
        PendingDeliveryState::Ingested => record.mark_ingested(),
        PendingDeliveryState::Unconfirmed => record.mark_unconfirmed(),
        PendingDeliveryState::AcceptedStrong => record.confirm_strong().unwrap(),
        PendingDeliveryState::AcceptedWeak => record.confirm().unwrap(),
    }
    record
}

struct Fixture {
    mux: MuxState,
    tab_id: String,
    loads: AtomicUsize,
    launches: AtomicUsize,
    historical_writes: AtomicUsize,
    events: parking_lot::Mutex<Vec<(BootStage, String)>>,
}

impl Fixture {
    fn new(pending: PendingDeliveryRecord, live_shell: bool) -> Self {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.agent_session_id = Some("original-used-session".into());
        tab.cwd = Some("fixture-cwd".into());
        tab.restored_from_snapshot = true;
        tab.pty_id = live_shell.then(|| "current-shell".into());
        tab.boot_stage = Some(BootStage::PtyStarting);
        tab.session_binding_nonce = Some("current-nonce".into());
        tab.reported_session_binding_nonce = Some("retired-nonce".into());
        tab.pending_delivery = Some(pending);
        Self {
            mux: Arc::new(TimedRwLock::new("restore-payload-test", mux)),
            tab_id,
            loads: AtomicUsize::new(0),
            launches: AtomicUsize::new(0),
            historical_writes: AtomicUsize::new(0),
            events: Default::default(),
        }
    }

    fn restore(&self, body_present: bool) -> Result<bool, String> {
        super::super::restore_materialization_core(
            &self.mux,
            &self.tab_id,
            "fixture-provider",
            Some("original-used-session"),
            Some("fixture-cwd"),
            true,
            || {
                let pending = self
                    .mux
                    .read()
                    .get_tab(&self.tab_id)
                    .unwrap()
                    .pending_delivery
                    .clone();
                resolve_restored_payload(pending.as_ref(), || {
                    self.loads.fetch_add(1, Ordering::SeqCst);
                    if body_present {
                        Ok(Some("historical-user-turn".into()))
                    } else {
                        Err("RestorePayloadMissing: historical-body-digest".into())
                    }
                })
            },
            |stage, reason| {
                assert!(commit_boot_stage_transition(
                    &self.mux,
                    &self.tab_id,
                    |tab| {
                        apply_boot_stage(tab, stage, Some(reason));
                        true
                    },
                    || self.events.lock().push((stage, reason.to_string())),
                ));
            },
            |launch, payload| {
                assert_eq!(launch.session_id.as_deref(), Some("original-used-session"));
                self.launches.fetch_add(1, Ordering::SeqCst);
                if payload.is_some() {
                    self.historical_writes.fetch_add(1, Ordering::SeqCst);
                }
                assert!(payload.is_none());
            },
        )
        .map(|launch| launch.is_some())
    }
}

#[test]
fn restored_payload_eligibility_respects_state_channel_and_attempt() {
    assert!(!requires_pending_input(None));
    for channel in [
        PendingDeliveryChannel::InteractiveInput,
        PendingDeliveryChannel::SystemPrompt,
        PendingDeliveryChannel::FallbackBootstrap,
    ] {
        for state in [
            PendingDeliveryState::Pending,
            PendingDeliveryState::Ingested,
            PendingDeliveryState::Unconfirmed,
            PendingDeliveryState::AcceptedStrong,
            PendingDeliveryState::AcceptedWeak,
        ] {
            for attempted in [false, true] {
                let pending = record(channel.clone(), state, attempted);
                let expected = channel == PendingDeliveryChannel::InteractiveInput
                    && state == PendingDeliveryState::Pending
                    && !attempted;
                assert_eq!(requires_pending_input(Some(&pending)), expected);
                let loads = AtomicUsize::new(0);
                let result = resolve_restored_payload(Some(&pending), || {
                    loads.fetch_add(1, Ordering::SeqCst);
                    Ok(Some("body".into()))
                });
                assert_eq!(result.is_err(), expected);
                assert_eq!(loads.load(Ordering::SeqCst), usize::from(expected));
                if !expected {
                    assert_eq!(result.unwrap(), None);
                }
            }
        }
    }
}

#[test]
fn restored_uncertain_payload_does_not_block_lazy_provider_launch() {
    for state in [
        PendingDeliveryState::Pending,
        PendingDeliveryState::Unconfirmed,
        PendingDeliveryState::Ingested,
    ] {
        for body_present in [false, true] {
            let pending = record(PendingDeliveryChannel::InteractiveInput, state, true);
            let fixture = Fixture::new(pending.clone(), true);
            assert!(fixture.restore(body_present).unwrap());
            assert!(!fixture.restore(body_present).unwrap());
            assert_eq!(fixture.loads.load(Ordering::SeqCst), 0);
            assert_eq!(fixture.launches.load(Ordering::SeqCst), 1);
            assert_eq!(fixture.historical_writes.load(Ordering::SeqCst), 0);
            assert!(fixture.events.lock().is_empty());
            let mux = fixture.mux.read();
            let tab = mux.get_tab(&fixture.tab_id).unwrap();
            assert_eq!(tab.pending_delivery.as_ref(), Some(&pending));
            assert_eq!(
                tab.agent_session_id.as_deref(),
                Some("original-used-session")
            );
            assert_eq!(tab.session_binding_nonce.as_deref(), Some("current-nonce"));
            assert!(!tab.launch_session_authenticated());
            assert!(tab.agent_booting);
            assert!(!tab.restored_from_snapshot);
        }
    }
}

#[test]
fn restored_lazy_activation_has_one_launch_claim() {
    let pending = record(
        PendingDeliveryChannel::InteractiveInput,
        PendingDeliveryState::Unconfirmed,
        true,
    );
    let fixture = Arc::new(Fixture::new(pending.clone(), true));
    let barrier = Arc::new(DeadlineBarrier::new(3));
    let attempts = (0..2)
        .map(|_| {
            let fixture = fixture.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait("restore activation");
                fixture.restore(false).unwrap()
            })
        })
        .collect::<Vec<_>>();
    barrier.wait("restore caller");
    let claims = attempts
        .into_iter()
        .map(|attempt| usize::from(join(attempt, "restore activation").unwrap()))
        .sum::<usize>();
    assert_eq!(claims, 1);
    assert_eq!(fixture.launches.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.loads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.historical_writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture
            .mux
            .read()
            .get_tab(&fixture.tab_id)
            .unwrap()
            .pending_delivery
            .as_ref(),
        Some(&pending)
    );
}

#[test]
fn restored_pending_input_failure_reaches_boot_event_seam() {
    for live_shell in [false, true] {
        for body_present in [false, true] {
            let pending = record(
                PendingDeliveryChannel::InteractiveInput,
                PendingDeliveryState::Pending,
                false,
            );
            let fixture = Fixture::new(pending.clone(), live_shell);
            let reason = fixture.restore(body_present).unwrap_err();
            assert!(reason.starts_with("restored_input_pending:"));
            assert_eq!(
                fixture.events.lock().as_slice(),
                [(BootStage::SpawnFailed, reason.clone())]
            );
            assert_eq!(fixture.launches.load(Ordering::SeqCst), 0);
            assert_eq!(fixture.historical_writes.load(Ordering::SeqCst), 0);
            let mux = fixture.mux.read();
            let tab = mux.get_tab(&fixture.tab_id).unwrap();
            assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
            assert_eq!(tab.boot_failure_reason.as_deref(), Some(reason.as_str()));
            assert_eq!(tab.pending_delivery.as_ref(), Some(&pending));
            assert_eq!(
                tab.agent_session_id.as_deref(),
                Some("original-used-session")
            );
            assert!(tab.restored_from_snapshot);
            assert!(!tab.agent_booting);
            assert_eq!(
                super::super::reattach_boot_stage(
                    tab.boot_stage,
                    tab.boot_failure_reason.as_deref()
                ),
                None
            );
        }
    }
}

#[test]
fn active_or_claimed_tab_never_resolves_historical_input() {
    for booting in [false, true] {
        let pending = record(
            PendingDeliveryChannel::InteractiveInput,
            PendingDeliveryState::Pending,
            false,
        );
        let fixture = Fixture::new(pending.clone(), true);
        {
            let mut mux = fixture.mux.write();
            let tab = mux.get_tab_mut(&fixture.tab_id).unwrap();
            tab.restored_from_snapshot = booting;
            tab.agent_booting = booting;
            tab.boot_stage = Some(BootStage::AgentReady);
        }
        assert!(!fixture.restore(false).unwrap());
        assert_eq!(fixture.loads.load(Ordering::SeqCst), 0);
        assert!(fixture.events.lock().is_empty());
        assert_eq!(
            fixture
                .mux
                .read()
                .get_tab(&fixture.tab_id)
                .unwrap()
                .boot_stage,
            Some(BootStage::AgentReady)
        );
    }
}

use super::{requires_pending_input, resolve_restored_payload};
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{
    DeliveryAttemptProvenance, PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};
use crate::service::boot_stage::BootStage;
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
    launches: AtomicUsize,
    historical_handoffs: AtomicUsize,
    events: parking_lot::Mutex<Vec<(BootStage, String)>>,
    // Concurrent restores legitimately hold mux on another thread, so only serial cases probe it.
    check_mux_free: bool,
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
            launches: AtomicUsize::new(0),
            historical_handoffs: AtomicUsize::new(0),
            events: Default::default(),
            check_mux_free: true,
        }
    }

    fn restore(&self) -> Result<bool, String> {
        super::super::restore_materialization_core(
            &self.mux,
            &self.tab_id,
            "fixture-provider",
            Some("original-used-session"),
            Some("fixture-cwd"),
            true,
            resolve_restored_payload,
            |stage, reason, _| {
                self.events.lock().push((stage, reason.to_string()));
            },
            |launch, payload| {
                assert!(
                    !self.check_mux_free || self.mux.try_write().is_some(),
                    "launch callback must not hold mux"
                );
                assert_eq!(launch.session_id.as_deref(), Some("original-used-session"));
                self.launches.fetch_add(1, Ordering::SeqCst);
                if payload.is_some() {
                    self.historical_handoffs.fetch_add(1, Ordering::SeqCst);
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
                let result = resolve_restored_payload(Some(&pending));
                assert_eq!(result.is_err(), expected);
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
        let pending = record(PendingDeliveryChannel::InteractiveInput, state, true);
        let fixture = Fixture::new(pending.clone(), true);
        assert!(fixture.restore().unwrap());
        assert!(!fixture.restore().unwrap());
        assert_eq!(fixture.launches.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.historical_handoffs.load(Ordering::SeqCst), 0);
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

#[test]
fn restored_lazy_activation_has_one_launch_claim() {
    let pending = record(
        PendingDeliveryChannel::InteractiveInput,
        PendingDeliveryState::Unconfirmed,
        true,
    );
    let mut fixture = Fixture::new(pending.clone(), true);
    fixture.check_mux_free = false;
    let fixture = Arc::new(fixture);
    let barrier = Arc::new(DeadlineBarrier::new(3));
    let attempts = (0..2)
        .map(|_| {
            let fixture = fixture.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait("restore activation");
                fixture.restore().unwrap()
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
    assert_eq!(fixture.historical_handoffs.load(Ordering::SeqCst), 0);
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
        let pending = record(
            PendingDeliveryChannel::InteractiveInput,
            PendingDeliveryState::Pending,
            false,
        );
        let fixture = Fixture::new(pending.clone(), live_shell);
        let reason = fixture.restore().unwrap_err();
        assert!(reason.starts_with("restored_input_pending:"));
        assert_eq!(
            fixture.events.lock().as_slice(),
            [(BootStage::SpawnFailed, reason.clone())]
        );
        assert_eq!(fixture.launches.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.historical_handoffs.load(Ordering::SeqCst), 0);
        let mux = fixture.mux.read();
        let tab = mux.get_tab(&fixture.tab_id).unwrap();
        assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
        assert_eq!(tab.boot_failure_reason.as_deref(), Some(reason.as_str()));
        assert!(tab.reported_session_binding_nonce.is_none());
        assert_eq!(tab.pending_delivery.as_ref(), Some(&pending));
        assert_eq!(
            tab.agent_session_id.as_deref(),
            Some("original-used-session")
        );
        assert!(tab.restored_from_snapshot);
        assert!(!tab.agent_booting);
        assert_eq!(
            super::super::reattach_boot_stage(tab.boot_stage, tab.boot_failure_reason.as_deref()),
            None
        );
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
        assert!(!fixture.restore().unwrap());
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

#[test]
fn delayed_restore_failure_cannot_retire_successor_launch() {
    for body_present in [false, true] {
        for change in [
            "launch",
            "provider",
            "session",
            "cwd",
            "epoch",
            "operation",
            "pty",
            "nonce",
            "proof",
            "pending",
            "aba",
        ] {
            let pending = record(
                PendingDeliveryChannel::InteractiveInput,
                PendingDeliveryState::Pending,
                false,
            );
            let fixture = Arc::new(Fixture::new(pending, true));
            let boundary = Arc::new(DeadlineBarrier::new(2));
            let old = {
                let fixture = fixture.clone();
                let boundary = boundary.clone();
                std::thread::spawn(move || {
                    super::super::restore_materialization_core(
                        &fixture.mux,
                        &fixture.tab_id,
                        "fixture-provider",
                        Some("original-used-session"),
                        Some("fixture-cwd"),
                        true,
                        |captured| {
                            boundary.wait("old restore observed");
                            boundary.wait("old restore released");
                            let delayed_lookup = if body_present {
                                Ok(Some("old successful body".to_string()))
                            } else {
                                Err("old body lookup failed".to_string())
                            };
                            delayed_lookup.and_then(|_| resolve_restored_payload(captured))
                        },
                        |stage, reason, _| fixture.events.lock().push((stage, reason.into())),
                        |_, _| {
                            fixture.launches.fetch_add(1, Ordering::SeqCst);
                        },
                    )
                    .map(|launch| launch.is_some())
                })
            };
            boundary.wait("successor installed after observation");
            let expected = {
                let mut mux = fixture.mux.write();
                let tab = mux.get_tab_mut(&fixture.tab_id).unwrap();
                match change {
                    "launch" => {
                        tab.lifecycle_epoch = Some(10);
                        tab.pty_id = Some("successor-pty".into());
                        tab.session_binding_nonce = Some("successor-nonce".into());
                        tab.reported_session_binding_nonce = Some("successor-nonce".into());
                        tab.restored_from_snapshot = false;
                        tab.agent_booting = false;
                        tab.boot_stage = Some(BootStage::AgentReady);
                        assert!(tab.launch_session_authenticated());
                    }
                    "provider" => tab.agent_type = Some("successor-provider".into()),
                    "session" => tab.agent_session_id = Some("successor-session".into()),
                    "cwd" => tab.cwd = Some("successor-cwd".into()),
                    "epoch" => tab.lifecycle_epoch = Some(10),
                    "operation" => tab.lifecycle_operation_id = Some("successor-operation".into()),
                    "pty" => tab.pty_id = Some("successor-pty".into()),
                    "nonce" => tab.session_binding_nonce = Some("successor-nonce".into()),
                    "proof" => {
                        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone()
                    }
                    "pending" => {
                        tab.pending_delivery = Some(record(
                            PendingDeliveryChannel::InteractiveInput,
                            PendingDeliveryState::Unconfirmed,
                            true,
                        ))
                    }
                    "aba" => {
                        tab.restored_from_snapshot = false;
                        tab.agent_booting = true;
                        tab.lifecycle_epoch = Some(10);
                        tab.restored_from_snapshot = true;
                        tab.agent_booting = false;
                    }
                    _ => unreachable!(),
                }
                serde_json::to_value(tab).unwrap()
            };
            boundary.wait("release stale restore");
            assert!(!join(old, "stale restore").unwrap().unwrap());
            assert!(fixture.events.lock().is_empty(), "stale event for {change}");
            assert_eq!(fixture.launches.load(Ordering::SeqCst), 0);
            assert_eq!(
                serde_json::to_value(fixture.mux.read().get_tab(&fixture.tab_id).unwrap()).unwrap(),
                expected
            );
        }
    }
}

#[test]
fn changed_pending_record_cannot_be_loaded_or_claimed_by_old_restore() {
    for replacement in [false, true] {
        let pending = record(
            PendingDeliveryChannel::InteractiveInput,
            PendingDeliveryState::Pending,
            false,
        );
        let fixture = Arc::new(Fixture::new(pending.clone(), true));
        let boundary = Arc::new(DeadlineBarrier::new(2));
        let old = {
            let fixture = fixture.clone();
            let boundary = boundary.clone();
            std::thread::spawn(move || {
                super::super::restore_materialization_core(
                    &fixture.mux,
                    &fixture.tab_id,
                    "fixture-provider",
                    Some("original-used-session"),
                    Some("fixture-cwd"),
                    true,
                    |captured| {
                        assert_eq!(captured, Some(&pending));
                        boundary.wait("pending captured");
                        boundary.wait("pending changed before classification");
                        resolve_restored_payload(captured)
                    },
                    |stage, reason, _| fixture.events.lock().push((stage, reason.into())),
                    |_, _| panic!("superseded pending must not launch"),
                )
                .map(|launch| launch.is_some())
            })
        };
        boundary.wait("replace pending");
        let expected = {
            let mut mux = fixture.mux.write();
            let tab = mux.get_tab_mut(&fixture.tab_id).unwrap();
            if replacement {
                let mut uncertain = record(
                    PendingDeliveryChannel::InteractiveInput,
                    PendingDeliveryState::Unconfirmed,
                    true,
                );
                uncertain.content_digest = "successor-uncertain-digest".into();
                tab.pending_delivery = Some(uncertain);
            } else {
                tab.pending_delivery
                    .as_mut()
                    .unwrap()
                    .begin_attempt(DeliveryAttemptProvenance {
                        attempt_id: "successor-attempt".into(),
                        started_at_ms: 19,
                    })
                    .unwrap();
                tab.pending_delivery.as_mut().unwrap().mark_unconfirmed();
            }
            tab.pending_delivery.clone()
        };
        boundary.wait("release old classifier");
        assert!(!join(old, "pending replacement restore").unwrap().unwrap());
        assert!(fixture.events.lock().is_empty());
        assert_eq!(
            fixture
                .mux
                .read()
                .get_tab(&fixture.tab_id)
                .unwrap()
                .pending_delivery,
            expected
        );
    }
}

#[test]
fn delayed_successful_restore_cannot_claim_reset_launch() {
    let pending = record(
        PendingDeliveryChannel::InteractiveInput,
        PendingDeliveryState::Unconfirmed,
        true,
    );
    let fixture = Arc::new(Fixture::new(pending, true));
    let boundary = Arc::new(DeadlineBarrier::new(2));
    let old = {
        let fixture = fixture.clone();
        let boundary = boundary.clone();
        std::thread::spawn(move || {
            super::super::restore_materialization_core(
                &fixture.mux,
                &fixture.tab_id,
                "fixture-provider",
                Some("original-used-session"),
                Some("fixture-cwd"),
                true,
                |captured| {
                    boundary.wait("successful restore captured");
                    boundary.wait("successful restore released");
                    resolve_restored_payload(captured)
                },
                |_, _, _| panic!("stale success cannot emit failure"),
                |_, _| panic!("stale success cannot claim successor"),
            )
            .map(|launch| launch.is_some())
        })
    };
    boundary.wait("reset launch");
    fixture
        .mux
        .write()
        .get_tab_mut(&fixture.tab_id)
        .unwrap()
        .lifecycle_epoch = Some(10);
    boundary.wait("release stale success");
    assert!(!join(old, "stale successful restore").unwrap().unwrap());
    assert!(
        fixture
            .mux
            .read()
            .get_tab(&fixture.tab_id)
            .unwrap()
            .restored_from_snapshot
    );
}

#[test]
fn restore_failure_publication_precedes_successor_ready_event() {
    use crate::service::boot_stage::{
        apply_boot_stage, boot_stage_event, commit_boot_stage_transition,
    };
    use crate::service::pty_io::test_support::TEST_WAIT_DEADLINE;
    use std::sync::mpsc;

    let pending = record(
        PendingDeliveryChannel::InteractiveInput,
        PendingDeliveryState::Pending,
        false,
    );
    let fixture = Arc::new(Fixture::new(pending.clone(), true));
    {
        let mut mux = fixture.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab_id).unwrap();
        tab.lifecycle_epoch = Some(9);
        tab.lifecycle_operation_id = Some("old-operation".into());
    }
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let (committed_tx, committed_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let (successor_attempt_tx, successor_attempt_rx) = mpsc::sync_channel(1);
    let (successor_published_tx, successor_published_rx) = mpsc::sync_channel(1);
    let old = {
        let fixture = fixture.clone();
        let events = events.clone();
        std::thread::spawn(move || {
            super::super::restore_materialization_core(
                &fixture.mux,
                &fixture.tab_id,
                "fixture-provider",
                Some("original-used-session"),
                Some("fixture-cwd"),
                true,
                resolve_restored_payload,
                |stage, reason, tab| {
                    assert_eq!(tab.boot_stage, Some(BootStage::SpawnFailed));
                    assert!(tab.reported_session_binding_nonce.is_none());
                    let event = boot_stage_event(&fixture.tab_id, tab, stage, Some(reason));
                    committed_tx.send(()).unwrap();
                    release_rx
                        .recv_timeout(TEST_WAIT_DEADLINE)
                        .expect("release old failure publisher");
                    events.lock().push(event);
                },
                |_, _| panic!("never-written historical input cannot launch"),
            )
            .map(|launch| launch.is_some())
        })
    };
    committed_rx
        .recv_timeout(TEST_WAIT_DEADLINE)
        .expect("old failure committed before publication");
    assert!(
        fixture.mux.try_write().is_none(),
        "failure mutation and publisher must retain the same mux boundary"
    );
    let successor = {
        let fixture = fixture.clone();
        let events = events.clone();
        std::thread::spawn(move || {
            successor_attempt_tx.send(()).unwrap();
            commit_boot_stage_transition(
                &fixture.mux,
                &fixture.tab_id,
                |tab| {
                    tab.lifecycle_epoch = Some(10);
                    tab.lifecycle_operation_id = Some("successor-operation".into());
                    tab.pty_id = Some("successor-pty".into());
                    tab.session_binding_nonce = Some("successor-nonce".into());
                    tab.reported_session_binding_nonce = Some("successor-nonce".into());
                    tab.restored_from_snapshot = false;
                    tab.agent_booting = false;
                    apply_boot_stage(tab, BootStage::AgentReady, None);
                    true
                },
                |tab| {
                    events.lock().push(boot_stage_event(
                        &fixture.tab_id,
                        tab,
                        BootStage::AgentReady,
                        None,
                    ));
                    successor_published_tx.send(()).unwrap();
                },
            )
        })
    };
    successor_attempt_rx
        .recv_timeout(TEST_WAIT_DEADLINE)
        .expect("successor attempts publication");
    assert!(matches!(
        successor_published_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    release_tx.send(()).unwrap();
    assert!(join(old, "serialized restore failure publisher")
        .unwrap()
        .is_err());
    assert!(join(successor, "serialized successor publisher").unwrap());
    successor_published_rx
        .recv_timeout(TEST_WAIT_DEADLINE)
        .expect("successor publication completed");
    let events = events.lock();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["stage"], "spawn_failed");
    assert_eq!(
        events[0]["launch_generation"],
        serde_json::json!({ "epoch": 9, "operation_id": "old-operation", "pty_id": "current-shell", "binding_nonce": "current-nonce" })
    );
    assert_eq!(events[1]["stage"], "agent_ready");
    assert_eq!(
        events[1]["launch_generation"],
        serde_json::json!({ "epoch": 10, "operation_id": "successor-operation", "pty_id": "successor-pty", "binding_nonce": "successor-nonce" })
    );
    let mux = fixture.mux.read();
    let tab = mux.get_tab(&fixture.tab_id).unwrap();
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert!(tab.launch_session_authenticated());
    assert_eq!(tab.pending_delivery.as_ref(), Some(&pending));
}

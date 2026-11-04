use super::*;

#[tokio::test]
async fn publish_carries_payload() {
    let bc = EventBroadcaster::new();
    let mut rx = bc.subscribe();
    bc.publish(SystemEvent::TabTitlePinned {
        tab_id: "p1".into(),
        title: "hello".into(),
    });
    match rx.recv().await.unwrap().event {
        SystemEvent::TabTitlePinned { tab_id, title } => {
            assert_eq!(tab_id, "p1");
            assert_eq!(title, "hello");
        }
        _ => panic!("wrong variant"),
    }
}

#[tokio::test]
async fn multiple_subscribers_each_receive() {
    let bc = EventBroadcaster::new();
    let mut rx1 = bc.subscribe();
    let mut rx2 = bc.subscribe();
    bc.publish(SystemEvent::TabFocused { tab_id: "p".into() });
    assert!(matches!(
        rx1.recv().await.unwrap().event,
        SystemEvent::TabFocused { .. }
    ));
    assert!(matches!(
        rx2.recv().await.unwrap().event,
        SystemEvent::TabFocused { .. }
    ));
}

#[tokio::test]
async fn slow_subscriber_lags_without_blocking_publisher() {
    let bc = EventBroadcaster::new();
    let mut rx = bc.subscribe();
    for i in 0..300u64 {
        bc.publish(SystemEvent::AgentIdle {
            tab_id: "p".into(),
            duration_secs: i,
        });
    }
    // Channel capacity is 256; oldest entries are dropped. First recv should signal Lagged, then continue yielding the remaining tail.
    let first = rx.recv().await;
    assert!(matches!(first, Err(broadcast::error::RecvError::Lagged(_))));
}

fn flood(bc: &EventBroadcaster, n: u64) {
    for i in 0..n {
        bc.send_bus(SystemEvent::AgentIdle {
            tab_id: "p".into(),
            duration_secs: i,
        });
    }
}

#[test]
fn replay_from_cursor_returns_missed_in_order() {
    let bc = EventBroadcaster::new();
    flood(&bc, 5); // seqs 1..=5
    let snap = bc.snapshot_ring();
    let boot = snap.boot_id.clone();
    match snap.plan(
        Some((&boot, 2)),
        ReplayBudget {
            max_bytes: usize::MAX,
            max_events: usize::MAX,
        },
    ) {
        ReplayPlan::Covered { replay } => {
            let seqs: Vec<u64> = replay.iter().map(|e| e.seq).collect();
            assert_eq!(seqs, vec![3, 4, 5]);
        }
        _ => panic!("expected Covered"),
    }
}

#[test]
fn covered_at_head_replays_nothing() {
    let bc = EventBroadcaster::new();
    flood(&bc, 5);
    let snap = bc.snapshot_ring();
    let boot = snap.boot_id.clone();
    match snap.plan(
        Some((&boot, 5)),
        ReplayBudget {
            max_bytes: usize::MAX,
            max_events: usize::MAX,
        },
    ) {
        ReplayPlan::Covered { replay } => assert!(replay.is_empty()),
        _ => panic!("expected Covered"),
    }
}

#[test]
fn stale_cursor_signals_evicted() {
    let bc = EventBroadcaster::new();
    flood(&bc, RING_MAX_COUNT as u64 + 10); // oldest retained advances past 1
    let snap = bc.snapshot_ring();
    assert_eq!(snap.entries.len(), RING_MAX_COUNT);
    let oldest = snap.entries.first().unwrap().seq;
    let boot = snap.boot_id.clone();
    match snap.plan(
        Some((&boot, 0)),
        ReplayBudget {
            max_bytes: usize::MAX,
            max_events: usize::MAX,
        },
    ) {
        ReplayPlan::Evicted {
            oldest: o, replay, ..
        } => {
            assert_eq!(o, oldest);
            assert_eq!(replay.len(), RING_MAX_COUNT);
        }
        _ => panic!("expected Evicted"),
    }
}

#[test]
fn boot_change_signals() {
    let bc = EventBroadcaster::new();
    flood(&bc, 3);
    let snap = bc.snapshot_ring();
    match snap.plan(
        Some(("some-other-boot", 1)),
        ReplayBudget {
            max_bytes: usize::MAX,
            max_events: usize::MAX,
        },
    ) {
        ReplayPlan::BootChanged { next_seq } => assert_eq!(next_seq, 4),
        _ => panic!("expected BootChanged"),
    }
}

#[test]
fn absent_cursor_starts_from_now() {
    let bc = EventBroadcaster::new();
    flood(&bc, 3);
    let snap = bc.snapshot_ring();
    assert!(matches!(
        snap.plan(
            None,
            ReplayBudget {
                max_bytes: usize::MAX,
                max_events: usize::MAX
            }
        ),
        ReplayPlan::FromNow
    ));
}

#[test]
fn ring_count_bound_holds_under_flood() {
    let bc = EventBroadcaster::new();
    flood(&bc, RING_MAX_COUNT as u64 * 2);
    let snap = bc.snapshot_ring();
    assert_eq!(snap.entries.len(), RING_MAX_COUNT);
    // Oldest retained = total - MAX + 1; newest = total. Gap-free seqs.
    assert_eq!(snap.entries.first().unwrap().seq, RING_MAX_COUNT as u64 + 1);
    assert_eq!(snap.entries.last().unwrap().seq, RING_MAX_COUNT as u64 * 2);
    assert_eq!(snap.next_seq, RING_MAX_COUNT as u64 * 2 + 1);
}

#[test]
fn ring_age_bound_evicts_front() {
    let mut ring = ReplayRing {
        entries: VecDeque::new(),
        next_seq: 1,
        retained_bytes: 0,
        retained_bytes_high_water: 0,
        encoded_events: 0,
        boot_id: "test".into(),
    };
    let t0 = Instant::now();
    ring.push(SystemEvent::MuxStateChanged, t0, true);
    ring.push(
        SystemEvent::MuxStateChanged,
        t0 + Duration::from_secs(1),
        true,
    );
    assert_eq!(ring.entries.len(), 2);
    // now = t0 + 121s; seq1 age 121s (>120) evicted, seq2 age 120s (not >120) kept.
    ring.push(
        SystemEvent::MuxStateChanged,
        t0 + RING_MAX_AGE + Duration::from_secs(1),
        true,
    );
    assert_eq!(ring.entries.len(), 2);
    assert_eq!(ring.entries.front().unwrap().seq, 2);
}

#[test]
fn live_tail_after_replay_has_no_gap_or_dup() {
    let bc = EventBroadcaster::new();
    let mut rx = bc.subscribe(); // subscribe FIRST (race-free attach)
    flood(&bc, 5); // seqs 1..=5 buffered in rx AND recorded in the ring
    let snap = bc.snapshot_ring();
    let boot = snap.boot_id.clone();
    let cursor: u64 = 2;
    let replay = match snap.plan(
        Some((&boot, cursor)),
        ReplayBudget {
            max_bytes: usize::MAX,
            max_events: usize::MAX,
        },
    ) {
        ReplayPlan::Covered { replay } => replay,
        _ => panic!("expected Covered"),
    };
    let mut replayed_max = cursor;
    let mut seen = Vec::new();
    for se in &replay {
        assert!(se.seq > replayed_max, "replay must be strictly increasing");
        replayed_max = se.seq;
        seen.push(se.seq);
    }
    flood(&bc, 1); // a live event lands (seq 6)
                   // Live loop dedupes the snapshot/live overlap by seq.
    while let Ok(se) = rx.try_recv() {
        if se.seq <= replayed_max {
            continue;
        }
        replayed_max = se.seq;
        seen.push(se.seq);
    }
    assert_eq!(seen, vec![3, 4, 5, 6]); // exact missed set, no gap, no dup at the seam
}

/// Concurrency load for the ordering test. A window of
/// PUBLISHERS * PER_PUBLISHER stays inside the bus capacity and the ring
/// bound, so a correct run cannot lag or evict; ROUNDS repeats the race.
const ORDER_PUBLISHERS: usize = 24;
const ORDER_PER_PUBLISHER: usize = 10;
const ORDER_ROUNDS: usize = 20;

#[test]
fn concurrent_publishers_deliver_every_seq_in_order() {
    use std::sync::Barrier;
    let total = ORDER_PUBLISHERS * ORDER_PER_PUBLISHER;
    assert!(total < SYSTEM_BUS_CAPACITY, "window must fit the bus");
    assert!(total < RING_MAX_COUNT, "window must fit the ring");
    let expected: Vec<u64> = (1..=total as u64).collect();

    for round in 0..ORDER_ROUNDS {
        let bc = Arc::new(EventBroadcaster::new());
        let mut rx = bc.subscribe();
        let gate = Arc::new(Barrier::new(ORDER_PUBLISHERS));
        let mut handles = Vec::with_capacity(ORDER_PUBLISHERS);
        for p in 0..ORDER_PUBLISHERS {
            let bc = Arc::clone(&bc);
            let gate = Arc::clone(&gate);
            handles.push(std::thread::spawn(move || {
                gate.wait();
                for i in 0..ORDER_PER_PUBLISHER {
                    bc.send_bus(SystemEvent::AgentIdle {
                        tab_id: format!("p{p}"),
                        duration_secs: i as u64,
                    });
                }
            }));
        }
        for h in handles {
            h.join().expect("publisher panicked");
        }

        let mut delivered = Vec::with_capacity(total);
        loop {
            match rx.try_recv() {
                Ok(se) => delivered.push(se.seq),
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    panic!("round {round}: spurious overflow, bus lagged by {n}")
                }
            }
        }
        assert_eq!(delivered.len(), total, "round {round}: lost a publication");
        assert_eq!(
            delivered,
            expected,
            "round {round}: delivery order != seq order, first inversion at index {:?}",
            delivered
                .iter()
                .zip(expected.iter())
                .position(|(a, b)| a != b)
        );
    }
}

/// Mirrors the `events_stream` decision table in web_routes.rs (read-only
/// here): one overflow frame per plan classification, one per live `Lagged`.
fn overflow_count_for(plan: &ReplayPlan) -> usize {
    match plan {
        ReplayPlan::FromNow | ReplayPlan::Covered { .. } => 0,
        ReplayPlan::BootChanged { .. } | ReplayPlan::Evicted { .. } => 1,
    }
}

#[test]
fn full_ring_emits_exactly_one_overflow() {
    let bc = EventBroadcaster::new();
    flood(&bc, RING_MAX_COUNT as u64 + 64);

    let mut rx = bc.subscribe();
    let snap = bc.snapshot_ring();
    let boot = snap.boot_id.clone();
    let plan = snap.plan(
        Some((&boot, 0)),
        ReplayBudget {
            max_bytes: usize::MAX,
            max_events: usize::MAX,
        },
    );
    let mut overflows = overflow_count_for(&plan);
    let (mut replayed_max, replay) = match plan {
        ReplayPlan::Evicted {
            next_seq, replay, ..
        } => (next_seq.saturating_sub(1), replay),
        _ => panic!("expected Evicted for a full ring with a stale cursor"),
    };
    assert_eq!(
        overflows, 1,
        "stale cursor must produce exactly one overflow"
    );
    assert_eq!(replay.len(), RING_MAX_COUNT);

    flood(&bc, 4);
    let mut live = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(se) => {
                if se.seq <= replayed_max {
                    continue;
                }
                replayed_max = se.seq;
                live.push(se.seq);
            }
            Err(broadcast::error::TryRecvError::Empty)
            | Err(broadcast::error::TryRecvError::Closed) => break,
            Err(broadcast::error::TryRecvError::Lagged(_)) => overflows += 1,
        }
    }
    assert_eq!(live.len(), 4, "live tail after overflow must be gap-free");
    assert_eq!(overflows, 1, "a full ring must emit exactly one overflow");
}

#[test]
fn live_loop_still_drops_replay_overlap() {
    let bc = EventBroadcaster::new();
    let mut rx = bc.subscribe();
    flood(&bc, 5);
    let snap = bc.snapshot_ring();
    let boot = snap.boot_id.clone();
    let cursor = 2u64;
    let replay = match snap.plan(
        Some((&boot, cursor)),
        ReplayBudget {
            max_bytes: usize::MAX,
            max_events: usize::MAX,
        },
    ) {
        ReplayPlan::Covered { replay } => replay,
        _ => panic!("expected Covered"),
    };
    let mut replayed_max = cursor;
    let mut yielded = Vec::new();
    for se in &replay {
        replayed_max = se.seq;
        yielded.push(se.seq);
    }
    assert_eq!(yielded, vec![3, 4, 5]);

    flood(&bc, 1);
    let mut dropped = 0usize;
    while let Ok(se) = rx.try_recv() {
        if se.seq <= replayed_max {
            dropped += 1;
            continue;
        }
        replayed_max = se.seq;
        yielded.push(se.seq);
    }
    assert_eq!(dropped, 5, "the guard must drop the whole 1..=5 overlap");
    assert_eq!(
        yielded,
        vec![3, 4, 5, 6],
        "no duplicate at the reconnect seam"
    );
}

#[test]
fn send_bus_critical_section_stays_under_lock_hold_threshold() {
    const SAMPLES: u64 = 10_000;
    let bc = EventBroadcaster::new();
    let _rx = bc.subscribe();
    let mut worst = Duration::ZERO;
    for i in 0..SAMPLES {
        let started = Instant::now();
        bc.send_bus(SystemEvent::AgentIdle {
            tab_id: "p".into(),
            duration_secs: i,
        });
        worst = worst.max(started.elapsed());
    }
    println!("send_bus worst critical section over {SAMPLES} calls: {worst:?}");
    assert!(
        worst < Duration::from_millis(50),
        "critical section crossed the [LOCK-HOLD] threshold: {worst:?}"
    );
}

fn large_event(bytes: usize) -> SystemEvent {
    SystemEvent::Generic {
        event_type: "large".into(),
        data: serde_json::json!({"text": "x".repeat(bytes)}),
    }
}

#[test]
fn byte_eviction_is_oldest_first_and_gap_free() {
    let bc = EventBroadcaster::new();
    for _ in 0..5 {
        bc.send_bus(large_event(2 * 1024 * 1024));
        assert!(bc.replay_stats().retained_bytes <= RING_MAX_BYTES);
    }
    let snap = bc.snapshot_ring();
    assert_eq!(
        snap.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        vec![3, 4, 5]
    );
    assert_eq!(snap.next_seq, 6);
    assert_eq!(
        bc.replay_stats().retained_bytes,
        snap.entries.iter().map(|e| e.frame.size).sum::<usize>()
    );
}

#[test]
fn oversized_payload_emits_an_ordered_overflow_marker() {
    let bc = EventBroadcaster::new();
    bc.send_bus(large_event(RING_MAX_BYTES));
    let snap = bc.snapshot_ring();
    let marker: serde_json::Value = serde_json::from_str(&snap.entries[0].frame.bytes).unwrap();
    assert_eq!(marker["event_type"], "overflow");
    assert_eq!(marker["data"]["reason"], "payload-limit");
    assert_eq!(marker["boot"], bc.boot_id());
    assert_eq!(marker["seq"], 1);
    assert!(!bc.replay_stats().oversize_singleton);
    assert!(bc.replay_stats().retained_bytes < MAX_EVENT_BYTES);
    bc.send_bus(SystemEvent::MuxStateChanged);
    assert_eq!(bc.snapshot_ring().entries[1].seq, 2);
}

#[test]
fn replay_budget_exact_fit_and_newest_contiguous_suffix() {
    let bc = EventBroadcaster::new();
    flood(&bc, 3);
    let snap = bc.snapshot_ring();
    let bytes = snap.entries.iter().map(|e| e.frame.size).sum::<usize>();
    assert!(matches!(snap.plan(Some((bc.boot_id(), 0)),
            ReplayBudget { max_bytes: bytes, max_events: 3 }),
            ReplayPlan::Covered { replay } if replay.len() == 3));
    for budget in [
        ReplayBudget {
            max_bytes: bytes - 1,
            max_events: 3,
        },
        ReplayBudget {
            max_bytes: bytes,
            max_events: 2,
        },
    ] {
        match snap.plan(Some((bc.boot_id(), 0)), budget) {
            ReplayPlan::Evicted { replay, .. } => {
                assert_eq!(replay.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![2, 3]);
                assert!(replay.iter().map(|e| e.frame.size).sum::<usize>() <= budget.max_bytes);
            }
            _ => panic!("expected budget overflow"),
        }
    }
    assert!(
        matches!(snap.plan(Some((bc.boot_id(), u64::MAX)), ReplayBudget::default()),
            ReplayPlan::Covered { replay } if replay.is_empty())
    );
}

#[test]
fn replay_default_limits_hold_for_covered_and_evicted_cursors() {
    let bc = EventBroadcaster::new();
    flood(&bc, 2100);
    let snap = bc.snapshot_ring();
    for cursor in [0, 52] {
        match snap.plan(Some((bc.boot_id(), cursor)), ReplayBudget::default()) {
            ReplayPlan::Evicted { replay, .. } => {
                assert_eq!(replay.len(), REPLAY_MAX_EVENTS);
                assert_eq!(replay.last().unwrap().seq, 2100);
                assert!(replay.iter().map(|e| e.frame.size).sum::<usize>() <= REPLAY_MAX_BYTES);
            }
            _ => panic!("expected replay limit"),
        }
    }
}

#[test]
fn replay_default_byte_budget_keeps_only_the_newest_fitting_events() {
    let bc = EventBroadcaster::new();
    for _ in 0..3 {
        bc.send_bus(large_event(1024 * 1024));
    }
    let snap = bc.snapshot_ring();
    match snap.plan(Some((bc.boot_id(), 0)), ReplayBudget::default()) {
        ReplayPlan::Evicted { replay, .. } => {
            assert_eq!(replay.len(), 1);
            assert_eq!(replay[0].seq, 3);
            assert!(replay[0].frame.size <= REPLAY_MAX_BYTES);
        }
        _ => panic!("expected byte budget overflow"),
    }
}

#[test]
fn publish_encodes_once_and_shares_frame_with_every_subscriber_and_replay() {
    let bc = EventBroadcaster::new();
    let mut receivers: Vec<_> = (0..8).map(|_| bc.subscribe()).collect();
    bc.publish(SystemEvent::MuxStateChanged);
    let snap = bc.snapshot_ring();
    let frame = &snap.entries[0].frame;
    for rx in &mut receivers {
        let received = rx.try_recv().unwrap();
        assert!(Arc::ptr_eq(frame, &received.frame));
        assert_eq!(received.seq, frame.seq);
    }
    assert_eq!(bc.replay_stats().encoded_events, 1);
    assert_eq!(bc.replay_stats().published_events, 1);
    let next = bc.snapshot_ring();
    assert!(Arc::ptr_eq(&snap.entries[0], &next.entries[0]));
}

#[test]
fn type_name_covers_all_variants() {
    assert_eq!(
        SystemEvent::MuxStateChanged.type_name(),
        "mux-state-changed"
    );
    assert_eq!(
        SystemEvent::TabTitlePinned {
            tab_id: "a".into(),
            title: "b".into()
        }
        .type_name(),
        "tab-title-pinned",
    );
    assert_eq!(
        SystemEvent::PluginActivity(PluginActivity {
            id: "a".into(),
            plugin_id: "plugin".into(),
            kind: "install".into(),
            label: "Installing".into(),
            percent: None,
            status: "running".into(),
            error: None,
            started_at: 1,
            updated_at: 1,
        })
        .type_name(),
        "plugin-activity",
    );
}

#[test]
fn held_event_encoding_does_not_hold_ring_or_other_publishers() {
    use std::sync::mpsc;
    let bc = Arc::new(EventBroadcaster::new());
    let (entered, started) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let publisher = {
        let bc = bc.clone();
        std::thread::spawn(move || {
            bc.prepare_and_send(SystemEvent::MuxStateChanged, true, |event, boot| {
                entered.send(()).unwrap();
                held.recv().unwrap();
                PreparedEvent::new(event, boot)
            })
        })
    };
    started.recv_timeout(Duration::from_secs(1)).unwrap();
    let (reply, result) = mpsc::channel();
    let reader = {
        let bc = bc.clone();
        std::thread::spawn(move || {
            let before = bc.current_seq();
            bc.send_bus(SystemEvent::MuxStateChanged);
            reply.send((before, bc.snapshot_ring().next_seq)).unwrap();
        })
    };
    let observed = result.recv_timeout(Duration::from_secs(1));
    release.send(()).unwrap();
    publisher.join().unwrap();
    reader.join().unwrap();
    assert_eq!(
        observed.expect("ring operations must finish with encoder held"),
        (1, 2)
    );
    assert_eq!(
        bc.snapshot_ring()
            .entries
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

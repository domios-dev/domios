use super::{
    ordered_ids, run_transport_waves, run_wave, take_next, RestoreBudgets, RestoreTransport,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

#[test]
fn bounded_transports_progress_independently_without_starving_queued_tabs() {
    let (entered, observed) = mpsc::channel();
    let (release_pty, pty_tokens) = mpsc::channel();
    let (release_chat, chat_tokens) = mpsc::channel();
    let pty_tokens = Mutex::new(pty_tokens);
    let chat_tokens = Mutex::new(chat_tokens);
    let worker = std::thread::spawn(move || {
        let items = [RestoreTransport::Chat; 6]
            .into_iter()
            .chain([RestoreTransport::Pty; 7])
            .collect::<Vec<_>>();
        run_transport_waves(
            &items,
            RestoreBudgets {
                foreground: 1,
                background: 4,
                chat: 2,
            },
            |kind| *kind,
            || vec![super::Priority::Background; 13],
            |_| Ok(()),
            |kind, _| {
                entered.send(*kind).unwrap();
                match kind {
                    RestoreTransport::Pty => pty_tokens.lock().unwrap().recv().unwrap(),
                    RestoreTransport::Chat => chat_tokens.lock().unwrap().recv().unwrap(),
                }
                Ok(())
            },
        )
    });
    let first = (0..6)
        .map(|_| observed.recv_timeout(Duration::from_secs(2)).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        first
            .iter()
            .filter(|kind| **kind == RestoreTransport::Pty)
            .count(),
        4
    );
    assert_eq!(
        first
            .iter()
            .filter(|kind| **kind == RestoreTransport::Chat)
            .count(),
        2
    );
    assert!(matches!(
        observed.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    for _ in 0..7 {
        release_pty.send(()).unwrap();
    }
    for _ in 0..3 {
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(2)).unwrap(),
            RestoreTransport::Pty
        );
    }
    for _ in 0..6 {
        release_chat.send(()).unwrap();
    }
    for _ in 0..4 {
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(2)).unwrap(),
            RestoreTransport::Chat
        );
    }
    assert_eq!(worker.join().unwrap(), vec![Ok(()); 13]);
}

#[test]
fn general_tabs_precede_projects_then_recent_activity_wins() {
    let ids = ordered_ids(vec![
        ("project-old".into(), false, Some(1)),
        ("general-old".into(), true, Some(1)),
        ("project-new".into(), false, Some(9)),
        ("general-new".into(), true, Some(2)),
        ("unknown".into(), false, None),
    ]);
    assert_eq!(
        ids,
        [
            "general-new",
            "general-old",
            "project-new",
            "project-old",
            "unknown"
        ]
    );
}

#[test]
fn changed_focus_takes_the_next_slot_without_reclaiming_in_flight_tabs() {
    let mut pending = vec![0, 1, 2, 3];
    assert_eq!(take_next(&mut pending, Some(2)), Some(2));
    assert_eq!(take_next(&mut pending, Some(3)), Some(3));
    assert_eq!(take_next(&mut pending, Some(2)), Some(0));
    assert_eq!(take_next(&mut pending, None), Some(1));
    assert_eq!(take_next(&mut pending, Some(3)), None);
}

#[test]
fn wave_reads_current_focus_when_each_slot_becomes_free() {
    let focused = AtomicUsize::new(0);
    let launched = Mutex::new(Vec::new());
    let results = run_wave(
        &[0, 1, 2, 3, 4],
        1,
        || Some(focused.load(Ordering::Relaxed)),
        |_| Ok(()),
        |item| {
            launched.lock().unwrap().push(*item);
            focused.store(if *item == 0 { 4 } else { 3 }, Ordering::Relaxed);
            Ok(())
        },
    );
    assert_eq!(launched.into_inner().unwrap(), [0, 4, 3, 1, 2]);
    assert_eq!(results, vec![Ok(()); 5]);
}

#[test]
fn every_cwd_is_validated_before_slots_start_and_failures_are_isolated() {
    let events = Mutex::new(Vec::new());
    let results = run_wave(
        &[0, 1, 2, 3],
        4,
        || None,
        |item| {
            events.lock().unwrap().push(("validate", *item));
            if *item == 0 {
                Err("missing-cwd".into())
            } else {
                Ok(())
            }
        },
        |item| {
            let mut events = events.lock().unwrap();
            assert_eq!(
                events.iter().filter(|event| event.0 == "validate").count(),
                4
            );
            events.push(("materialize", *item));
            if *item == 2 {
                Err("spawn-refused".into())
            } else {
                Ok(())
            }
        },
    );
    assert_eq!(
        results,
        [
            Err("missing-cwd".into()),
            Ok(()),
            Err("spawn-refused".into()),
            Ok(())
        ]
    );
    let events = events.into_inner().unwrap();
    assert!(!events.contains(&("materialize", 0)));
    assert_eq!(events.len(), 7);
}

#[test]
fn four_tabs_can_start_independently_and_the_fifth_waits_for_a_slot() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release = Arc::new(Mutex::new(release_rx));
    let worker = std::thread::spawn(move || {
        run_wave(
            &[0, 1, 2, 3, 4],
            4,
            || None,
            |_| Ok(()),
            |item| {
                entered_tx.send(*item).unwrap();
                release.lock().unwrap().recv().unwrap();
                Ok(())
            },
        )
    });
    let mut first = (0..4)
        .map(|_| entered_rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .collect::<Vec<_>>();
    first.sort();
    assert_eq!(first, [0, 1, 2, 3]);
    assert!(matches!(
        entered_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    for _ in 0..5 {
        release_tx.send(()).unwrap();
    }
    assert_eq!(worker.join().unwrap(), vec![Ok(()); 5]);
    assert_eq!(entered_rx.recv_timeout(Duration::from_secs(5)).unwrap(), 4);
}

#[test]
fn reserved_worker_never_takes_background_and_focus_changes_reprioritize_pending() {
    use super::{take_prioritized, Priority};
    let mut pending = vec![0, 1, 2, 3];
    let mut priorities = vec![Priority::Background; 4];
    assert_eq!(take_prioritized(&mut pending, &priorities, true), None);
    assert_eq!(pending, [0, 1, 2, 3]);
    priorities[3] = Priority::Focused;
    priorities[2] = Priority::Visible;
    assert_eq!(take_prioritized(&mut pending, &priorities, true), Some(3));
    assert_eq!(take_prioritized(&mut pending, &priorities, false), Some(2));
    assert_eq!(take_prioritized(&mut pending, &priorities, true), None);
    assert_eq!(take_prioritized(&mut pending, &priorities, false), Some(0));
}

#[test]
fn visibility_classification_preserves_focused_hidden_and_workspace_rules() {
    use super::Priority;
    assert_eq!(
        Priority::from_visibility(true, false, false, true),
        Priority::Focused
    );
    assert_eq!(
        Priority::from_visibility(false, true, true, false),
        Priority::Visible
    );
    for flags in [
        (false, true, true),
        (true, false, false),
        (true, true, true),
    ] {
        assert_eq!(
            Priority::from_visibility(false, flags.0, flags.1, flags.2),
            Priority::Background
        );
    }
}

#[test]
fn five_workers_serve_visible_burst_before_any_completion() {
    let (entered, observed) = mpsc::channel();
    let (release, tokens) = mpsc::channel();
    let tokens = Mutex::new(tokens);
    let worker = std::thread::spawn(move || {
        run_transport_waves(
            &[0, 1, 2, 3, 4],
            super::RESTORE_BUDGETS,
            |_| RestoreTransport::Pty,
            || vec![super::Priority::Visible; 5],
            |_| Ok(()),
            |item, admission| {
                entered.send((*item, admission)).unwrap();
                tokens.lock().unwrap().recv().unwrap();
                Ok(())
            },
        )
    });
    let admitted = (0..5)
        .map(|_| observed.recv_timeout(Duration::from_secs(2)).unwrap())
        .collect::<Vec<_>>();
    assert!(admitted
        .iter()
        .all(|(_, admission)| admission.priority == super::Priority::Visible));
    assert_eq!(
        admitted
            .iter()
            .filter(|(_, admission)| admission.permit == super::AdmissionPermit::Reserved)
            .count(),
        1
    );
    assert_eq!(
        admitted
            .iter()
            .filter(|(_, admission)| admission.permit == super::AdmissionPermit::Shared)
            .count(),
        4
    );
    let mut ids = admitted.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, [0, 1, 2, 3, 4]);
    for _ in 0..5 {
        release.send(()).unwrap();
    }
    assert_eq!(worker.join().unwrap(), vec![Ok(()); 5]);
}

#[test]
fn focus_event_uses_reserved_worker_while_all_shared_workers_are_busy() {
    let focused = Arc::new(AtomicUsize::new(usize::MAX));
    let current_focus = focused.clone();
    let (entered, observed) = mpsc::channel();
    let (release, tokens) = mpsc::channel();
    let tokens = Mutex::new(tokens);
    let worker = std::thread::spawn(move || {
        run_transport_waves(
            &[0, 1, 2, 3, 4, 5],
            super::RESTORE_BUDGETS,
            |_| RestoreTransport::Pty,
            || {
                (0..6)
                    .map(|index| {
                        if index == current_focus.load(Ordering::SeqCst) {
                            super::Priority::Focused
                        } else {
                            super::Priority::Background
                        }
                    })
                    .collect()
            },
            |_| Ok(()),
            |item, admission| {
                entered.send((*item, admission)).unwrap();
                tokens.lock().unwrap().recv().unwrap();
                Ok(())
            },
        )
    });
    let background = (0..4)
        .map(|_| observed.recv_timeout(Duration::from_secs(2)).unwrap())
        .collect::<Vec<_>>();
    assert!(background.iter().all(|(_, admission)| admission.priority
        == super::Priority::Background
        && admission.permit == super::AdmissionPermit::Shared));
    let mut ids = background.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, [0, 1, 2, 3]);
    focused.store(4, Ordering::SeqCst);
    super::changes().notify();
    let (id, admission) = observed.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(id, 4);
    assert_eq!(admission.priority, super::Priority::Focused);
    assert_eq!(admission.permit, super::AdmissionPermit::Reserved);
    for _ in 0..6 {
        release.send(()).unwrap();
    }
    assert_eq!(worker.join().unwrap(), vec![Ok(()); 6]);
}

#[test]
fn change_between_observation_and_wait_cannot_be_lost() {
    let changes = codeterm_service_core::service::restore_admission::Changes::default();
    let observed = changes.revision();
    changes.notify();
    changes.wait(observed);
    assert_ne!(changes.revision(), observed);
}

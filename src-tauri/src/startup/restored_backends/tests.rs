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
            RestoreBudgets { pty: 4, chat: 2 },
            |kind| *kind,
            || None,
            |_| Ok(()),
            |kind| {
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

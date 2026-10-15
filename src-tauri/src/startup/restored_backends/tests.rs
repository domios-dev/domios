use super::{ordered_ids, run_wave};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

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
fn every_cwd_is_validated_before_slots_start_and_failures_are_isolated() {
    let events = Mutex::new(Vec::new());
    let results = run_wave(
        &[0, 1, 2, 3],
        4,
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

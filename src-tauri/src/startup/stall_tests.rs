use super::progress::{Boundary, StartupStep};
use parking_lot::Mutex;
use std::cell::RefCell;
use std::sync::{mpsc, Arc};
use std::time::Duration;

#[derive(Debug)]
struct TestChild {
    id: u8,
    calls: u64,
}

#[test]
fn startup_no_global_stall_slow_child_does_not_block_unrelated_caller() {
    let slot = Arc::new(Mutex::new(Some(TestChild { id: 1, calls: 0 })));
    let (request_started_tx, request_started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let first_slot = slot.clone();
    let first = std::thread::spawn(move || {
        codeterm_util::child_slot::request_with_idle(
            &first_slot,
            || Err("the idle child must be reused".into()),
            |_| false,
            |child| {
                request_started_tx.send(child.id).unwrap();
                release_rx.recv().unwrap();
                child.calls += 1;
                Ok(child.id)
            },
        )
    });

    assert_eq!(request_started_rx.recv().unwrap(), 1);
    let second_slot = slot.clone();
    let (second_done_tx, second_done_rx) = mpsc::channel();
    let second = std::thread::spawn(move || {
        let result = codeterm_util::child_slot::request_with_idle(
            &second_slot,
            || Ok(TestChild { id: 2, calls: 0 }),
            |_| false,
            |child| {
                child.calls += 1;
                Ok(child.id)
            },
        );
        second_done_tx.send(result).unwrap();
    });
    let second_result = second_done_rx.recv_timeout(Duration::from_secs(1));
    if second_result.is_err() {
        release_tx.send(()).unwrap();
        first.join().unwrap().unwrap();
        second.join().unwrap();
        panic!("an unrelated caller waited behind the active child request");
    }
    assert_eq!(second_result.unwrap().unwrap(), 2);

    release_tx.send(()).unwrap();
    assert_eq!(first.join().unwrap().unwrap(), 1);
    second.join().unwrap();
}

#[test]
fn startup_no_global_stall_child_requests_stay_exclusive_and_recycle() {
    let call_budget = crate::mem::embeddings::EMBED_CHILD_CALL_BUDGET;
    let slot = Mutex::new(Some(TestChild { id: 1, calls: 0 }));
    for expected_calls in 1..=2 {
        let id = codeterm_util::child_slot::request_with_idle(
            &slot,
            || Err("the idle child must be reused".into()),
            |child| child.calls >= call_budget,
            |child| {
                child.calls += 1;
                Ok(child.id)
            },
        )
        .unwrap();
        assert_eq!(id, 1);
        assert_eq!(slot.lock().as_ref().unwrap().calls, expected_calls);
    }

    slot.lock().as_mut().unwrap().calls = call_budget;
    let recycled_id = codeterm_util::child_slot::request_with_idle(
        &slot,
        || Ok(TestChild { id: 2, calls: 0 }),
        |child| child.calls >= call_budget,
        |child| {
            child.calls += 1;
            Ok(child.id)
        },
    )
    .unwrap();

    assert_eq!(recycled_id, 2);
    let guard = slot.lock();
    let child = guard.as_ref().unwrap();
    assert_eq!(child.id, 2);
    assert_eq!(child.calls, 1);
}

#[test]
fn startup_no_global_stall_step_records_both_boundaries() {
    let boundaries = RefCell::new(Vec::new());
    {
        let _step = StartupStep::with_sink("opaque-step", |boundary, _, _| {
            boundaries.borrow_mut().push(boundary);
        });
    }

    assert_eq!(
        boundaries.into_inner(),
        vec![Boundary::Started, Boundary::Completed]
    );
}

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

static L12: &LocalModel = &crate::embedder::local::MODELS[0];

#[test]
fn cold_query_waits_for_model_readiness_and_returns_the_vector() {
    let (worker, receivers) = channels();
    let (load_started, started) = mpsc::channel();
    let (release, readiness) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        run(
            receivers,
            cpu_pool,
            |_| {
                load_started.send(()).unwrap();
                readiness.recv().unwrap();
                Ok(())
            },
            |_, _| Ok(vec![0.5, 0.5]),
        )
    });
    let (reply, result) = mpsc::channel();
    let query = std::thread::spawn(move || {
        reply.send(worker.embed(L12, "cold query", true)).unwrap();
    });
    started.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));
    release.send(()).unwrap();
    assert_eq!(
        result
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
        vec![0.5, 0.5]
    );
    query.join().unwrap();
    thread.join().unwrap();
}

#[test]
fn queries_run_before_queued_bulk_work() {
    let (worker, receivers) = channels();
    let (reply, _) = mpsc::sync_channel(1);
    worker
        .bulk
        .try_send(Request {
            model: L12,
            text: "bulk".into(),
            reply: reply.clone(),
        })
        .unwrap();
    worker
        .query
        .try_send(Request {
            model: L12,
            text: "query".into(),
            reply,
        })
        .unwrap();
    let first = receivers.next().unwrap().text;
    let second = receivers.next().unwrap().text;
    assert_eq!((first.as_str(), second.as_str()), ("query", "bulk"));
}

#[test]
fn full_bulk_queue_refuses_without_waiting_for_the_model() {
    let (worker, _receivers) = channels();
    for _ in 0..QUEUE_CAPACITY {
        let (reply, _) = mpsc::sync_channel(1);
        worker
            .bulk
            .try_send(Request {
                model: L12,
                text: String::new(),
                reply,
            })
            .unwrap();
    }
    assert!(worker
        .embed(L12, "overflow", false)
        .unwrap_err()
        .starts_with("embedding busy:"));
}

#[test]
fn failed_load_answers_the_waiter_and_allows_explicit_retry() {
    let (worker, receivers) = channels();
    let thread = std::thread::spawn(move || {
        run(
            receivers,
            cpu_pool,
            |_| Err::<(), _>("offline".into()),
            |_, _| unreachable!(),
        )
    });
    assert_eq!(worker.embed(L12, "one", false).unwrap_err(), "offline");
    assert_eq!(worker.embed(L12, "two", false).unwrap_err(), "offline");
    drop(worker);
    thread.join().unwrap();
}

#[test]
fn idle_worker_joins_cpu_pool_and_retains_model_until_descriptor_changes() {
    let (worker, mut receivers) = channels();
    let (waiting, idle) = mpsc::channel();
    receivers.waiting = Some(waiting);
    let pools = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(AtomicUsize::new(0));
    let exited = Arc::new(AtomicUsize::new(0));
    let forwards = Arc::new(AtomicUsize::new(0));
    let loads = Arc::new(AtomicUsize::new(0));
    let thread = {
        let pools = pools.clone();
        let started = started.clone();
        let exited = exited.clone();
        let forwards = forwards.clone();
        let loads = loads.clone();
        std::thread::spawn(move || {
            run(
                receivers,
                || {
                    pools.fetch_add(1, Ordering::SeqCst);
                    let started = started.clone();
                    let exited = exited.clone();
                    cpu_pool()
                        .start_handler(move |_| {
                            started.fetch_add(1, Ordering::SeqCst);
                        })
                        .exit_handler(move |_| {
                            exited.fetch_add(1, Ordering::SeqCst);
                        })
                },
                |_| {
                    loads.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
                |_, _| {
                    assert!(rayon::current_thread_index().is_some());
                    forwards.fetch_add(1, Ordering::SeqCst);
                    Ok(vec![1.0])
                },
            )
        })
    };

    idle.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(pools.load(Ordering::SeqCst), 0);
    assert_eq!(started.load(Ordering::SeqCst), 0);
    assert_eq!(loads.load(Ordering::SeqCst), 0);
    assert_eq!(forwards.load(Ordering::SeqCst), 0);

    let e5 = &crate::embedder::local::MODELS[1];
    for (index, descriptor) in [L12, L12, e5, e5, L12].into_iter().enumerate() {
        assert_eq!(worker.embed(descriptor, "job", true).unwrap(), vec![1.0]);
        idle.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(pools.load(Ordering::SeqCst), index + 1);
        assert_eq!(started.load(Ordering::SeqCst), (index + 1) * 4);
        assert_eq!(exited.load(Ordering::SeqCst), (index + 1) * 4);
        assert_eq!(forwards.load(Ordering::SeqCst), index + 1);
        assert_eq!(loads.load(Ordering::SeqCst), [1, 1, 2, 2, 3][index]);
        assert!(matches!(idle.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }
    drop(worker);
    thread.join().unwrap();
}

#[test]
fn request_arriving_during_pool_join_runs_without_a_second_wake() {
    let (worker, mut receivers) = channels();
    let (waiting, idle) = mpsc::channel();
    receivers.waiting = Some(waiting);
    let (retiring, retired) = mpsc::channel();
    let (release, exit) = mpsc::channel();
    let exit = Arc::new(parking_lot::Mutex::new(exit));
    let loads = Arc::new(AtomicUsize::new(0));
    let thread = {
        let loads = loads.clone();
        std::thread::spawn(move || {
            run(
                receivers,
                || {
                    let retiring = retiring.clone();
                    let exit = exit.clone();
                    cpu_pool().exit_handler(move |index| {
                        if index == 0 {
                            retiring.send(()).unwrap();
                            exit.lock().recv_timeout(Duration::from_secs(1)).unwrap();
                        }
                    })
                },
                |_| {
                    loads.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
                |_, _| Ok(vec![1.0]),
            )
        })
    };
    idle.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(worker.embed(L12, "first", true).unwrap(), vec![1.0]);
    retired.recv_timeout(Duration::from_secs(1)).unwrap();
    worker.wake.try_send(()).unwrap();
    let (reply, result) = mpsc::sync_channel(1);
    worker
        .query
        .send(Request {
            model: L12,
            text: "during join".into(),
            reply,
        })
        .unwrap();
    // Coalescing a wake during teardown must not hide the queued request.
    assert!(matches!(
        worker.wake.try_send(()),
        Err(mpsc::TrySendError::Full(()))
    ));
    release.send(()).unwrap();
    assert_eq!(
        result
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap(),
        vec![1.0]
    );
    retired.recv_timeout(Duration::from_secs(1)).unwrap();
    release.send(()).unwrap();
    idle.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(loads.load(Ordering::SeqCst), 1);
    drop(worker);
    thread.join().unwrap();
}

#[test]
fn queued_requests_share_one_cpu_pool() {
    let (worker, mut receivers) = channels();
    let (waiting, idle) = mpsc::channel();
    receivers.waiting = Some(waiting);
    let (reply, result) = mpsc::sync_channel(2);
    for text in ["first", "second"] {
        worker
            .bulk
            .send(Request {
                model: L12,
                text: text.into(),
                reply: reply.clone(),
            })
            .unwrap();
    }
    let pools = Arc::new(AtomicUsize::new(0));
    let thread = {
        let pools = pools.clone();
        std::thread::spawn(move || {
            run(
                receivers,
                || {
                    pools.fetch_add(1, Ordering::SeqCst);
                    cpu_pool()
                },
                |_| Ok(()),
                |_, _| Ok(vec![1.0]),
            )
        })
    };
    for _ in 0..2 {
        assert_eq!(
            result
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap(),
            vec![1.0]
        );
    }
    idle.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(pools.load(Ordering::SeqCst), 1);
    drop(worker);
    thread.join().unwrap();
}

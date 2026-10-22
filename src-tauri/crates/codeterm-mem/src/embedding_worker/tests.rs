use super::*;

static L12: &LocalModel = &crate::embedder::local::MODELS[0];

#[test]
fn cold_query_waits_for_model_readiness_and_returns_the_vector() {
    let (worker, receivers) = channels();
    let (load_started, started) = mpsc::channel();
    let (release, readiness) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        run(
            receivers,
            Duration::ZERO,
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
    let first = receivers.next(None).unwrap().text;
    let second = receivers.next(None).unwrap().text;
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

struct Loaded(mpsc::Sender<()>);

impl Drop for Loaded {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[test]
fn idle_deadline_drops_the_model_and_next_job_loads_again() {
    let (worker, receivers) = channels();
    let (dropped, drop_events) = mpsc::channel();
    let (loaded, load_events) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        run(
            receivers,
            Duration::ZERO,
            |_| {
                loaded.send(()).unwrap();
                Ok(Loaded(dropped.clone()))
            },
            |_, _| Ok(vec![1.0]),
        )
    });
    assert_eq!(worker.embed(L12, "one", false).unwrap(), vec![1.0]);
    load_events.recv_timeout(Duration::from_secs(1)).unwrap();
    drop_events.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(worker.embed(L12, "two", false).unwrap(), vec![1.0]);
    load_events.recv_timeout(Duration::from_secs(1)).unwrap();
    drop_events.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(worker);
    thread.join().unwrap();
}

#[test]
fn failed_load_answers_the_waiter_and_allows_explicit_retry() {
    let (worker, receivers) = channels();
    let thread = std::thread::spawn(move || {
        run(
            receivers,
            Duration::ZERO,
            |_| Err::<(), _>("offline".into()),
            |_, _| unreachable!(),
        )
    });
    assert_eq!(worker.embed(L12, "one", false).unwrap_err(), "offline");
    assert_eq!(worker.embed(L12, "two", false).unwrap_err(), "offline");
    drop(worker);
    thread.join().unwrap();
}

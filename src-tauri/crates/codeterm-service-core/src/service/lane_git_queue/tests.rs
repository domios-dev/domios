use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn manual_queue() -> (LaneGitQueue, Arc<Mutex<Vec<LaneGitJob>>>) {
    let drains: Arc<Mutex<Vec<LaneGitJob>>> = Default::default();
    let captured = drains.clone();
    let queue = LaneGitQueue::new(move |drain| captured.lock().push(drain));
    (queue, drains)
}

fn run_drains(drains: &Mutex<Vec<LaneGitJob>>) {
    let pending: Vec<_> = drains.lock().drain(..).collect();
    for drain in pending {
        drain();
    }
}

fn job(log: &Arc<Mutex<Vec<&'static str>>>, name: &'static str) -> LaneGitJob {
    let log = log.clone();
    Box::new(move || log.lock().push(name))
}

#[test]
fn lane_git_queue_runs_one_drain_per_repository_in_order() {
    let (queue, drains) = manual_queue();
    let log = Arc::new(Mutex::new(Vec::new()));

    assert_eq!(
        queue.enqueue("/repo", "a", job(&log, "a")),
        Enqueued::Started
    );
    assert_eq!(
        queue.enqueue("/repo", "b", job(&log, "b")),
        Enqueued::Queued
    );
    assert_eq!(
        queue.enqueue("/repo/", "c", job(&log, "c")),
        Enqueued::Queued
    );
    assert_eq!(drains.lock().len(), 1, "a burst must start a single drain");

    run_drains(&drains);
    assert_eq!(*log.lock(), ["a", "b", "c"]);
    assert_eq!(queue.pending("/repo"), 0);

    assert_eq!(
        queue.enqueue("/repo", "d", job(&log, "d")),
        Enqueued::Started
    );
    assert_eq!(
        drains.lock().len(),
        1,
        "an idle repository starts a new drain"
    );
}

#[test]
fn lane_git_queue_keeps_repositories_independent() {
    let (queue, drains) = manual_queue();
    let log = Arc::new(Mutex::new(Vec::new()));
    assert_eq!(
        queue.enqueue("/one", "a", job(&log, "a")),
        Enqueued::Started
    );
    assert_eq!(
        queue.enqueue("/two", "a", job(&log, "b")),
        Enqueued::Started
    );
    assert_eq!(drains.lock().len(), 2);
}

#[test]
fn lane_git_queue_coalesces_duplicate_keys_and_bounds_pending_work() {
    let (queue, drains) = manual_queue();
    let runs = Arc::new(AtomicUsize::new(0));
    let counted = |runs: &Arc<AtomicUsize>| -> LaneGitJob {
        let runs = runs.clone();
        Box::new(move || {
            runs.fetch_add(1, Ordering::SeqCst);
        })
    };

    assert_eq!(
        queue.enqueue("/repo", "lane", counted(&runs)),
        Enqueued::Started
    );
    assert_eq!(
        queue.enqueue("/repo", "lane", counted(&runs)),
        Enqueued::Duplicate
    );
    for index in 1..MAX_PENDING_PER_REPOSITORY {
        assert_eq!(
            queue.enqueue("/repo", format!("lane-{index}"), counted(&runs)),
            Enqueued::Queued
        );
    }
    assert_eq!(
        queue.enqueue("/repo", "overflow", counted(&runs)),
        Enqueued::Full
    );

    run_drains(&drains);
    assert_eq!(runs.load(Ordering::SeqCst), MAX_PENDING_PER_REPOSITORY);
}

#[test]
fn lane_git_queue_survives_a_panicking_job() {
    let (queue, drains) = manual_queue();
    let log = Arc::new(Mutex::new(Vec::new()));
    queue.enqueue("/repo", "boom", Box::new(|| panic!("job failed")));
    queue.enqueue("/repo", "after", job(&log, "after"));
    run_drains(&drains);
    assert_eq!(*log.lock(), ["after"]);
    assert_eq!(
        queue.enqueue("/repo", "next", job(&log, "next")),
        Enqueued::Started
    );
}

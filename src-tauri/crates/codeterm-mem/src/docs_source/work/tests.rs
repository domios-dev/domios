use super::*;

#[test]
fn docs_work_counts_backlog_and_flight_and_cleans_up_on_failure() {
    let counts = Arc::new(Mutex::new(Counts::default()));
    let mut work = BuildWork::with_counts(counts.clone(), 3);
    work.skip();
    let flight = work.start();
    {
        let counts = counts.lock();
        assert_eq!((counts.queued, counts.in_flight), (1, 1));
    }
    drop(flight);
    assert_eq!(counts.lock().in_flight, 0);
    drop(work);
    assert_eq!(counts.lock().queued, 0);
}

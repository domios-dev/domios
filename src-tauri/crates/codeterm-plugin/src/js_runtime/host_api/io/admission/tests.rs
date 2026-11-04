use super::*;
use std::sync::atomic::AtomicUsize;

type Started = Arc<Mutex<Vec<(String, Permit)>>>;

fn submit(admission: &Arc<Admission>, started: &Started, plugin: &str, label: &str) {
    let started = started.clone();
    let label = label.to_string();
    admission
        .submit(
            plugin,
            ticket(),
            Box::new(move |permit| started.lock().push((label, permit))),
        )
        .unwrap();
}

fn labels(started: &Started) -> Vec<String> {
    started
        .lock()
        .iter()
        .map(|(label, _)| label.clone())
        .collect()
}

fn finish(started: &Started, label: &str) {
    let permit = {
        let mut running = started.lock();
        let index = running.iter().position(|(held, _)| held == label).unwrap();
        running.remove(index).1
    };
    drop(permit);
}

struct Capture(Arc<AtomicUsize>);
impl Drop for Capture {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn captured_launch(dropped: &Arc<AtomicUsize>) -> Launch {
    let capture = Capture(dropped.clone());
    Box::new(move |_permit| drop(capture))
}

#[test]
fn saturated_plugin_queues_jobs_and_starts_them_in_submission_order() {
    let admission = Arc::new(Admission::new(16, 2, 8));
    let started = Started::default();
    for label in ["a1", "a2", "a3", "a4", "a5"] {
        submit(&admission, &started, "a", label);
    }
    submit(&admission, &started, "b", "b1");
    assert_eq!(labels(&started), ["a1", "a2", "b1"]);

    finish(&started, "a2");
    finish(&started, "a1");
    assert_eq!(labels(&started), ["b1", "a3", "a4"]);
    finish(&started, "a4");
    finish(&started, "a3");
    assert_eq!(labels(&started), ["b1", "a5"]);
    finish(&started, "a5");
    finish(&started, "b1");
    let state = admission.state.lock();
    assert_eq!(state.running, 0);
    assert!(state.active.is_empty() && state.queued.is_empty());
}

#[test]
fn global_capacity_admits_the_oldest_eligible_job_across_plugins() {
    let admission = Arc::new(Admission::new(2, 2, 8));
    let started = Started::default();
    for (plugin, label) in [
        ("a", "a1"),
        ("a", "a2"),
        ("b", "b1"),
        ("a", "a3"),
        ("b", "b2"),
    ] {
        submit(&admission, &started, plugin, label);
    }
    assert_eq!(labels(&started), ["a1", "a2"]);

    finish(&started, "a1");
    assert_eq!(labels(&started), ["a2", "b1"]);
    finish(&started, "b1");
    assert_eq!(labels(&started), ["a2", "a3"]);
    finish(&started, "a2");
    assert_eq!(labels(&started), ["a3", "b2"]);
}

#[test]
fn a_job_submitted_after_release_queues_behind_earlier_jobs_of_its_plugin() {
    let admission = Arc::new(Admission::new(4, 1, 8));
    let started = Started::default();
    for label in ["a1", "a2", "a3"] {
        submit(&admission, &started, "a", label);
    }
    finish(&started, "a1");
    submit(&admission, &started, "a", "a4");
    assert_eq!(labels(&started), ["a2"]);
    finish(&started, "a2");
    assert_eq!(labels(&started), ["a3"]);
    finish(&started, "a3");
    assert_eq!(labels(&started), ["a4"]);
}

#[test]
fn cancel_and_resubmit_cycles_never_accumulate_queued_captures_behind_held_capacity() {
    let admission = Arc::new(Admission::new(16, 1, 2));
    let started = Started::default();
    submit(&admission, &started, "a", "held");
    let dropped = Arc::new(AtomicUsize::new(0));
    for cycle in 0..100 {
        let queued = ticket();
        admission
            .submit("a", queued, captured_launch(&dropped))
            .unwrap();
        assert_eq!(admission.queued("a"), 1);
        admission.withdraw(queued);
        assert_eq!(admission.queued("a"), 0);
        assert_eq!(dropped.load(Ordering::SeqCst), cycle + 1);
    }
    assert_eq!(labels(&started), ["held"]);
    finish(&started, "held");
    assert_eq!(dropped.load(Ordering::SeqCst), 100);
    assert_eq!(admission.state.lock().running, 0);
}

#[test]
fn a_full_plugin_queue_refuses_and_drops_the_launch_without_blocking_other_plugins() {
    let admission = Arc::new(Admission::new(16, 1, 2));
    let started = Started::default();
    for label in ["a1", "a2", "a3"] {
        submit(&admission, &started, "a", label);
    }
    let dropped = Arc::new(AtomicUsize::new(0));
    assert!(admission
        .submit("a", ticket(), captured_launch(&dropped))
        .is_err());
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(admission.queued("a"), 2);
    submit(&admission, &started, "b", "b1");
    assert_eq!(labels(&started), ["a1", "b1"]);
}

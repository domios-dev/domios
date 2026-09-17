use std::collections::VecDeque;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use parking_lot::{Condvar, Mutex};

use super::{DiagnosticsCore, DiagnosticsRequest, LaneDiagnosticsSummary};

type Completion = Box<dyn FnOnce(Result<LaneDiagnosticsSummary, String>) + Send + 'static>;

struct Job {
    request: DiagnosticsRequest,
    completion: Completion,
}

#[derive(Default)]
struct QueueState {
    jobs: VecDeque<Job>,
    closed: bool,
}

struct Queue {
    state: Mutex<QueueState>,
    ready: Condvar,
}

pub(super) struct Scheduler {
    queue: Arc<Queue>,
    worker: Option<JoinHandle<()>>,
}

impl Scheduler {
    pub(super) fn new(core: Arc<DiagnosticsCore>) -> Self {
        let queue = Arc::new(Queue {
            state: Mutex::new(QueueState::default()),
            ready: Condvar::new(),
        });
        let worker_queue = Arc::clone(&queue);
        let worker = thread::Builder::new()
            .name("codeterm-lane-diagnostics".into())
            .spawn(move || run_queue(worker_queue, core))
            .expect("start lane diagnostics worker");
        Self {
            queue,
            worker: Some(worker),
        }
    }

    pub(super) fn enqueue<F>(&self, request: DiagnosticsRequest, completion: F) -> bool
    where
        F: FnOnce(Result<LaneDiagnosticsSummary, String>) + Send + 'static,
    {
        let mut state = self.queue.state.lock();
        if state.closed {
            return false;
        }
        state.jobs.retain(|job| {
            job.request.plan_id != request.plan_id || job.request.task_id != request.task_id
        });
        state.jobs.push_back(Job {
            request,
            completion: Box::new(completion),
        });
        self.queue.ready.notify_one();
        true
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        {
            let mut state = self.queue.state.lock();
            state.closed = true;
            state.jobs.clear();
            self.queue.ready.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_queue(queue: Arc<Queue>, core: Arc<DiagnosticsCore>) {
    loop {
        let job = {
            let mut state = queue.state.lock();
            loop {
                if state.closed {
                    return;
                }
                if let Some(job) = state.jobs.pop_front() {
                    break job;
                }
                queue.ready.wait(&mut state);
            }
        };
        let result = core.run(job.request);
        (job.completion)(result);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{DiagnosticsRequest, LaneDiagnosticsService};
    use codeterm_types::plan::LaneDiagnosticsSummary;
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    fn request(attempt_id: &str) -> DiagnosticsRequest {
        DiagnosticsRequest {
            plan_id: "plan".into(),
            task_id: "task".into(),
            attempt_id: attempt_id.into(),
            canonical_project_path: "canonical".into(),
            lane_worktree_path: "lane".into(),
            lane_marker: Some("lane".into()),
            lane_branch: None,
            base: None,
            sha: None,
            changed_files: Vec::new(),
            reporter_tab_id: "worker".into(),
            require_lane_marker: false,
        }
    }

    fn clean_summary(request: &DiagnosticsRequest) -> LaneDiagnosticsSummary {
        LaneDiagnosticsSummary::from_findings(
            request.plan_id.clone(),
            request.task_id.clone(),
            request.attempt_id.clone(),
            "warm".into(),
            vec!["fake".into()],
            Vec::new(),
        )
    }

    #[test]
    fn lane_diagnostics_enqueue_returns_before_blocked_runner() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = std::sync::Mutex::new(release_rx);
        let runner = Arc::new(move |request: DiagnosticsRequest, _: Duration| {
            started_tx.send(request.attempt_id.clone()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
            Ok(clean_summary(&request))
        });
        let service = LaneDiagnosticsService::with_runner_for_test(
            tempfile::tempdir().unwrap().path().to_path_buf(),
            runner,
        );
        let (done_tx, done_rx) = mpsc::channel();
        assert!(service.enqueue(request("attempt-1"), move |result| {
            done_tx.send(result.unwrap().attempt_id).unwrap();
        }));
        assert_eq!(started_rx.recv().unwrap(), "attempt-1");
        release_tx.send(()).unwrap();
        assert_eq!(done_rx.recv().unwrap(), "attempt-1");
    }

    #[test]
    fn lane_diagnostics_newer_queued_report_supersedes_older() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = std::sync::Mutex::new(release_rx);
        let runner = Arc::new(move |request: DiagnosticsRequest, _: Duration| {
            started_tx.send(request.attempt_id.clone()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
            Ok(clean_summary(&request))
        });
        let service = LaneDiagnosticsService::with_runner_for_test(
            tempfile::tempdir().unwrap().path().to_path_buf(),
            runner,
        );
        let (done_tx, done_rx) = mpsc::channel();
        for attempt in ["attempt-1", "attempt-2", "attempt-3"] {
            let done_tx = done_tx.clone();
            assert!(service.enqueue(request(attempt), move |result| {
                done_tx.send(result.unwrap().attempt_id).unwrap();
            }));
            if attempt == "attempt-1" {
                assert_eq!(started_rx.recv().unwrap(), "attempt-1");
            }
        }
        release_tx.send(()).unwrap();
        assert_eq!(done_rx.recv().unwrap(), "attempt-1");
        assert_eq!(started_rx.recv().unwrap(), "attempt-3");
        release_tx.send(()).unwrap();
        assert_eq!(done_rx.recv().unwrap(), "attempt-3");
        assert!(done_rx.try_recv().is_err());
    }
}

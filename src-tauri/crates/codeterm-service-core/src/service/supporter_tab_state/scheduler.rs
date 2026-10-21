use std::collections::{HashSet, VecDeque};
#[cfg(test)]
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};

use super::ShadowVerdict;
use parking_lot::{Condvar, Mutex};

const WORKERS: usize = 4;
const QUEUE_SIZE: usize = 32;
pub(crate) const MAX_QUESTIONS: usize = 6;

type Work = Box<dyn FnOnce() -> Option<ShadowVerdict> + Send>;
pub(super) type Completion = Box<dyn FnOnce(Option<ShadowVerdict>) + Send>;

struct Job {
    tab: String,
    critical: bool,
    work: Work,
    completion: Option<Completion>,
    #[cfg(test)]
    result: mpsc::Sender<std::thread::Result<Option<ShadowVerdict>>>,
}

impl Job {
    fn cancel(self) {
        if let Some(completion) = self.completion {
            completion(None);
        }
        #[cfg(test)]
        let _ = self.result.send(Ok(None));
    }
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    tabs: HashSet<String>,
    closed: bool,
    coalesced: u64,
    dropped: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Status {
    pub(crate) active: usize,
    pub(crate) queued: usize,
    pub(crate) coalesced: u64,
    pub(crate) dropped: u64,
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
}

pub(crate) struct Scheduler(Arc<Shared>);

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.close();
    }
}

pub(crate) struct ObservationHandle {
    #[cfg(test)]
    result: mpsc::Receiver<std::thread::Result<Option<ShadowVerdict>>>,
}

#[cfg(test)]
impl ObservationHandle {
    pub fn join(self) -> std::thread::Result<Option<ShadowVerdict>> {
        self.result.recv().unwrap_or(Ok(None))
    }
}

static QUEUE: OnceLock<Scheduler> = OnceLock::new();

pub(crate) fn global() -> &'static Scheduler {
    QUEUE.get_or_init(Scheduler::new)
}

pub(crate) fn status() -> serde_json::Value {
    let snapshot = QUEUE.get().map(Scheduler::status);
    serde_json::json!({ "questions_per_event": MAX_QUESTIONS, "global_in_flight": WORKERS,
        "per_tab_in_flight": 1, "queue_limit": QUEUE_SIZE,
        "active": snapshot.map_or(0, |s| s.active), "queued": snapshot.map_or(0, |s| s.queued),
        "coalesced": snapshot.map_or(0, |s| s.coalesced), "dropped": snapshot.map_or(0, |s| s.dropped) })
}

impl Scheduler {
    pub(crate) fn new() -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            ready: Condvar::new(),
        });
        for index in 0..WORKERS {
            let worker = shared.clone();
            if std::thread::Builder::new()
                .name(format!("tab-observer-{index}"))
                .spawn(move || run(worker))
                .is_err()
            {
                shared.queue.lock().closed = true;
                shared.ready.notify_all();
                break;
            }
        }
        Self(shared)
    }

    pub fn submit(
        &self,
        tab: String,
        critical: bool,
        questions: usize,
        work: impl FnOnce() -> Option<ShadowVerdict> + Send + 'static,
    ) -> Option<ObservationHandle> {
        self.submit_with_completion(tab, critical, questions, work, None)
    }

    pub(crate) fn submit_with_completion(
        &self,
        tab: String,
        critical: bool,
        questions: usize,
        work: impl FnOnce() -> Option<ShadowVerdict> + Send + 'static,
        completion: Option<Completion>,
    ) -> Option<ObservationHandle> {
        if questions == 0 || questions > MAX_QUESTIONS {
            return None;
        }
        #[cfg(test)]
        let (tx, rx) = mpsc::channel();
        let mut cancelled = Vec::new();
        let mut queue = self.0.queue.lock();
        if queue.closed {
            return None;
        }
        if !critical {
            if let Some(index) = queue
                .jobs
                .iter()
                .position(|job| job.tab == tab && !job.critical)
            {
                if let Some(old) = queue.jobs.remove(index) {
                    cancelled.push(old);
                }
                queue.coalesced += 1;
            }
        }
        if queue.jobs.len() == QUEUE_SIZE {
            if critical {
                let index = queue.jobs.iter().position(|job| !job.critical)?;
                if let Some(old) = queue.jobs.remove(index) {
                    cancelled.push(old);
                }
                queue.dropped += 1;
            } else {
                queue.dropped += 1;
                return None;
            }
        }
        queue.jobs.push_back(Job {
            tab,
            critical,
            work: Box::new(work),
            completion,
            #[cfg(test)]
            result: tx,
        });
        drop(queue);
        for job in cancelled {
            job.cancel();
        }
        self.0.ready.notify_all();
        Some(ObservationHandle {
            #[cfg(test)]
            result: rx,
        })
    }

    pub fn status(&self) -> Status {
        let queue = self.0.queue.lock();
        Status {
            active: queue.tabs.len(),
            queued: queue.jobs.len(),
            coalesced: queue.coalesced,
            dropped: queue.dropped,
        }
    }

    pub(crate) fn close(&self) {
        let mut queue = self.0.queue.lock();
        queue.closed = true;
        let cancelled: Vec<_> = queue.jobs.drain(..).collect();
        drop(queue);
        for job in cancelled {
            job.cancel();
        }
        self.0.ready.notify_all();
    }
}

fn run(shared: Arc<Shared>) {
    loop {
        let job = {
            let mut queue = shared.queue.lock();
            loop {
                if queue.closed {
                    return;
                }
                if let Some(index) = queue
                    .jobs
                    .iter()
                    .position(|job| !queue.tabs.contains(&job.tab))
                {
                    if let Some(job) = queue.jobs.remove(index) {
                        queue.tabs.insert(job.tab.clone());
                        break job;
                    }
                }
                shared.ready.wait(&mut queue);
            }
        };
        let result = {
            let _role =
                codeterm_util::named_thread::Scope::enter(&format!("tab-observer:{}", job.tab));
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(job.work))
        };
        if let Some(completion) = job.completion {
            let completed = match &result {
                Ok(verdict) => verdict.clone(),
                Err(_) => None,
            };
            completion(completed);
        }
        shared.queue.lock().tabs.remove(&job.tab);
        #[cfg(test)]
        let _ = job.result.send(result);
        #[cfg(not(test))]
        drop(result);
        shared.ready.notify_all();
    }
}

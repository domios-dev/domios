//! Background lane git work: one job at a time per repository, bounded, never a parallel burst.

use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

pub const MAX_PENDING_PER_REPOSITORY: usize = 64;

pub type LaneGitJob = Box<dyn FnOnce() + Send>;
type Spawner = Arc<dyn Fn(LaneGitJob) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enqueued {
    Started,
    Queued,
    Duplicate,
    Full,
}

#[derive(Default)]
struct RepositoryQueue {
    jobs: VecDeque<(String, LaneGitJob)>,
    draining: bool,
}

#[derive(Clone)]
pub struct LaneGitQueue {
    repositories: Arc<Mutex<HashMap<String, RepositoryQueue>>>,
    spawn: Spawner,
}

impl LaneGitQueue {
    pub fn new(spawn: impl Fn(LaneGitJob) + Send + Sync + 'static) -> Self {
        Self {
            repositories: Default::default(),
            spawn: Arc::new(spawn),
        }
    }

    pub fn background() -> Self {
        Self::new(|drain| {
            crate::async_runtime::spawn_blocking(drain);
        })
    }

    /// A pending job with the same key already covers this request; it reads state when it runs.
    pub fn enqueue(&self, repository: &str, key: impl Into<String>, job: LaneGitJob) -> Enqueued {
        let repository = repository_key(repository);
        let key = key.into();
        let mut repositories = self.repositories.lock();
        let queue = repositories.entry(repository.clone()).or_default();
        if queue.jobs.iter().any(|(pending, _)| *pending == key) {
            return Enqueued::Duplicate;
        }
        if queue.jobs.len() >= MAX_PENDING_PER_REPOSITORY {
            log::warn!("[lane-git-queue] {repository} is full; refused {key}");
            return Enqueued::Full;
        }
        queue.jobs.push_back((key, job));
        if queue.draining {
            return Enqueued::Queued;
        }
        queue.draining = true;
        drop(repositories);
        let owner = self.clone();
        (self.spawn)(Box::new(move || owner.drain(&repository)));
        Enqueued::Started
    }

    pub fn pending(&self, repository: &str) -> usize {
        self.repositories
            .lock()
            .get(&repository_key(repository))
            .map_or(0, |queue| queue.jobs.len())
    }

    fn drain(&self, repository: &str) {
        loop {
            let next = {
                let mut repositories = self.repositories.lock();
                let Some(queue) = repositories.get_mut(repository) else {
                    return;
                };
                match queue.jobs.pop_front() {
                    Some(job) => job,
                    None => {
                        repositories.remove(repository);
                        return;
                    }
                }
            };
            let (key, job) = next;
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                log::warn!("[lane-git-queue] {key} panicked in {repository}");
            }
        }
    }
}

fn repository_key(path: &str) -> String {
    let path = codeterm_paths::strip_verbatim_prefix(path.trim()).replace('\\', "/");
    let path = path.trim_end_matches('/').to_string();
    if cfg!(windows) || cfg!(target_os = "macos") {
        path.to_lowercase()
    } else {
        path
    }
}

#[cfg(test)]
mod tests;

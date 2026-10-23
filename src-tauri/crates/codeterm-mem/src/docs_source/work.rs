use crate::embed_queue::QueueStatus;
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};

#[derive(Default)]
struct Counts {
    queued: usize,
    in_flight: usize,
}

fn counts() -> &'static Arc<Mutex<Counts>> {
    static COUNTS: OnceLock<Arc<Mutex<Counts>>> = OnceLock::new();
    COUNTS.get_or_init(|| Arc::new(Mutex::new(Counts::default())))
}

pub(crate) fn add_status(status: &mut QueueStatus) {
    let counts = counts().lock();
    status.queued += counts.queued;
    status.in_flight += counts.in_flight;
}

pub(super) struct BuildWork {
    counts: Arc<Mutex<Counts>>,
    remaining: usize,
    started: usize,
    finished: usize,
}

impl BuildWork {
    pub(super) fn new(queued: usize) -> Self {
        Self::with_counts(counts().clone(), queued)
    }

    fn with_counts(counts: Arc<Mutex<Counts>>, queued: usize) -> Self {
        counts.lock().queued += queued;
        Self {
            counts,
            remaining: queued,
            started: 0,
            finished: 0,
        }
    }

    pub(super) fn skip(&mut self) {
        self.remaining -= 1;
        self.counts.lock().queued -= 1;
    }

    pub(super) fn start(&mut self) -> Flight {
        self.remaining -= 1;
        self.started += 1;
        let mut counts = self.counts.lock();
        counts.queued -= 1;
        counts.in_flight += 1;
        Flight(self.counts.clone())
    }

    pub(super) fn complete(&mut self) {
        self.finished += 1;
    }
    pub(super) fn started(&self) -> usize {
        self.started
    }
    pub(super) fn finished(&self) -> usize {
        self.finished
    }
}

impl Drop for BuildWork {
    fn drop(&mut self) {
        self.counts.lock().queued -= self.remaining;
    }
}

pub(super) struct Flight(Arc<Mutex<Counts>>);

impl Drop for Flight {
    fn drop(&mut self) {
        self.0.lock().in_flight -= 1;
    }
}

#[cfg(test)]
mod tests;

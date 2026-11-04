use std::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) struct Refresh {
    running: AtomicBool,
}

impl Refresh {
    pub const fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
        }
    }

    pub fn start(
        &'static self,
        refresh: impl Future<Output = ()> + Send + 'static,
    ) -> Option<tokio::task::JoinHandle<()>> {
        if self
            .running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return None;
        }
        // Requests share one refresh task; cancellation of a request cannot discard its result.
        let running = Running(self);
        Some(tokio::spawn(async move {
            let _running = running;
            refresh.await;
        }))
    }
}

struct Running(&'static Refresh);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests;

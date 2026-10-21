use std::time::Instant;

pub(super) struct Route {
    started: Instant,
    pub branch: &'static str,
}

impl Route {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            branch: "preflight",
        }
    }
}

impl Drop for Route {
    fn drop(&mut self) {
        log::debug!(
            "[pty.route] branch={} total_us={}",
            self.branch,
            self.started.elapsed().as_micros()
        );
    }
}

pub(super) async fn blocking<T: Send + 'static>(
    stage: &'static str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, tokio::task::JoinError> {
    let queued = Instant::now();
    tokio::task::spawn_blocking(move || {
        let queue_us = queued.elapsed().as_micros();
        let started = Instant::now();
        let result = work();
        log::debug!(
            "[pty.stage] stage={} queue_us={} work_us={}",
            stage,
            queue_us,
            started.elapsed().as_micros()
        );
        result
    })
    .await
}

use std::sync::mpsc;
use std::time::Duration;

use super::runtime::{shutdown_all, ShutdownReport};
use crate::service::ServiceState;

/// Long enough for Chromium to flush cookies into a named profile.
const EXIT_GRACEFUL: Duration = Duration::from_secs(2);
const EXIT_LIMIT: Duration = Duration::from_secs(4);
const EXIT_WAIT_SLACK: Duration = Duration::from_millis(500);

/// Surface runtimes closing in the background while the rest of the exit sequence runs.
pub(crate) struct SurfaceShutdown(mpsc::Receiver<ShutdownReport>);

/// Starts closing every live surface runtime; safe on any thread, including a runtime worker.
pub(crate) fn begin_surface_shutdown(svc: &ServiceState) -> SurfaceShutdown {
    let runtimes = svc.surface_runtimes.take_all();
    let (done, wait) = mpsc::channel();
    tauri::async_runtime::spawn(async move {
        let _ = done.send(shutdown_all(runtimes, EXIT_GRACEFUL, EXIT_LIMIT).await);
    });
    SurfaceShutdown(wait)
}

impl SurfaceShutdown {
    /// Bounded: shutdown never waits on a browser past `EXIT_LIMIT`.
    pub(crate) fn wait(self) {
        match self.0.recv_timeout(EXIT_LIMIT + EXIT_WAIT_SLACK) {
            Ok(report) => log::info!(
                "[shutdown] browser surfaces closed: {} graceful, {} killed",
                report.graceful,
                report.killed
            ),
            Err(_) => {
                log::warn!("[shutdown] browser surfaces closed: teardown did not report in time")
            }
        }
    }
}

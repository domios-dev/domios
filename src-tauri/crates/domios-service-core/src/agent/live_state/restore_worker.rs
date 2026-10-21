use std::sync::mpsc::{self, SyncSender, TrySendError};

pub(crate) struct RestoreReportWorker {
    requested: SyncSender<()>,
}

impl RestoreReportWorker {
    fn start(mut settle: impl FnMut() -> bool + Send + 'static) -> Self {
        let (requested, changes) = mpsc::sync_channel(1);
        codeterm_util::named_thread::spawn("restore-report", move || {
            while changes.recv().is_ok() {
                if !settle() {
                    break;
                }
            }
        });
        Self { requested }
    }

    fn request(&self) {
        match self.requested.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => {
                log::error!("[restore] report worker is unavailable");
            }
        }
    }
}

impl crate::service::ServiceContext {
    pub(crate) fn schedule_restore_report(&self) {
        self.extensions
            .get_or_init(|| {
                let owner = self.ext::<std::sync::Weak<Self>>().clone();
                RestoreReportWorker::start(move || {
                    let Some(service) = owner.upgrade() else {
                        return false;
                    };
                    let started = std::time::Instant::now();
                    service.settle_restore_report();
                    log::debug!(
                        "[restore] report observation elapsed_us={}",
                        started.elapsed().as_micros()
                    );
                    true
                })
            })
            .request();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn requests_coalesce_without_waiting_and_keep_changes_during_an_observation() {
        let (entered, observed) = mpsc::channel();
        let (release, continue_work) = mpsc::channel();
        let mut passes = 0;
        let worker = RestoreReportWorker::start(move || {
            passes += 1;
            entered.send(passes).unwrap();
            if passes == 1 {
                continue_work.recv().unwrap();
            }
            true
        });
        worker.request();
        assert_eq!(observed.recv_timeout(Duration::from_secs(1)).unwrap(), 1);
        let submitted = std::thread::scope(|scope| {
            let (finished, received) = mpsc::channel();
            let requests = &worker;
            let requester = scope.spawn(move || {
                requests.request();
                requests.request();
                finished.send(()).unwrap();
            });
            let submitted = received.recv_timeout(Duration::from_secs(1));
            release.send(()).unwrap();
            requester.join().unwrap();
            submitted
        });
        submitted.expect("request must not wait for the current observation");
        assert_eq!(observed.recv_timeout(Duration::from_secs(1)).unwrap(), 2);
        drop(worker);
        assert!(matches!(
            observed.recv_timeout(Duration::from_secs(1)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }
}

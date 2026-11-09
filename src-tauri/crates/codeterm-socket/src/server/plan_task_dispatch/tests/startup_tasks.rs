use codeterm_service_core::service::ServiceContext;
use parking_lot::Mutex;
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

#[derive(Default)]
pub(crate) struct StartupTasks {
    tasks: Mutex<Vec<(mpsc::Receiver<()>, JoinHandle<()>)>>,
}

pub(super) struct StartupTasksGuard(Arc<StartupTasks>);

impl StartupTasks {
    pub(super) fn install(service: &ServiceContext) -> StartupTasksGuard {
        let tasks = Arc::new(Self::default());
        assert!(service.register_host_extension(tasks.clone()).is_ok());
        StartupTasksGuard(tasks)
    }

    pub(crate) fn spawn(&self, work: impl FnOnce() + Send + 'static) {
        let (done, finished) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            work();
            let _ = done.send(());
        });
        self.tasks.lock().push((finished, worker));
    }

    fn finish(&self) {
        for (finished, worker) in std::mem::take(&mut *self.tasks.lock()) {
            let completed = finished.recv_timeout(super::super::DISPATCH_READY_TIMEOUT);
            assert!(completed.is_ok(), "dispatch startup must release its Git children before the fixture is removed: {completed:?}");
            worker.join().expect("dispatch startup worker panicked");
        }
    }
}

impl StartupTasksGuard {
    pub(super) fn finish(&self) {
        self.0.finish();
    }
}

impl Drop for StartupTasksGuard {
    fn drop(&mut self) {
        self.0.finish();
    }
}

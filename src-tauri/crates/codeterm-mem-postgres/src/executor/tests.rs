use super::*;
use std::{
    sync::mpsc,
    thread::{self, ThreadId},
};

struct Probe {
    owner: ThreadId,
    dropped: mpsc::Sender<ThreadId>,
}

impl Drop for Probe {
    fn drop(&mut self) {
        assert!(tokio::runtime::Handle::try_current().is_err());
        assert_eq!(thread::current().id(), self.owner);
        self.dropped.send(self.owner).unwrap();
    }
}

fn exercise() {
    let executor = Arc::new(ThreadPoolBuilder::new().num_threads(1).build().unwrap());
    let (dropped, observed) = mpsc::channel();
    let value = executor.install(|| {
        assert!(tokio::runtime::Handle::try_current().is_err());
        Probe {
            owner: thread::current().id(),
            dropped,
        }
    });
    let mut owned = Owned {
        value: Some(value),
        executor,
    };
    let owner = owned.with(|probe| {
        assert!(tokio::runtime::Handle::try_current().is_err());
        assert_eq!(thread::current().id(), probe.owner);
        probe.owner
    });
    assert_ne!(owner, thread::current().id());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _owned = owned;
        panic!("caller unwinds while owning a client");
    }));
    assert!(result.is_err());
    assert_eq!(observed.recv().unwrap(), owner);
}

#[test]
fn executor_keeps_operations_and_unwind_cleanup_on_its_thread() {
    exercise();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            exercise();
        });
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::spawn(async {
                exercise();
            })
            .await
            .unwrap();
        });
}

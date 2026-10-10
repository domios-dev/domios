use super::OwnerThread;
use std::{
    cell::Cell,
    rc::Rc,
    sync::{mpsc, Arc, Barrier, Mutex},
    thread::{self, ThreadId},
};

#[derive(Clone)]
struct NativeOwner {
    count: Rc<Cell<usize>>,
    drops: Arc<Mutex<Vec<ThreadId>>>,
}

// Mirrors the upstream context contract; workers in these tests only clone/drop the holder.
unsafe impl Send for NativeOwner {}
unsafe impl Sync for NativeOwner {}

impl Drop for NativeOwner {
    fn drop(&mut self) {
        self.drops.lock().unwrap().push(thread::current().id());
    }
}

fn native_owner() -> (Arc<OwnerThread<NativeOwner>>, Arc<Mutex<Vec<ThreadId>>>) {
    let drops = Arc::new(Mutex::new(Vec::new()));
    let owner = Arc::new(OwnerThread::new(NativeOwner {
        count: Rc::new(Cell::new(0)),
        drops: drops.clone(),
    }));
    (owner, drops)
}

#[test]
fn windows_rc_ownership_regression_concurrent_clones_preserve_native_count() {
    let (owner, drops) = native_owner();
    let begin = Arc::new(Barrier::new(5));
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let handle = owner.clone();
            let begin = begin.clone();
            thread::spawn(move || {
                begin.wait();
                for _ in 0..128 {
                    let clone = handle.clone();
                    let _ = format!("{clone:?}");
                    drop(clone);
                }
            })
        })
        .collect();
    begin.wait();
    for _ in 0..128 {
        assert_eq!(Rc::strong_count(&owner.count), 1);
        let native_clone = owner.count.clone();
        native_clone.set(native_clone.get() + 1);
        assert_eq!(Rc::strong_count(&owner.count), 2);
        drop(native_clone);
    }
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(owner.count.get(), 128);
    assert_eq!(Rc::strong_count(&owner.count), 1);
    drop(owner);
    assert_eq!(*drops.lock().unwrap(), [thread::current().id()]);
}

#[test]
fn windows_rc_ownership_regression_late_worker_final_drop_retains_native_payload() {
    let (owner, drops) = native_owner();
    let ready = Arc::new(Barrier::new(2));
    let disposed = Arc::new(Barrier::new(2));
    let worker = {
        let handle = owner.clone();
        let ready = ready.clone();
        let disposed = disposed.clone();
        thread::spawn(move || {
            ready.wait();
            disposed.wait();
            drop(handle.clone());
            drop(handle);
        })
    };
    ready.wait();
    drop(owner);
    assert!(drops.lock().unwrap().is_empty());
    disposed.wait();
    worker.join().unwrap();
    assert!(drops.lock().unwrap().is_empty());
}

#[test]
fn windows_rc_ownership_regression_worker_returns_final_owner_for_native_disposal() {
    let (owner, drops) = native_owner();
    let returned = thread::spawn(move || {
        drop(owner.clone());
        owner
    })
    .join()
    .unwrap();
    assert!(drops.lock().unwrap().is_empty());
    assert_eq!(Rc::strong_count(&returned.count), 1);
    drop(returned);
    assert_eq!(*drops.lock().unwrap(), [thread::current().id()]);
}

#[test]
fn windows_rc_ownership_regression_worker_unwind_retains_native_payload() {
    let (owner, drops) = native_owner();
    let worker = thread::spawn(move || {
        let _final_handle = owner;
        panic!("controlled worker unwind");
    });
    assert!(worker.join().is_err());
    assert!(drops.lock().unwrap().is_empty());
}

#[derive(Clone)]
struct ContextOwnership<C> {
    main_thread: C,
}

fn assert_context_clone_keeps_native_count<C: Clone + Send + 'static>(
    wrap: impl FnOnce(NativeOwner) -> C,
) {
    let count = Rc::new(Cell::new(0));
    let context = ContextOwnership {
        main_thread: wrap(NativeOwner {
            count: count.clone(),
            drops: Arc::new(Mutex::new(Vec::new())),
        }),
    };
    let before = Rc::strong_count(&count);
    let (cloned_tx, cloned_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let clone = context.clone();
        let _ = &clone.main_thread;
        cloned_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        drop(clone);
        context
    });
    cloned_rx.recv().unwrap();
    let during = Rc::strong_count(&count);
    release_tx.send(()).unwrap();
    let returned = worker.join().unwrap();
    let after = Rc::strong_count(&count);
    drop(returned);
    assert_eq!(Rc::strong_count(&count), 1);
    assert_eq!(after, before);
    assert_eq!(
        during, before,
        "worker Context clone changed native Rc count"
    );
}

#[test]
fn windows_rc_ownership_regression_context_field_shared_count_invariant() {
    assert_context_clone_keeps_native_count(|native| Arc::new(OwnerThread::new(native)));
}

#[test]
fn windows_rc_ownership_regression_context_field_by_value_count_invariant_fails() {
    let result =
        std::panic::catch_unwind(|| assert_context_clone_keeps_native_count(|native| native));
    let failure = result.expect_err("pinned by-value dispatcher clone must violate the invariant");
    let message = failure.downcast_ref::<String>().unwrap();
    assert!(message.contains("worker Context clone changed native Rc count"));
}

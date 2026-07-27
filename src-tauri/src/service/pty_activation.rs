use parking_lot::{Condvar, Mutex};
use std::collections::HashSet;
use std::sync::OnceLock;

struct ActivationGates {
    active: Mutex<HashSet<String>>,
    changed: Condvar,
}

fn gates() -> &'static ActivationGates {
    static GATES: OnceLock<ActivationGates> = OnceLock::new();
    GATES.get_or_init(|| ActivationGates {
        active: Mutex::new(HashSet::new()),
        changed: Condvar::new(),
    })
}

pub struct PaneActivationPermit {
    pane_id: String,
}

pub fn acquire(pane_id: &str) -> PaneActivationPermit {
    acquire_inner(pane_id, || {})
}

fn acquire_inner(pane_id: &str, mut waiting: impl FnMut()) -> PaneActivationPermit {
    let gates = gates();
    let mut active = gates.active.lock();
    while !active.insert(pane_id.to_string()) {
        waiting();
        gates.changed.wait(&mut active);
    }
    PaneActivationPermit {
        pane_id: pane_id.to_string(),
    }
}

#[cfg(test)]
pub fn acquire_with_wait_hook(pane_id: &str, waiting: impl FnMut()) -> PaneActivationPermit {
    acquire_inner(pane_id, waiting)
}

pub async fn acquire_async(pane_id: String) -> Result<PaneActivationPermit, String> {
    tokio::task::spawn_blocking(move || acquire(&pane_id))
        .await
        .map_err(|error| format!("Tab activation waiter failed: {error}"))
}

impl Drop for PaneActivationPermit {
    fn drop(&mut self) {
        let gates = gates();
        gates.active.lock().remove(&self.pane_id);
        gates.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::acquire;
    use std::sync::mpsc;

    #[test]
    fn same_pane_waits_for_the_active_transition() {
        let first = acquire("activation-serial");
        let (attempt_tx, attempt_rx) = mpsc::channel();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            attempt_tx.send(()).unwrap();
            let _second = acquire("activation-serial");
            acquired_tx.send(()).unwrap();
        });

        attempt_rx.recv().unwrap();
        assert!(matches!(
            acquired_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        drop(first);
        acquired_rx.recv().unwrap();
        waiter.join().unwrap();
    }

    #[test]
    fn different_panes_do_not_block_each_other() {
        let _first = acquire("activation-a");
        let _second = acquire("activation-b");
    }
}

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct WorkerAckIdentity {
    pub(crate) plan_id: String,
    pub(crate) task_id: String,
    pub(crate) attempt_id: String,
    pub(crate) tab_id: String,
    pub(crate) session_id: Option<String>,
    pub(crate) binding_nonce: Option<String>,
}

pub(crate) struct WorkerAckArm {
    pub(crate) receiver: Option<Receiver<WorkerAckSignal>>,
    pub(crate) starts_supervisor: bool,
    pub(crate) identity_conflict: bool,
}

#[derive(Default)]
pub(super) struct WorkerAckState {
    waiters: HashMap<WorkerAckIdentity, WorkerAckWaiter>,
    by_tab: HashMap<String, WorkerAckIdentity>,
    pending_holds: HashSet<WorkerAckIdentity>,
}

impl WorkerAckWaiter {
    fn notify(&self, signal: WorkerAckSignal) -> bool {
        self.observers.iter().fold(false, |sent, observer| {
            observer.send(signal).is_ok() || sent
        })
    }
}

fn same_startup_hold(left: &WorkerAckIdentity, right: &WorkerAckIdentity) -> bool {
    left.plan_id == right.plan_id
        && left.task_id == right.task_id
        && left.attempt_id == right.attempt_id
        && left.tab_id == right.tab_id
        && left.binding_nonce == right.binding_nonce
        && (left.binding_nonce.is_some() || left.session_id == right.session_id)
}

impl WorkerAckRegistry {
    pub(crate) fn arm(&self, identity: WorkerAckIdentity) -> WorkerAckArm {
        let (sender, receiver) = mpsc::channel();
        let previous = {
            let mut state = self.waiters.lock();
            let current = state.by_tab.get(&identity.tab_id).cloned();
            if current.as_ref() == Some(&identity) {
                return WorkerAckArm {
                    receiver: None,
                    starts_supervisor: false,
                    identity_conflict: false,
                };
            }
            if current.as_ref().is_some_and(|current| {
                current.attempt_id == identity.attempt_id
                    && current.session_id == identity.session_id
                    && current.binding_nonce == identity.binding_nonce
            }) {
                return WorkerAckArm {
                    receiver: None,
                    starts_supervisor: false,
                    identity_conflict: true,
                };
            }
            let previous = current.and_then(|current| state.waiters.remove(&current));
            let held = state
                .pending_holds
                .iter()
                .any(|pending| same_startup_hold(pending, &identity));
            state
                .pending_holds
                .retain(|pending| pending.tab_id != identity.tab_id);
            let _ = state.waiters.insert(
                identity.clone(),
                WorkerAckWaiter {
                    observers: vec![sender],
                },
            );
            let _ = state
                .by_tab
                .insert(identity.tab_id.clone(), identity.clone());
            if held {
                state
                    .waiters
                    .get(&identity)
                    .unwrap()
                    .notify(WorkerAckSignal::StartupHeld);
            }
            previous
        };
        if let Some(previous) = previous {
            previous.notify(WorkerAckSignal::Superseded);
        }
        WorkerAckArm {
            receiver: Some(receiver),
            starts_supervisor: true,
            identity_conflict: false,
        }
    }

    pub(crate) fn acknowledge(&self, identity: &WorkerAckIdentity) -> bool {
        let waiter = {
            let mut state = self.waiters.lock();
            let is_current = state.by_tab.get(&identity.tab_id) == Some(identity);
            if is_current {
                state
                    .pending_holds
                    .retain(|pending| !same_startup_hold(pending, identity));
                let _ = state.by_tab.remove(&identity.tab_id);
                state.waiters.remove(identity)
            } else {
                if !state.by_tab.contains_key(&identity.tab_id) {
                    state.pending_holds.remove(identity);
                }
                None
            }
        };
        waiter.is_some_and(|waiter| waiter.notify(WorkerAckSignal::Acknowledged))
    }

    pub(crate) fn expire(&self, identity: &WorkerAckIdentity) -> bool {
        let mut state = self.waiters.lock();
        if state.by_tab.get(&identity.tab_id) == Some(identity) {
            state
                .pending_holds
                .retain(|pending| !same_startup_hold(pending, identity));
            let _ = state.by_tab.remove(&identity.tab_id);
            let _ = state.waiters.remove(identity);
            true
        } else {
            if !state.by_tab.contains_key(&identity.tab_id) {
                let _ = state.pending_holds.remove(identity);
            }
            false
        }
    }

    pub(crate) fn extend_deadline(&self, identity: &WorkerAckIdentity) -> bool {
        let mut state = self.waiters.lock();
        let current = state.by_tab.get(&identity.tab_id).cloned();
        if let Some(current) = current {
            if same_startup_hold(&current, identity) {
                state
                    .waiters
                    .get(&current)
                    .unwrap()
                    .notify(WorkerAckSignal::StartupHeld);
                state
                    .pending_holds
                    .retain(|pending| !same_startup_hold(pending, identity));
                state.pending_holds.insert(identity.clone());
                return true;
            }
            return false;
        }
        state
            .pending_holds
            .retain(|pending| pending.tab_id != identity.tab_id);
        state.pending_holds.insert(identity.clone())
    }
}

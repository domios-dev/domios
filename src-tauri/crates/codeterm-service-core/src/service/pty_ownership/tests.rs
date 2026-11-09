use super::{LinkOutcome, PtyOwnership, TabPtyClaim};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Mutex};

#[derive(Default)]
struct RecordingState {
    next_id: usize,
    links: HashMap<String, String>,
    live: HashSet<String>,
    killed: Vec<String>,
    commands: Vec<(String, String)>,
    ready: HashSet<String>,
    readers: HashSet<String>,
}

#[derive(Clone, Default)]
struct RecordingCallbacks(Arc<Mutex<RecordingState>>);

impl RecordingCallbacks {
    fn spawn(&self) -> String {
        let mut state = self.0.lock().unwrap();
        state.next_id += 1;
        let id = format!("pty-{}", state.next_id);
        state.live.insert(id.clone());
        id
    }

    fn run(&self, ownership: &PtyOwnership, tab: &str, launch: bool, command: &str) {
        let claim = ownership.acquire(tab).unwrap();
        self.run_claimed(claim, tab, launch, command);
    }

    fn run_claimed(&self, claim: TabPtyClaim, tab: &str, launch: bool, command: &str) {
        let state = self.clone();
        let existing = self.clone();
        let spawned = self.clone();
        let live = self.clone();
        let linked = self.clone();
        let killed = self.clone();
        let ready = self.clone();
        let created = Arc::new(AtomicBool::new(false));
        let created_by_spawn = created.clone();
        let selection = claim.coordinate(
            launch,
            move || {
                let state = existing.0.lock().unwrap();
                state
                    .links
                    .get(tab)
                    .filter(|pty_id| state.live.contains(*pty_id))
                    .cloned()
            },
            move || {
                created_by_spawn.store(true, Ordering::Release);
                let pty_id = spawned.spawn();
                if launch {
                    spawned.0.lock().unwrap().readers.insert(pty_id.clone());
                }
                pty_id
            },
            move |id| live.0.lock().unwrap().live.contains(id),
            move |id| {
                LinkOutcome::Linked(
                    linked
                        .0
                        .lock()
                        .unwrap()
                        .links
                        .insert(tab.to_string(), id.to_string()),
                )
            },
            move |id| {
                let mut state = killed.0.lock().unwrap();
                if state.live.remove(id) {
                    state.killed.push(id.to_string());
                }
            },
            move |id| {
                ready.0.lock().unwrap().ready.insert(tab.to_string());
                let _ = id;
            },
        );
        if launch {
            state
                .0
                .lock()
                .unwrap()
                .commands
                .push((selection.pty_id.clone(), command.to_string()));
        } else if created.load(Ordering::Acquire) && selection.retained_candidate {
            state.0.lock().unwrap().readers.insert(selection.pty_id);
        }
    }

    fn close(&self, ownership: &PtyOwnership, tab: &str) {
        let pty_id = self.0.lock().unwrap().links.remove(tab).unwrap();
        ownership.release_pty(&pty_id);
        let mut state = self.0.lock().unwrap();
        if state.live.remove(&pty_id) {
            state.killed.push(pty_id);
        }
    }
}

fn concurrent_pair(frontend_first: bool) -> (PtyOwnership, RecordingCallbacks) {
    let ownership = PtyOwnership::default();
    let callbacks = RecordingCallbacks::default();
    let start = Arc::new(Barrier::new(5));
    let mut threads = Vec::new();
    for (tab, command) in [("pair-a", "agent-a"), ("pair-b", "agent-b")] {
        let (first_acquired_tx, first_acquired_rx) = std::sync::mpsc::channel();
        let (second_waiting_tx, second_waiting_rx) = std::sync::mpsc::channel();
        let ownership_first = ownership.clone();
        let callbacks_first = callbacks.clone();
        let start_first = start.clone();
        threads.push(std::thread::spawn(move || {
            start_first.wait();
            let claim = ownership_first.acquire(tab).unwrap();
            first_acquired_tx.send(()).unwrap();
            second_waiting_rx.recv().unwrap();
            callbacks_first.run_claimed(
                claim,
                tab,
                !frontend_first,
                if frontend_first { "" } else { command },
            );
        }));

        let ownership_second = ownership.clone();
        let callbacks_second = callbacks.clone();
        let start_second = start.clone();
        threads.push(std::thread::spawn(move || {
            start_second.wait();
            first_acquired_rx.recv().unwrap();
            let mut waiting_tx = Some(second_waiting_tx);
            let claim = ownership_second.acquire_with_wait_hook(tab, move || {
                if let Some(tx) = waiting_tx.take() {
                    tx.send(()).unwrap();
                }
            });
            callbacks_second.run_claimed(
                claim,
                tab,
                frontend_first,
                if frontend_first { command } else { "" },
            );
        }));
    }
    start.wait();
    for thread in threads {
        thread.join().unwrap();
    }
    (ownership, callbacks)
}

#[test]
fn concurrent_pair_spawn_single_pty_per_tab() {
    for frontend_first in [false, true] {
        let (_, callbacks) = concurrent_pair(frontend_first);
        let state = callbacks.0.lock().unwrap();
        assert_eq!(state.links.len(), 2);
        assert_eq!(state.live.len(), 2);
        assert!(state.killed.is_empty());
        assert_eq!(state.commands.len(), 2);
        assert_eq!(state.readers, state.live);
        assert_eq!(
            state.ready,
            HashSet::from(["pair-a".into(), "pair-b".into()])
        );
        assert!(state
            .commands
            .iter()
            .all(|(pty, _)| state.links.values().any(|linked| linked == pty)));
    }
}

#[test]
fn link_pty_reuses_launch_owned_pty() {
    let ownership = PtyOwnership::default();
    let callbacks = RecordingCallbacks::default();
    callbacks.run(&ownership, "reuse", true, "agent");
    callbacks.run(&ownership, "reuse", false, "");
    let state = callbacks.0.lock().unwrap();
    assert_eq!(state.live.len(), 1);
    assert!(state.killed.is_empty());
    assert_eq!(state.readers, state.live);
}

#[test]
fn orphan_sweep_respects_live_claims() {
    let ownership = PtyOwnership::default();
    let callbacks = RecordingCallbacks::default();
    callbacks.run(&ownership, "claimed", true, "agent");
    let claimed = callbacks.0.lock().unwrap().links["claimed"].clone();
    let orphan = callbacks.spawn();
    let permit = ownership.acquire("claimed").unwrap();
    let live = callbacks.clone();
    let linked = callbacks.clone();
    let killed = callbacks.clone();
    let selection = permit.settle(
        orphan.clone(),
        false,
        move |id| live.0.lock().unwrap().live.contains(id),
        move |id| {
            LinkOutcome::Linked(
                linked
                    .0
                    .lock()
                    .unwrap()
                    .links
                    .insert("claimed".into(), id.into()),
            )
        },
        move |id| {
            let mut state = killed.0.lock().unwrap();
            if state.live.remove(id) {
                state.killed.push(id.into());
            }
        },
        |_| {},
    );
    let state = callbacks.0.lock().unwrap();
    assert_eq!(selection.pty_id, claimed);
    assert!(state.live.contains(&claimed));
    assert_eq!(state.killed, vec![orphan]);
}

#[test]
fn true_orphan_still_reaped() {
    let ownership = PtyOwnership::default();
    let callbacks = RecordingCallbacks::default();
    let candidate = callbacks.spawn();
    let permit = ownership.acquire("orphan-tab").unwrap();
    let live = callbacks.clone();
    let killed = callbacks.clone();
    permit.settle(
        candidate.clone(),
        false,
        move |id| live.0.lock().unwrap().live.contains(id),
        move |_| LinkOutcome::TabMissing,
        move |id| {
            let mut state = killed.0.lock().unwrap();
            if state.live.remove(id) {
                state.killed.push(id.into());
            }
        },
        |_| {},
    );
    let state = callbacks.0.lock().unwrap();
    assert!(!state.live.contains(&candidate));
    assert_eq!(state.killed, vec![candidate]);
}

#[test]
fn missing_tab_launch_candidate_is_reaped_without_readiness() {
    let ownership = PtyOwnership::default();
    let callbacks = RecordingCallbacks::default();
    let candidate = callbacks.spawn();
    let permit = ownership.acquire("missing-tab").unwrap();
    let live = callbacks.clone();
    let killed = callbacks.clone();
    let ready = Arc::new(AtomicBool::new(false));
    let ready_callback = ready.clone();
    let selection = permit.settle(
        candidate.clone(),
        true,
        move |id| live.0.lock().unwrap().live.contains(id),
        move |_| LinkOutcome::TabMissing,
        move |id| {
            let mut state = killed.0.lock().unwrap();
            if state.live.remove(id) {
                state.killed.push(id.into());
            }
        },
        move |_| ready_callback.store(true, Ordering::Release),
    );

    assert!(!selection.linked);
    assert!(!ready.load(Ordering::Acquire));
    assert_eq!(callbacks.0.lock().unwrap().killed, vec![candidate]);
}

#[test]
fn ensure_pty_reader_decision_uses_selected_claim() {
    let retained = super::PtySelection {
        pty_id: "retained".into(),
        retained_candidate: true,
        linked: true,
    };
    let reused = super::PtySelection {
        pty_id: "claimed".into(),
        retained_candidate: false,
        linked: true,
    };
    let missing = super::PtySelection {
        pty_id: "reaped".into(),
        retained_candidate: true,
        linked: false,
    };

    assert_eq!(retained.reader_pty_id(), Some("retained"));
    assert_eq!(reused.reader_pty_id(), None);
    assert_eq!(missing.reader_pty_id(), None);
}

#[test]
fn retry_after_failed_pair_spawn_fresh_single_ownership() {
    let ownership = PtyOwnership::default();
    let callbacks = RecordingCallbacks::default();
    callbacks.run(&ownership, "retry", true, "first");
    let first = callbacks.0.lock().unwrap().links["retry"].clone();
    callbacks.close(&ownership, "retry");
    callbacks.run(&ownership, "retry", true, "second");
    let second = callbacks.0.lock().unwrap().links["retry"].clone();
    let state = callbacks.0.lock().unwrap();
    assert_ne!(first, second);
    assert_eq!(state.live, HashSet::from([second]));
    assert_eq!(state.killed, vec![first]);
}

#[test]
fn dead_launch_claim_yields_to_frontend_shell() {
    let ownership = PtyOwnership::default();
    let callbacks = RecordingCallbacks::default();
    callbacks.run(&ownership, "dead-launch", true, "agent");
    let exited = callbacks.0.lock().unwrap().links["dead-launch"].clone();
    callbacks.0.lock().unwrap().live.remove(&exited);
    assert_eq!(ownership.launch_claim("dead-launch"), Some(exited.clone()));

    callbacks.run(&ownership, "dead-launch", false, "shell");

    let state = callbacks.0.lock().unwrap();
    let shell = state.links["dead-launch"].clone();
    assert_ne!(shell, exited);
    assert_eq!(state.live, HashSet::from([shell.clone()]));
    assert!(state.readers.contains(&shell));
    assert_eq!(ownership.launch_claim("dead-launch"), None);
}

#[test]
fn stale_launch_claim_is_replaced_without_reentrant_locking() {
    let ownership = PtyOwnership::default();
    let callbacks = RecordingCallbacks::default();
    callbacks.run(&ownership, "stale", true, "first");
    let stale = callbacks.0.lock().unwrap().links["stale"].clone();
    callbacks.0.lock().unwrap().live.remove(&stale);
    callbacks.run(&ownership, "stale", true, "retry");

    let fresh = callbacks.0.lock().unwrap().links["stale"].clone();
    assert_ne!(stale, fresh);
    assert!(callbacks.0.lock().unwrap().live.contains(&fresh));
}

#[test]
fn pair_cleanup_reaps_exactly_owned() {
    let (ownership, callbacks) = concurrent_pair(false);
    let owned = callbacks.0.lock().unwrap().live.clone();
    callbacks.close(&ownership, "pair-a");
    callbacks.close(&ownership, "pair-b");
    let state = callbacks.0.lock().unwrap();
    assert!(state.live.is_empty());
    assert_eq!(state.killed.iter().cloned().collect::<HashSet<_>>(), owned);
    assert_eq!(state.killed.len(), 2);
}
#[test]
fn model_provider_relink_disposes_the_displaced_pty_exactly_once() {
    use std::cell::RefCell;
    let ownership = PtyOwnership::default();
    let claim = ownership.acquire("same-tab").unwrap();
    let live = RefCell::new(HashSet::from(["old".to_string(), "fresh".to_string()]));
    let linked = RefCell::new(None::<String>);
    let killed = RefCell::new(Vec::new());
    let settle = |candidate: &str| {
        claim.settle(
            candidate.into(),
            true,
            |id| live.borrow().contains(id),
            |id| LinkOutcome::Linked(linked.replace(Some(id.into()))),
            |id| {
                killed.borrow_mut().push(id.to_string());
                assert!(live.borrow_mut().remove(id), "disposal must run once");
            },
            |_| {},
        )
    };
    assert!(settle("old").linked);
    ownership.release_pty("old");
    let selection = settle("fresh");
    assert!(selection.linked && selection.retained_candidate);
    assert!(settle("fresh").linked);
    assert_eq!(*linked.borrow(), Some("fresh".into()));
    assert_eq!(*killed.borrow(), vec!["old"]);
    assert_eq!(*live.borrow(), HashSet::from(["fresh".to_string()]));
    assert_eq!(ownership.launch_claim("same-tab"), Some("fresh".into()));
}

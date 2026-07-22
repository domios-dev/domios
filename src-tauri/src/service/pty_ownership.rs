use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct PtyOwnership {
    launch_claims: Arc<Mutex<HashMap<String, String>>>,
}

pub struct PanePtyClaim {
    pane_id: String,
    ownership: PtyOwnership,
    _activation: crate::service::pty_activation::PaneActivationPermit,
}

#[derive(Debug, Eq, PartialEq)]
pub struct PtySelection {
    pub pty_id: String,
    pub retained_candidate: bool,
    pub linked: bool,
}

impl PtySelection {
    pub fn reader_pty_id(&self) -> Option<&str> {
        (self.linked && self.retained_candidate).then_some(self.pty_id.as_str())
    }
}

pub enum LinkOutcome {
    Linked(Option<String>),
    PaneMissing,
}

impl PtyOwnership {
    pub fn acquire(&self, pane_id: &str) -> PanePtyClaim {
        PanePtyClaim {
            pane_id: pane_id.to_string(),
            ownership: self.clone(),
            _activation: crate::service::pty_activation::acquire(pane_id),
        }
    }

    pub async fn acquire_async(&self, pane_id: String) -> Result<PanePtyClaim, String> {
        let activation = crate::service::pty_activation::acquire_async(pane_id.clone()).await?;
        Ok(PanePtyClaim {
            pane_id,
            ownership: self.clone(),
            _activation: activation,
        })
    }

    #[cfg(test)]
    fn acquire_with_wait_hook(&self, pane_id: &str, waiting: impl FnMut()) -> PanePtyClaim {
        PanePtyClaim {
            pane_id: pane_id.to_string(),
            ownership: self.clone(),
            _activation: crate::service::pty_activation::acquire_with_wait_hook(pane_id, waiting),
        }
    }

    pub fn launch_claim(&self, pane_id: &str) -> Option<String> {
        self.launch_claims.lock().get(pane_id).cloned()
    }

    pub fn release_pty(&self, pty_id: &str) {
        self.launch_claims
            .lock()
            .retain(|_, claimed| claimed != pty_id);
    }

    pub fn release_ptys(&self, pty_ids: &[String]) {
        if pty_ids.is_empty() {
            return;
        }
        self.launch_claims
            .lock()
            .retain(|_, claimed| !pty_ids.contains(claimed));
    }
}

impl PanePtyClaim {
    pub fn settle(
        &self,
        candidate_pty_id: String,
        launch_owned: bool,
        mut is_live: impl FnMut(&str) -> bool,
        mut link: impl FnMut(&str) -> LinkOutcome,
        mut kill: impl FnMut(&str),
        mut ready: impl FnMut(&str),
    ) -> PtySelection {
        let existing_claim = {
            let claims = self.ownership.launch_claims.lock();
            claims.get(&self.pane_id).cloned()
        };
        let mut claimed = existing_claim.and_then(|claimed| {
            if is_live(&claimed) {
                Some(claimed)
            } else {
                self.ownership.release_pty(&claimed);
                None
            }
        });
        if launch_owned && claimed.is_none() {
            self.ownership
                .launch_claims
                .lock()
                .insert(self.pane_id.clone(), candidate_pty_id.clone());
            claimed = Some(candidate_pty_id.clone());
        }
        let selected = claimed.unwrap_or_else(|| candidate_pty_id.clone());
        let retained_candidate = selected == candidate_pty_id;
        if !retained_candidate {
            kill(&candidate_pty_id);
        }
        let linked = match link(&selected) {
            LinkOutcome::Linked(displaced) => {
                if let Some(displaced) = displaced.filter(|displaced| displaced != &selected) {
                    kill(&displaced);
                }
                true
            }
            LinkOutcome::PaneMissing => {
                self.ownership.release_pty(&selected);
                kill(&selected);
                false
            }
        };
        if linked {
            ready(&selected);
        }
        PtySelection {
            pty_id: selected,
            retained_candidate,
            linked,
        }
    }

    #[cfg(test)]
    fn coordinate(
        &self,
        launch_owned: bool,
        mut existing: impl FnMut() -> Option<String>,
        mut spawn: impl FnMut() -> String,
        is_live: impl FnMut(&str) -> bool,
        link: impl FnMut(&str) -> LinkOutcome,
        kill: impl FnMut(&str),
        ready: impl FnMut(&str),
    ) -> PtySelection {
        let candidate = existing().unwrap_or_else(&mut spawn);
        self.settle(candidate, launch_owned, is_live, link, kill, ready)
    }
}

#[cfg(test)]
mod tests {
    use super::{LinkOutcome, PanePtyClaim, PtyOwnership};
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

        fn run(&self, ownership: &PtyOwnership, pane: &str, launch: bool, command: &str) {
            let claim = ownership.acquire(pane);
            self.run_claimed(claim, pane, launch, command);
        }

        fn run_claimed(&self, claim: PanePtyClaim, pane: &str, launch: bool, command: &str) {
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
                        .get(pane)
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
                            .insert(pane.to_string(), id.to_string()),
                    )
                },
                move |id| {
                    let mut state = killed.0.lock().unwrap();
                    if state.live.remove(id) {
                        state.killed.push(id.to_string());
                    }
                },
                move |id| {
                    ready.0.lock().unwrap().ready.insert(pane.to_string());
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

        fn close(&self, ownership: &PtyOwnership, pane: &str) {
            let pty_id = self.0.lock().unwrap().links.remove(pane).unwrap();
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
        for (pane, command) in [("pair-a", "agent-a"), ("pair-b", "agent-b")] {
            let (first_acquired_tx, first_acquired_rx) = std::sync::mpsc::channel();
            let (second_waiting_tx, second_waiting_rx) = std::sync::mpsc::channel();
            let ownership_first = ownership.clone();
            let callbacks_first = callbacks.clone();
            let start_first = start.clone();
            threads.push(std::thread::spawn(move || {
                start_first.wait();
                let claim = ownership_first.acquire(pane);
                first_acquired_tx.send(()).unwrap();
                second_waiting_rx.recv().unwrap();
                callbacks_first.run_claimed(
                    claim,
                    pane,
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
                let claim = ownership_second.acquire_with_wait_hook(pane, move || {
                    if let Some(tx) = waiting_tx.take() {
                        tx.send(()).unwrap();
                    }
                });
                callbacks_second.run_claimed(
                    claim,
                    pane,
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
    fn concurrent_pair_spawn_single_pty_per_pane() {
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
        let claimed = ownership.launch_claim("claimed").unwrap();
        let orphan = callbacks.spawn();
        let permit = ownership.acquire("claimed");
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
        let permit = ownership.acquire("orphan-pane");
        let live = callbacks.clone();
        let killed = callbacks.clone();
        permit.settle(
            candidate.clone(),
            false,
            move |id| live.0.lock().unwrap().live.contains(id),
            move |_| LinkOutcome::PaneMissing,
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
    fn missing_pane_launch_candidate_is_reaped_without_readiness() {
        let ownership = PtyOwnership::default();
        let callbacks = RecordingCallbacks::default();
        let candidate = callbacks.spawn();
        let permit = ownership.acquire("missing-pane");
        let live = callbacks.clone();
        let killed = callbacks.clone();
        let ready = Arc::new(AtomicBool::new(false));
        let ready_callback = ready.clone();
        let selection = permit.settle(
            candidate.clone(),
            true,
            move |id| live.0.lock().unwrap().live.contains(id),
            move |_| LinkOutcome::PaneMissing,
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
        assert!(ownership.launch_claim("missing-pane").is_none());
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
        let first = ownership.launch_claim("retry").unwrap();
        callbacks.close(&ownership, "retry");
        callbacks.run(&ownership, "retry", true, "second");
        let second = ownership.launch_claim("retry").unwrap();
        let state = callbacks.0.lock().unwrap();
        assert_ne!(first, second);
        assert_eq!(state.live, HashSet::from([second]));
        assert_eq!(state.killed, vec![first]);
    }

    #[test]
    fn stale_launch_claim_is_replaced_without_reentrant_locking() {
        let ownership = PtyOwnership::default();
        let callbacks = RecordingCallbacks::default();
        callbacks.run(&ownership, "stale", true, "first");
        let stale = ownership.launch_claim("stale").unwrap();
        callbacks.0.lock().unwrap().live.remove(&stale);
        callbacks.run(&ownership, "stale", true, "retry");

        let fresh = ownership.launch_claim("stale").unwrap();
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
}

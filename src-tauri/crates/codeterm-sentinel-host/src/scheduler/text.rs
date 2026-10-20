use super::{source, storage, Handle, Sentinel, ServiceState};

impl Handle {
    pub fn enqueue_text(&self, job: &Sentinel, text: String) -> Result<(), String> {
        let mut store = self.store.lock();
        store.ensure_available()?;
        let current = store.get(&job.id).ok_or("blueprint not found")?;
        if current.revision != job.revision
            || current.machine.stopped
            || current.machine.paused
            || current
                .expires_at
                .is_some_and(|at| store.clock.now_ms() >= at)
        {
            return Err("blueprint is not running".into());
        }
        {
            let mut inbox = self.inbox.lock();
            inbox.bind(job);
            inbox.enqueue(&job.id, text)?;
        }
        store.wake_stage(&job.id);
        Ok(())
    }

    pub(crate) fn terminal_output(&self, tab_id: &str, generation: Option<u64>, bytes: &[u8]) {
        let store = self.store.lock();
        if !store.list().iter().any(|job| {
            job.terminal().is_some()
                && !job.machine.stopped
                && matches!(&job.observe().cloned(), Some(source::TabBinding::Tab(id)) if id == tab_id)
        }) {
            return;
        }
        let now = store.clock.now_ms();
        let mut inbox = self.inbox.lock();
        for job in store.list().iter().filter(|job| {
            !job.machine.stopped
                && !job.machine.paused
                && matches!(&job.observe().cloned(), Some(source::TabBinding::Tab(id)) if id == tab_id)
        }) {
            inbox.feed(job, generation, bytes, now);
        }
        drop(inbox);
        drop(store);
        self.notify();
    }

    pub(super) fn prepare_text_events(&self) {
        let mut store = self.store.lock();
        let mut inbox = self.inbox.lock();
        inbox.retain(store.list());
        inbox.settle(store.clock.now_ms());
        for id in inbox.ready() {
            store.wake_stage(&id);
        }
        let failures = inbox.failures();
        drop(inbox);
        drop(store);
        for id in failures {
            let _ = storage::commit(&self.store, |store| {
                store.stage_stop(&id, "terminal_queue_full")
            });
        }
    }

    pub(super) fn reconcile_terminal_sources(&self, svc: &ServiceState) {
        let sources: Vec<_> = self
            .store
            .lock()
            .list()
            .iter()
            .filter(|job| job.terminal().is_some() && !job.machine.stopped)
            .filter_map(|job| {
                let Some(source::TabBinding::Tab(tab)) = &job.observe().cloned() else {
                    return None;
                };
                Some((
                    job.id.clone(),
                    tab.clone(),
                    job.workspace_id.clone(),
                    job.team_id.clone(),
                    job.creator_tab_id.clone(),
                ))
            })
            .collect();
        for (id, tab_id, workspace, team, creator) in sources {
            let valid = {
                let mux = svc.mux.read();
                mux.get_tab(&tab_id).is_some_and(|tab| {
                    (tab.team_id == team
                        || (team.is_none() && creator == tab_id && tab.team_id.is_some()))
                        && mux.find_workspace_for_tab(&tab_id).as_deref()
                            == Some(workspace.as_str())
                })
            };
            if !valid {
                let _ = storage::commit(&self.store, |store| {
                    store.stage_stop(&id, "observed_tab_removed_or_scope_changed")
                });
            }
        }
    }
}

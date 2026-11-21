use super::*;
use super::super::crud::RunFinished;

#[derive(Clone)]
pub struct TerminalRun {
    pub job_id: String,
    pub run_id: String,
    pub status: CronRunStatus,
    pub summary: String,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub tab_id: Option<String>,
}

pub struct TerminalRunCommit {
    request: TerminalRun,
    uncertain: Option<Prepared<RunFinished>>,
    confirmed: Option<RunFinished>,
    change: TerminalChange,
    legacy_pending: Option<RunFinished>,
}

enum TerminalChange { Finish, Report(String), Retire }

impl TerminalRunCommit {
    pub fn new(request: TerminalRun) -> Self {
        Self { request, uncertain: None, confirmed: None, change: TerminalChange::Finish, legacy_pending: None }
    }

    pub fn report(request: TerminalRun, delivery: String) -> Self {
        Self { change: TerminalChange::Report(delivery), ..Self::new(request) }
    }

    pub fn retire(request: TerminalRun) -> Self {
        Self { change: TerminalChange::Retire, ..Self::new(request) }
    }

    pub async fn settle(&mut self, cron: &CronState) -> Result<RunFinished, CronCommitError> {
        self.settle_guarded(cron, || Ok(()), |_, _| Ok(())).await
    }

    pub async fn settle_guarded<G, A>(&mut self, cron: &CronState, mut exclusion: impl FnMut() -> Result<G, String>, mut admission: impl FnMut(&CronStore, &G) -> Result<A, String>) -> Result<RunFinished, CronCommitError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(finished) = self.legacy_pending {
            cron.lock().save().map_err(CronCommitError::Unsaved)?;
            self.legacy_pending = None;
            return Ok(finished);
        }
        #[cfg(not(any(test, feature = "test-support")))]
        if self.legacy_pending.is_some() {
            return Err(CronCommitError::NotApplied("legacy cron backend is test-only".into()));
        }
        if let Some(finished) = self.confirmed {
            reread_settled(cron).await?;
            self.confirmed = None;
            return Ok(finished);
        }
        if self.uncertain.is_some() {
            return self.reconcile(cron).await;
        }
        let storage = cron.lock().commits_to_storage();
        if !storage {
            #[cfg(any(test, feature = "test-support"))]
            {
                let prepared = {
                    let lifecycle = exclusion().map_err(CronCommitError::Refused)?;
                    let mut store = cron.lock();
                    let _admission = admission(&store, &lifecycle).map_err(CronCommitError::Refused)?;
                    let prepared = prepare(&mut store, &self.request, &self.change).map_err(CronCommitError::Refused)?;
                    for change in &prepared.changes { store.apply_change(change); }
                    prepared
                };
                if !prepared.changes.is_empty() {
                    self.legacy_pending = Some(prepared.value);
                    cron.lock().save().map_err(CronCommitError::Unsaved)?;
                    self.legacy_pending = None;
                }
                return Ok(prepared.value);
            }
            #[cfg(not(any(test, feature = "test-support")))]
            return Err(CronCommitError::NotApplied("legacy cron backend is test-only".into()));
        }
        settle_prior(cron).await?;
        for _ in 0..MAX_ATTEMPTS {
            let (prepared, bases, completion, probe) = {
                let lifecycle = exclusion().map_err(CronCommitError::Refused)?;
                let mut store = cron.lock();
                if !store.fence_clear() {
                    return Err(CronCommitError::Fenced("cron terminal admission awaits earlier settlement".into()));
                }
                let _admission = admission(&store, &lifecycle).map_err(CronCommitError::Refused)?;
                let prepared = prepare(&mut store, &self.request, &self.change).map_err(CronCommitError::Refused)?;
                if prepared.changes.is_empty() {
                    return Ok(prepared.value);
                }
                let Plan { writes, bases } = store.plan(&prepared.changes).map_err(|error| CronCommitError::NotApplied(error.to_string()))?;
                let storage = store.storage().ok_or_else(|| CronCommitError::NotApplied(STAGED_ONLY.into()))?;
                (prepared, bases, storage.repo.apply(writes), storage.probe.clone())
            };
            probe.hit(CronStage::Submitted);
            match completion.await {
                Ok(committed) => {
                    self.confirmed = Some(prepared.value);
                    probe.hit(CronStage::Committed);
                    let consistent = cron.lock().publish(&prepared.changes, &bases, &committed.value, committed.receipt.revision);
                    if !consistent {
                        reread_settled(cron).await?;
                    }
                    self.confirmed = None;
                    return Ok(prepared.value);
                }
                Err(StorageError::StaleRevision { .. }) => reread_settled(cron).await?,
                Err(StorageError::Unconfirmed { .. }) => {
                    fence(cron, |fence| fence.reread = true);
                    self.uncertain = Some(prepared);
                    return self.reconcile(cron).await;
                }
                Err(error) => return Err(CronCommitError::NotApplied(error.to_string())),
            }
        }
        Err(CronCommitError::NotApplied("cron terminal identity kept changing".into()))
    }

    async fn reconcile(&mut self, cron: &CronState) -> Result<RunFinished, CronCommitError> {
        reread_settled(cron).await?;
        let prepared = self.uncertain.as_ref().ok_or_else(|| CronCommitError::Unconfirmed("terminal outcome is absent".into()))?;
        let store = cron.lock();
        let applied = prepared.changes.iter().all(|change| match change {
            CronChange::UpdateRuns { job_id, history, job } => {
                history.iter().all(|run| store.history_entry(job_id, &run.run_id) == Some(run))
                    && job.as_ref().is_none_or(|job| job.last_run.as_ref().is_none_or(|expected| {
                        match store.get(job_id).and_then(|job| job.last_run.as_ref()) {
                            Some(current) if current.run_id == expected.run_id => current == expected,
                            _ => history.iter().any(|run| run.run_id == expected.run_id && store.history_entry(job_id, &run.run_id) == Some(run)),
                        }
                    }))
            }
            CronChange::Remove { id, keep_history: true } => store.get(id).is_none(),
            _ => false,
        });
        if applied {
            if matches!(self.change, TerminalChange::Finish) {
                return Err(CronCommitError::Unconfirmed("matching terminal readback cannot establish the original writer's fresh win; original outcome retained".into()));
            }
            let finished = prepared.value;
            drop(store);
            self.uncertain = None;
            return Ok(finished);
        }
        if matches!(self.change, TerminalChange::Finish) && store.run_receipt(&self.request.job_id, &self.request.run_id).is_none() {
            return Err(CronCommitError::Unconfirmed("terminal receipt is unavailable for exact outcome reconciliation; original finalizer retained".into()));
        }
        let lost = match &self.change {
            TerminalChange::Finish => !store.run_is_active(&self.request.job_id, &self.request.run_id),
            TerminalChange::Report(_) => store.run_copies(&self.request.job_id, &self.request.run_id).into_iter().flatten().any(|run| run.report_delivery.is_some() || run.status == CronRunStatus::Cancelled),
            TerminalChange::Retire => store.get(&self.request.job_id).is_none(),
        };
        if lost {
            drop(store);
            self.uncertain = None;
            return Ok(RunFinished { fresh: false, elapsed_ms: None });
        }
        drop(store);
        Err(CronCommitError::Unconfirmed("terminal transition has no definitive outcome; original request retained without resubmission".into()))
    }
}

fn prepare(store: &mut CronStore, request: &TerminalRun, change: &TerminalChange) -> Result<Prepared<RunFinished>, String> {
    match change {
        TerminalChange::Report(delivery) => {
            let report = store.prepare_set_run_report(&request.job_id, &request.run_id, &request.summary, delivery);
            return Ok(Prepared { value: RunFinished { fresh: !report.changes.is_empty(), elapsed_ms: None }, changes: report.changes });
        }
        TerminalChange::Retire => {
            if store.get(&request.job_id).is_none() { return Ok(Prepared::unchanged(RunFinished { fresh: false, elapsed_ms: None })); }
            if !store.get(&request.job_id).is_some_and(|job| job.oneshot && job.last_run.as_ref().is_some_and(|run| run.run_id == request.run_id && !matches!(run.status, CronRunStatus::Running | CronRunStatus::AwaitingOwnerPrompt))) {
                return Err("Cron oneshot definition or last occurrence changed; retirement remains owned for adjudication".into());
            }
            let retire = store.prepare_delete(&request.job_id, true)?;
            return Ok(Prepared { value: RunFinished { fresh: !retire.changes.is_empty(), elapsed_ms: None }, changes: retire.changes });
        }
        TerminalChange::Finish => {}
    }
    if !matches!(request.status, CronRunStatus::Success | CronRunStatus::Failed) { return Err("Terminal finalization requires success or failure".into()); }
    let session = request.session_id.as_deref().map(|session_id| RunSession { session_id, cwd: request.cwd.as_deref() });
    let mut prepared = store.prepare_finish_run_with_summary(&request.job_id, &request.run_id, request.status.clone(), session, Some(&request.summary), None)?;
    if prepared.value.fresh {
        if let Some(tab) = request.tab_id.as_deref() {
            let bind = |run: &mut CronRunInfo| -> Result<(), String> {
                if run.tab_id.as_deref().is_some_and(|own| own != tab) { return Err("Terminal launch tab differs from the run receipt".into()); }
                run.tab_id = Some(tab.into());
                Ok(())
            };
            for change in &mut prepared.changes {
                if let CronChange::UpdateRuns { history, job, .. } = change {
                    for run in history.iter_mut().filter(|run| run.run_id == request.run_id) { bind(run)?; }
                    if let Some(run) = job.as_mut().and_then(|job| job.last_run.as_mut()).filter(|run| run.run_id == request.run_id) { bind(run)?; }
                }
            }
        }
    }
    Ok(prepared)
}

async fn settle_prior(cron: &CronState) -> Result<(), CronCommitError> {
    let (pending, mut reread) = {
        let mut store = cron.lock();
        let Some(storage) = store.storage_mut() else { return Ok(()) };
        if storage.fence.resolving {
            return Err(CronCommitError::Fenced("another owner is reconciling cron outcomes".into()));
        }
        storage.fence.resolving = true;
        (std::mem::take(&mut storage.fence.in_flight), std::mem::take(&mut storage.fence.reread))
    };
    for InFlight { completion, changes, bases } in pending {
        match completion.await {
            Ok(committed) => reread |= !cron.lock().publish(&changes, &bases, &committed.value, committed.receipt.revision),
            Err(StorageError::Unconfirmed { .. }) => reread = true,
            Err(_) => {}
        }
    }
    let outcome = if reread { reread_settled(cron).await } else { Ok(()) };
    fence(cron, |fence| {
        fence.resolving = false;
        fence.reread |= outcome.is_err();
    });
    notify_fence_settled(cron);
    outcome
}

impl CronStore {
    pub fn terminal_recovery_root(&self) -> PathBuf { self.logs_root.clone() }
}

//! Persistence boundary of the cron cache. The legacy JSON file stays the authority until
//! the all-domain cutover; only `open_storage` builds a Storage-backed store. Record
//! conversions and the legacy decoder live in `codec`, the scheduler-port draft in `port`.
//!
//! Storage changes are prepared under the cron lock, committed with the lock released,
//! and published into the cache only after COMMIT, fenced by each job identity's revision.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{mpsc, Arc};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use super::crud::{log_finished, CronChange, Prepared, RunSession};
use super::{CronFile, CronJob, CronRunInfo, CronRunStatus, CronStore};
use crate::cron::clock::SharedClock;
use crate::cron::scheduler::CronState;
use crate::storage::{
    CommitReceipt, Committed, Completion, CronApplied, CronHistoryDisposition, CronJobId,
    CronJobState, CronRepository, CronWrite, Revision, Storage, StorageError,
};

mod codec;
mod port;

pub use codec::{decode_legacy_cron_file, read_legacy_cron_file, LegacyCronError};
use codec::{definition_of, job_info, job_record, run_info, run_record, stored_scope};
use port::PortDraft;
pub use port::{settle_port, PortSettlement};

const STAGED_ONLY: &str = "cron storage commits only through the staged cron boundary";
const MAX_ATTEMPTS: usize = 4;

/// Why a cron change did not commit as asked. Refusals keep the domain's own message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CronCommitError {
    /// The domain refused the change; nothing was written.
    Refused(String),
    /// Storage refused or failed the commit; neither the database nor the cache changed.
    NotApplied(String),
    /// Never submitted: an earlier outcome is unresolved, so admission stays closed.
    Fenced(String),
    /// Legacy JSON authority: the in-memory change stands but saving the file failed.
    Unsaved(String),
    /// The outcome is unknown; cron changes stay fenced until committed state is reread.
    Unconfirmed(String),
}

impl CronCommitError {
    /// Whether the cache holds the change anyway, as the legacy JSON authority keeps it.
    pub fn applied(&self) -> bool {
        matches!(self, CronCommitError::Unsaved(_))
    }
}

impl fmt::Display for CronCommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CronCommitError::Refused(message) | CronCommitError::Unsaved(message) => {
                f.write_str(message)
            }
            CronCommitError::NotApplied(message) => {
                write!(f, "cron storage did not apply the change: {message}")
            }
            CronCommitError::Fenced(message) => {
                write!(f, "cron storage admission is fenced: {message}")
            }
            CronCommitError::Unconfirmed(message) => {
                write!(f, "cron storage outcome unconfirmed: {message}")
            }
        }
    }
}

impl From<CronCommitError> for String {
    fn from(error: CronCommitError) -> String {
        error.to_string()
    }
}

/// A committed change: the caller's value, every live job it removed and, for Storage,
/// the durability receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronCommitted<T> {
    pub value: T,
    pub removed: Vec<String>,
    pub receipt: Option<CommitReceipt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CronStorageOptions {
    /// Failure deadline of one commit or committed read; expiry is unconfirmed, not failed.
    pub deadline: Duration,
}

impl Default for CronStorageOptions {
    fn default() -> Self {
        CronStorageOptions {
            deadline: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CronStage {
    /// The change is prepared and the cron lock released; nothing is submitted yet.
    Prepared,
    /// The writer admitted or refused the submission; its outcome is still pending.
    Submitted,
    /// The commit settled successfully; the cache has not been published yet.
    Committed,
}

#[cfg(test)]
pub(crate) type StageProbe = Arc<dyn Fn(CronStage) + Send + Sync>;

/// Test barrier around the unlocked commit window; compiled to nothing outside tests.
#[derive(Clone, Default)]
struct Probe(#[cfg(test)] Option<StageProbe>);

impl Probe {
    #[cfg(test)]
    fn hit(&self, stage: CronStage) {
        if let Some(probe) = &self.0 {
            probe(stage);
        }
    }

    #[cfg(not(test))]
    fn hit(&self, _stage: CronStage) {}
}

pub(super) enum Backend {
    #[cfg(test)]
    Legacy(PathBuf),
    Storage(Box<StorageCron>),
    Unavailable(String),
}

impl Backend {
    #[cfg(test)]
    pub(super) fn legacy_path(&self) -> Result<&Path, String> {
        match self {
            Backend::Legacy(path) => Ok(path),
            Backend::Storage(_) => Err(STAGED_ONLY.to_string()),
            Backend::Unavailable(reason) => Err(reason.clone()),
        }
    }

    pub(super) fn require_legacy(&self) -> Result<(), String> {
        match self {
            #[cfg(test)]
            Backend::Legacy(_) => Ok(()),
            Backend::Storage(_) => Err(STAGED_ONLY.to_string()),
            Backend::Unavailable(reason) => Err(reason.clone()),
        }
    }
}

struct InFlight {
    completion: Completion<Committed<CronApplied>>,
    changes: Vec<CronChange>,
    bases: Vec<(String, Option<Revision>)>,
}

/// Unresolved outcomes; while any remains, no new change is prepared.
#[derive(Default)]
struct Fence {
    /// Submissions that outlived their deadline; each completion still reports.
    in_flight: Vec<InFlight>,
    /// Committed state must be reread before the cache is trusted again.
    reread: bool,
    /// One caller is settling the fence with the cron lock released.
    resolving: bool,
}

pub(super) struct StorageCron {
    repo: CronRepository,
    deadline: Duration,
    settle_notify: Option<Arc<dyn Fn() + Send + Sync>>,
    unavailable: Option<String>,
    unavailable_reported: bool,
    /// Last published identity revision per job, kept after removal to fence late publishes.
    revisions: HashMap<String, Revision>,
    fence: Fence,
    draft: Option<PortDraft>,
    probe: Probe,
}

struct Plan {
    writes: Vec<CronWrite>,
    bases: Vec<(String, Option<Revision>)>,
}

impl CronStore {
    pub(crate) fn unavailable(reason: String, logs_root: PathBuf, clock: SharedClock) -> CronStore {
        CronStore {
            file: CronFile::default(),
            backend: Backend::Unavailable(reason),
            logs_root,
            tab_owners: HashMap::new(),
            clock,
        }
    }

    pub(crate) fn unavailable_reason(&self) -> Option<&str> {
        match &self.backend {
            Backend::Unavailable(reason) => Some(reason),
            Backend::Storage(storage) => storage.unavailable.as_deref(),
            #[cfg(test)]
            Backend::Legacy(_) => None,
        }
    }

    /// Blocking: hydrates the cache from committed rows. Call off the async runtime and
    /// outside every lock; an unreadable store is refused, never treated as empty.
    pub fn open_storage(
        storage: &Storage,
        logs_root: PathBuf,
        clock: SharedClock,
        options: CronStorageOptions,
    ) -> Result<CronStore, String> {
        let repo = storage.crons();
        let read = repo
            .snapshot()
            .wait(options.deadline)
            .map_err(|error| format!("cron storage: {error}"))?;
        let mut store = CronStore {
            file: CronFile::default(),
            backend: Backend::Storage(Box::new(StorageCron {
                repo,
                deadline: options.deadline,
                settle_notify: None,
                unavailable: None,
                unavailable_reported: false,
                revisions: HashMap::new(),
                fence: Fence::default(),
                draft: None,
                probe: Probe::default(),
            })),
            logs_root,
            tab_owners: HashMap::new(),
            clock,
        };
        store.merge(read.value.states, read.revision)?;
        Ok(store)
    }

    #[cfg(test)]
    pub(crate) fn set_stage_probe(&mut self, probe: StageProbe) {
        if let Some(storage) = self.storage_mut() {
            storage.probe = Probe(Some(probe));
        }
    }

    #[cfg(test)]
    pub(crate) fn set_storage_deadline(&mut self, deadline: Duration) {
        if let Some(storage) = self.storage_mut() {
            storage.deadline = deadline;
        }
    }

    pub(crate) fn set_fence_notifier(&mut self, notifier: impl Fn() + Send + Sync + 'static) {
        if let Some(storage) = self.storage_mut() {
            storage.settle_notify = Some(Arc::new(notifier));
        }
    }

    pub(crate) fn scheduler_unavailable(&self) -> bool {
        self.unavailable_reason().is_some()
    }

    pub(crate) fn scheduler_settlement_ready(&self) -> bool {
        self.storage().is_some_and(|storage| {
            let fence = &storage.fence;
            (fence.in_flight.is_empty() && !fence.reread && !fence.resolving)
                || fence
                    .in_flight
                    .iter()
                    .any(|in_flight| in_flight.completion.is_settled())
        })
    }

    pub(crate) fn mark_scheduler_unavailable(&mut self, reason: String) {
        match &mut self.backend {
            Backend::Storage(storage) => {
                storage.unavailable.get_or_insert(reason);
                storage.draft = None;
            }
            Backend::Unavailable(unavailable) => *unavailable = reason,
            #[cfg(test)]
            Backend::Legacy(_) => {}
        }
    }

    pub(crate) fn take_scheduler_unavailable_incident(&mut self) -> Option<String> {
        let storage = self.storage_mut()?;
        if storage.unavailable_reported {
            return None;
        }
        let reason = storage.unavailable.clone()?;
        storage.unavailable_reported = true;
        Some(reason)
    }

    fn storage(&self) -> Option<&StorageCron> {
        match &self.backend {
            Backend::Storage(storage) => Some(storage),
            Backend::Unavailable(_) => None,
            #[cfg(test)]
            Backend::Legacy(_) => None,
        }
    }

    fn storage_mut(&mut self) -> Option<&mut StorageCron> {
        match &mut self.backend {
            Backend::Storage(storage) => Some(storage),
            Backend::Unavailable(_) => None,
            #[cfg(test)]
            Backend::Legacy(_) => None,
        }
    }

    /// No earlier outcome is unresolved and no caller is settling one.
    fn fence_clear(&self) -> bool {
        self.storage().is_some_and(|storage| {
            let fence = &storage.fence;
            fence.in_flight.is_empty() && !fence.reread && !fence.resolving
        })
    }

    fn cached_revision(&self, id: &str) -> Option<Revision> {
        self.storage()
            .and_then(|storage| storage.revisions.get(id).copied())
    }

    pub(crate) fn team_teardown_revisions(
        &self,
        team_id: &str,
    ) -> Result<Vec<(String, Revision)>, String> {
        if self.storage().is_none() {
            return Err(STAGED_ONLY.to_string());
        }
        if !self.fence_clear() {
            return Err("cron storage has an unresolved commit fence".into());
        }
        self.list_for_team(team_id)
            .into_iter()
            .map(|job| {
                let revision = self
                    .cached_revision(&job.id)
                    .ok_or_else(|| format!("cron job {} has no published revision", job.id))?;
                Ok((job.id.clone(), revision))
            })
            .collect()
    }

    fn set_revision(&mut self, id: &str, revision: Revision) {
        if let Some(storage) = self.storage_mut() {
            storage.revisions.insert(id.to_string(), revision);
        }
    }

    /// Publishes committed rows: an identity is replaced only by a newer revision, and a
    /// cached job missing from the read is dropped only when it is older than the read.
    fn merge(&mut self, states: Vec<CronJobState>, read: Revision) -> Result<(), String> {
        let Backend::Storage(storage) = &mut self.backend else {
            return Err(STAGED_ONLY.to_string());
        };
        let mut seen = HashSet::new();
        for state in states {
            let id = state.job_id.as_str().to_string();
            seen.insert(id.clone());
            let newer = storage
                .revisions
                .get(&id)
                .map_or(true, |cached| state.identity.revision > *cached);
            if !newer {
                continue;
            }
            let job = state.job.map(job_info).transpose()?;
            let history: Vec<CronRunInfo> = state.history.into_iter().map(run_info).collect();
            match job {
                Some(job) => match self.file.jobs.iter_mut().find(|slot| slot.id == id) {
                    Some(slot) => *slot = job,
                    None => self.file.jobs.push(job),
                },
                None => self.file.jobs.retain(|slot| slot.id != id),
            }
            if history.is_empty() {
                self.file.history.remove(&id);
            } else {
                self.file.history.insert(id.clone(), history);
            }
            storage.revisions.insert(id, state.identity.revision);
        }
        let gone: Vec<String> = self
            .file
            .jobs
            .iter()
            .map(|job| job.id.clone())
            .chain(self.file.history.keys().cloned())
            .filter(|id| !seen.contains(id))
            .filter(|id| {
                storage
                    .revisions
                    .get(id)
                    .map_or(true, |cached| *cached < read)
            })
            .collect();
        for id in gone {
            self.file.jobs.retain(|job| job.id != id);
            self.file.history.remove(&id);
            storage.revisions.insert(id, read);
        }
        Ok(())
    }

    fn plan(&self, changes: &[CronChange]) -> Result<Plan, StorageError> {
        let base = |id: &str| {
            self.cached_revision(id)
                .ok_or_else(|| StorageError::NotFound {
                    entity: format!("cron job {id}"),
                })
        };
        let mut plan = Plan {
            writes: Vec::with_capacity(changes.len()),
            bases: Vec::with_capacity(changes.len()),
        };
        for change in changes {
            let (write, fenced) = match change {
                CronChange::Create(job) => (CronWrite::Create(job_record(job)?), None),
                CronChange::Patch(job) => {
                    let expect = base(&job.id)?;
                    let write = CronWrite::Patch {
                        job: job_record(job)?,
                        expect,
                    };
                    (write, Some(expect))
                }
                CronChange::StartRun { job, run } => {
                    let expect = base(&job.id)?;
                    let write = CronWrite::StartRun {
                        job: job_record(job)?,
                        run: run_record(run)?,
                        expect,
                    };
                    (write, Some(expect))
                }
                CronChange::UpdateRuns {
                    job_id,
                    history,
                    job,
                } => {
                    let expect = base(job_id)?;
                    (
                        self.plan_runs(job_id, history, job.as_ref(), expect)?,
                        Some(expect),
                    )
                }
                CronChange::Remove { id, keep_history } => {
                    let expect = base(id)?;
                    let history = if *keep_history {
                        CronHistoryDisposition::Keep
                    } else {
                        CronHistoryDisposition::Drop
                    };
                    let write = CronWrite::Remove {
                        id: CronJobId::new(id.clone())?,
                        expect,
                        history,
                    };
                    (write, Some(expect))
                }
                CronChange::RemoveScope(scope) => {
                    plan.writes
                        .push(CronWrite::RemoveScope(stored_scope(scope)));
                    continue;
                }
            };
            if let Some(id) = change.job_id() {
                plan.bases.push((id.to_string(), fenced));
            }
            plan.writes.push(write);
        }
        Ok(plan)
    }

    /// Run rows that changed. A last run equal to its history row is written through that
    /// row; an imported divergent copy evolves apart from it and is never created here.
    fn plan_runs(
        &self,
        job_id: &str,
        history: &[CronRunInfo],
        job: Option<&CronJob>,
        expect: Revision,
    ) -> Result<CronWrite, StorageError> {
        let mut runs = history
            .iter()
            .map(run_record)
            .collect::<Result<Vec<_>, _>>()?;
        let mut definition = None;
        let mut divergent = None;
        if let Some(job) = job {
            let original = self.get(job_id);
            let previous_last = original.and_then(|original| original.last_run.as_ref());
            if let Some(last) = job
                .last_run
                .as_ref()
                .filter(|last| previous_last != Some(*last))
            {
                let listed = history
                    .iter()
                    .find(|run| run.run_id == last.run_id)
                    .or_else(|| self.history_entry(job_id, &last.run_id));
                let diverged = previous_last.is_some_and(|previous| {
                    self.history_entry(job_id, &previous.run_id)
                        .is_some_and(|entry| entry != previous)
                });
                match listed {
                    Some(listed) if diverged || listed != last => {
                        divergent = Some(run_record(last)?)
                    }
                    Some(_) => {}
                    None => runs.push(run_record(last)?),
                }
            }
            if original.map_or(true, |original| {
                definition_of(original) != definition_of(job)
            }) {
                definition = Some(job_record(job)?);
            }
        }
        Ok(CronWrite::UpdateRuns {
            job_id: CronJobId::new(job_id.to_string())?,
            expect,
            runs,
            job: definition,
            last_run: divergent,
        })
    }

    /// Applies a committed change; false when the cache was neither at the change's base
    /// nor already past it, so committed state must be reread.
    fn publish(
        &mut self,
        changes: &[CronChange],
        bases: &[(String, Option<Revision>)],
        applied: &CronApplied,
        revision: Revision,
    ) -> bool {
        let mut consistent = true;
        for change in changes {
            if let CronChange::RemoveScope(_) = change {
                for id in &applied.removed {
                    let id = id.as_str();
                    if self
                        .cached_revision(id)
                        .map_or(true, |cached| cached < revision)
                    {
                        self.remove_scoped(id);
                        self.set_revision(id, revision);
                    }
                }
                continue;
            }
            let Some(id) = change.job_id() else { continue };
            let base = bases
                .iter()
                .find(|(job, _)| job == id)
                .and_then(|(_, base)| *base);
            let cached = self.cached_revision(id);
            if cached == base {
                self.apply_change(change);
                self.set_revision(id, revision);
            } else if cached.map_or(true, |cached| cached < revision) {
                consistent = false;
            }
        }
        consistent
    }

    pub(crate) fn publish_external(
        &mut self,
        changes: &[CronChange],
        applied: &CronApplied,
        revision: Revision,
    ) -> bool {
        self.publish(changes, &[], applied, revision)
    }
}

/// Legacy JSON applies and saves under the cron lock as before. Storage blocks off-lock
/// for COMMIT and publishes only what committed: call it off the async runtime.
pub(crate) fn commit<T>(
    cron: &CronState,
    mut prepare: impl FnMut(&mut CronStore) -> Result<Prepared<T>, String>,
) -> Result<CronCommitted<T>, CronCommitError> {
    {
        #[cfg(test)]
        let mut store = cron.lock();
        #[cfg(not(test))]
        let store = cron.lock();
        if store.backend.require_legacy().is_ok() {
            #[cfg(test)]
            {
                let prepared = prepare(&mut store).map_err(CronCommitError::Refused)?;
                let removed = legacy_removed(&store, &prepared.changes);
                for change in &prepared.changes {
                    store.apply_change(change);
                }
                if !prepared.changes.is_empty() {
                    store.save().map_err(CronCommitError::Unsaved)?;
                }
                return Ok(CronCommitted {
                    value: prepared.value,
                    removed,
                    receipt: None,
                });
            }
            #[cfg(not(test))]
            {
                return Err(CronCommitError::NotApplied(
                    "legacy cron backend is test-only".to_string(),
                ));
            }
        }
    }
    settle_fence(cron)?;
    for _ in 0..MAX_ATTEMPTS {
        let (prepared, plan, repo, deadline, probe, settle_notify) = {
            let mut store = cron.lock();
            let prepared = prepare(&mut store).map_err(CronCommitError::Refused)?;
            if prepared.changes.is_empty() {
                return Ok(CronCommitted {
                    value: prepared.value,
                    removed: Vec::new(),
                    receipt: None,
                });
            }
            let plan = store
                .plan(&prepared.changes)
                .map_err(|error| CronCommitError::NotApplied(error.to_string()))?;
            let storage = store
                .storage()
                .ok_or_else(|| CronCommitError::NotApplied(STAGED_ONLY.to_string()))?;
            let repo = storage.repo.clone();
            (
                prepared,
                plan,
                repo,
                storage.deadline,
                storage.probe.clone(),
                storage.settle_notify.clone(),
            )
        };
        probe.hit(CronStage::Prepared);
        let Plan { writes, bases } = plan;
        let completion = {
            let store = cron.lock();
            if !store.fence_clear() {
                return Err(CronCommitError::Fenced(
                    "an earlier cron outcome became unresolved".to_string(),
                ));
            }
            if bases
                .iter()
                .any(|(id, base)| store.cached_revision(id) != *base)
            {
                continue;
            }
            // Admission is a bounded non-blocking enqueue; under the cron lock it is atomic
            // with every fence installation, which also takes this lock.
            repo.apply(writes)
        };
        probe.hit(CronStage::Submitted);
        let waited = wait_retaining(completion, deadline);
        let waited = match waited {
            Waited::Expired(completion) => register_settlement_wake(completion, settle_notify),
            settled => settled,
        };
        match waited {
            Waited::Settled(Ok(committed)) => {
                probe.hit(CronStage::Committed);
                let consistent = cron.lock().publish(
                    &prepared.changes,
                    &bases,
                    &committed.value,
                    committed.receipt.revision,
                );
                if !consistent && reread(cron).is_err() {
                    fence(cron, |fence| fence.reread = true);
                }
                return Ok(CronCommitted {
                    value: prepared.value,
                    removed: removed_ids(&committed.value),
                    receipt: Some(committed.receipt),
                });
            }
            Waited::Settled(Err(StorageError::StaleRevision { .. })) => {
                if let Err(error) = reread(cron) {
                    fence(cron, |fence| fence.reread = true);
                    return Err(error);
                }
            }
            Waited::Settled(Err(error @ StorageError::Unconfirmed { .. })) => {
                fence(cron, |fence| fence.reread = true);
                return Err(CronCommitError::Unconfirmed(error.to_string()));
            }
            Waited::Settled(Err(error)) => {
                if matches!(&error, StorageError::Closed) {
                    mark_unavailable(cron, error.to_string());
                }
                return Err(CronCommitError::NotApplied(error.to_string()));
            }
            Waited::Expired(completion) => {
                fence(cron, |fence| {
                    fence.in_flight.push(InFlight {
                        completion,
                        changes: prepared.changes,
                        bases,
                    })
                });
                return Err(CronCommitError::Unconfirmed(
                    "the cron commit outlived its deadline".to_string(),
                ));
            }
        }
    }
    Err(CronCommitError::NotApplied(
        "cron jobs kept changing concurrently".to_string(),
    ))
}

#[cfg(test)]
fn legacy_removed(store: &CronStore, changes: &[CronChange]) -> Vec<String> {
    let mut removed = Vec::new();
    for change in changes {
        match change {
            CronChange::Remove { id, .. } => removed.push(id.clone()),
            CronChange::RemoveScope(scope) => removed.extend(
                store
                    .list()
                    .iter()
                    .filter(|job| scope.contains(job))
                    .map(|job| job.id.clone()),
            ),
            _ => {}
        }
    }
    removed
}

fn removed_ids(applied: &CronApplied) -> Vec<String> {
    applied
        .removed
        .iter()
        .map(|id| id.as_str().to_string())
        .collect()
}

fn fence(cron: &CronState, update: impl FnOnce(&mut Fence)) {
    if let Some(storage) = cron.lock().storage_mut() {
        update(&mut storage.fence);
    }
}

/// Settles earlier unconfirmed outcomes before any new change is prepared.
fn settle_fence(cron: &CronState) -> Result<(), CronCommitError> {
    let (in_flight, mut needs_reread) = {
        let mut store = cron.lock();
        let Some(storage) = store.storage_mut() else {
            return Ok(());
        };
        let fence = &mut storage.fence;
        if fence.resolving {
            return Err(CronCommitError::Fenced(
                "another caller is reconciling cron storage".to_string(),
            ));
        }
        if fence.in_flight.is_empty() && !fence.reread {
            return Ok(());
        }
        fence.resolving = true;
        let reread = std::mem::take(&mut fence.reread);
        (std::mem::take(&mut fence.in_flight), reread)
    };
    let mut unresolved = Vec::new();
    let mut terminal_error = None;
    for InFlight {
        completion,
        changes,
        bases,
    } in in_flight
    {
        match completion.try_take() {
            Err(completion) => unresolved.push(InFlight {
                completion,
                changes,
                bases,
            }),
            Ok(Ok(committed)) => {
                let consistent = cron.lock().publish(
                    &changes,
                    &bases,
                    &committed.value,
                    committed.receipt.revision,
                );
                needs_reread |= !consistent;
            }
            Ok(Err(StorageError::Unconfirmed { .. })) => needs_reread = true,
            Ok(Err(error @ StorageError::Closed)) => {
                let reason = error.to_string();
                mark_unavailable(cron, reason.clone());
                terminal_error = Some(reason);
            }
            Ok(Err(_)) => {}
        }
    }
    let reread_error = if needs_reread {
        reread(cron).err()
    } else {
        None
    };
    let still_unresolved = !unresolved.is_empty();
    let terminal_reread = reread_error.is_some() && cron.lock().scheduler_unavailable();
    fence(cron, |fence| {
        fence.resolving = false;
        fence.in_flight.extend(unresolved);
        fence.reread |= reread_error.is_some();
    });
    if !still_unresolved && terminal_reread {
        notify_fence_settled(cron);
    }
    if still_unresolved {
        return Err(CronCommitError::Fenced(
            "an earlier cron commit is still unresolved".to_string(),
        ));
    }
    if let Some(error) = terminal_error {
        return Err(CronCommitError::NotApplied(error));
    }
    reread_error.map_or(Ok(()), |error| {
        Err(CronCommitError::Fenced(format!(
            "committed state was not reread: {error}"
        )))
    })
}

/// Rereads every committed identity and merges it under the revision fence.
fn reread(cron: &CronState) -> Result<(), CronCommitError> {
    let (repo, deadline) = {
        let store = cron.lock();
        let storage = store
            .storage()
            .ok_or_else(|| CronCommitError::NotApplied(STAGED_ONLY.to_string()))?;
        (storage.repo.clone(), storage.deadline)
    };
    let read = match repo.snapshot().wait(deadline) {
        Ok(read) => read,
        Err(error @ StorageError::Unconfirmed { .. }) => {
            return Err(CronCommitError::Unconfirmed(format!(
                "committed cron state was not reread: {error}"
            )))
        }
        Err(error) => {
            let reason = format!("committed cron state could not be reread: {error}");
            mark_unavailable(cron, reason.clone());
            return Err(CronCommitError::NotApplied(reason));
        }
    };
    let merged = { cron.lock().merge(read.value.states, read.revision) };
    if let Err(error) = merged {
        mark_unavailable(cron, error.clone());
        return Err(CronCommitError::NotApplied(error));
    }
    notify_fence_settled(cron);
    Ok(())
}

/// A Storage change admitted without waiting for COMMIT.
struct Admitted<T> {
    completion: Completion<Committed<CronApplied>>,
    prepared: Prepared<T>,
    bases: Vec<(String, Option<Revision>)>,
}

enum Admission<T> {
    Unchanged(T),
    Admitted(Admitted<T>),
}

impl CronStore {
    /// Whether changes commit to Storage off the cron lock rather than to the legacy file.
    pub(crate) fn commits_to_storage(&self) -> bool {
        self.storage().is_some()
    }

    /// Admits a Storage change without waiting for COMMIT; `None` when nothing is written.
    /// The future is driven by the completion's own wake and publishes only what committed.
    pub(crate) fn stage<T, P>(
        cron: &CronState,
        mut prepare: P,
    ) -> Result<
        Option<impl Future<Output = Result<CronCommitted<T>, CronCommitError>> + Send + 'static>,
        CronCommitError,
    >
    where
        T: Send + 'static,
        P: FnMut(&mut CronStore) -> Result<Prepared<T>, String> + Send + 'static,
    {
        let admitted = match admit(cron, &mut prepare)? {
            Admission::Unchanged(_) => return Ok(None),
            Admission::Admitted(admitted) => admitted,
        };
        let cron = cron.clone();
        Ok(Some(async move {
            settle_admitted(&cron, admitted, prepare).await
        }))
    }
}

/// Prepares and admits in one cron lock hold. A fenced store refuses, since settling its
/// fence waits on committed reads.
fn admit<T>(
    cron: &CronState,
    prepare: impl FnOnce(&mut CronStore) -> Result<Prepared<T>, String>,
) -> Result<Admission<T>, CronCommitError> {
    let mut store = cron.lock();
    let repo = store
        .storage()
        .map(|storage| storage.repo.clone())
        .ok_or_else(|| CronCommitError::NotApplied(STAGED_ONLY.to_string()))?;
    if !store.fence_clear() {
        return Err(CronCommitError::Fenced(
            "an earlier cron outcome is unresolved".to_string(),
        ));
    }
    let prepared = prepare(&mut store).map_err(CronCommitError::Refused)?;
    if prepared.changes.is_empty() {
        return Ok(Admission::Unchanged(prepared.value));
    }
    let Plan { writes, bases } = store
        .plan(&prepared.changes)
        .map_err(|error| CronCommitError::NotApplied(error.to_string()))?;
    let completion = repo.apply(writes);
    Ok(Admission::Admitted(Admitted {
        completion,
        prepared,
        bases,
    }))
}

/// Publishes the outcome once the completion wakes; a stale base rereads committed state
/// and admits the same request again, as `commit` does.
async fn settle_admitted<T>(
    cron: &CronState,
    mut admitted: Admitted<T>,
    mut prepare: impl FnMut(&mut CronStore) -> Result<Prepared<T>, String>,
) -> Result<CronCommitted<T>, CronCommitError> {
    let mut attempts = 1;
    loop {
        let Admitted {
            completion,
            prepared,
            bases,
        } = admitted;
        let outcome = completion.await;
        match outcome {
            Ok(committed) => {
                let consistent = cron.lock().publish(
                    &prepared.changes,
                    &bases,
                    &committed.value,
                    committed.receipt.revision,
                );
                if !consistent && reread_settled(cron).await.is_err() {
                    fence(cron, |fence| fence.reread = true);
                }
                return Ok(CronCommitted {
                    value: prepared.value,
                    removed: removed_ids(&committed.value),
                    receipt: Some(committed.receipt),
                });
            }
            Err(StorageError::StaleRevision { .. }) => {
                if let Err(error) = reread_settled(cron).await {
                    fence(cron, |fence| fence.reread = true);
                    return Err(error);
                }
                if attempts == MAX_ATTEMPTS {
                    return Err(CronCommitError::NotApplied(
                        "cron jobs kept changing concurrently".to_string(),
                    ));
                }
                attempts += 1;
                admitted = match admit(cron, &mut prepare)? {
                    Admission::Admitted(next) => next,
                    Admission::Unchanged(value) => {
                        return Ok(CronCommitted {
                            value,
                            removed: Vec::new(),
                            receipt: None,
                        })
                    }
                };
            }
            Err(error @ StorageError::Unconfirmed { .. }) => {
                fence(cron, |fence| fence.reread = true);
                return Err(CronCommitError::Unconfirmed(error.to_string()));
            }
            Err(error @ StorageError::Closed) => {
                mark_unavailable(cron, error.to_string());
                return Err(CronCommitError::NotApplied(error.to_string()));
            }
            Err(error) => return Err(CronCommitError::NotApplied(error.to_string())),
        }
    }
}

/// `reread` driven by the read completion's wake instead of a blocking wait.
async fn reread_settled(cron: &CronState) -> Result<(), CronCommitError> {
    let repo = cron
        .lock()
        .storage()
        .map(|storage| storage.repo.clone())
        .ok_or_else(|| CronCommitError::NotApplied(STAGED_ONLY.to_string()))?;
    let read = match repo.snapshot().await {
        Ok(read) => read,
        Err(error @ StorageError::Unconfirmed { .. }) => {
            return Err(CronCommitError::Unconfirmed(format!(
                "committed cron state was not reread: {error}"
            )))
        }
        Err(error) => {
            let reason = format!("committed cron state could not be reread: {error}");
            mark_unavailable(cron, reason.clone());
            return Err(CronCommitError::NotApplied(reason));
        }
    };
    let merged = { cron.lock().merge(read.value.states, read.revision) };
    if let Err(error) = merged {
        mark_unavailable(cron, error.clone());
        return Err(CronCommitError::NotApplied(error));
    }
    notify_fence_settled(cron);
    Ok(())
}

/// Records a run completion; `Ok(true)` only for the first terminal transition, which
/// alone may notify.
pub(crate) fn finish_run(
    cron: &CronState,
    job_id: &str,
    run_id: &str,
    status: CronRunStatus,
    session: Option<RunSession<'_>>,
) -> Result<bool, CronCommitError> {
    let finished = commit(cron, |store| {
        store.prepare_finish_run(job_id, run_id, status.clone(), session)
    })?
    .value;
    log_finished(job_id, run_id, &status, finished);
    Ok(finished.fresh)
}

pub(crate) fn finish_run_with_summary(
    cron: &CronState,
    job_id: &str,
    run_id: &str,
    status: CronRunStatus,
    summary: &str,
) -> Result<bool, CronCommitError> {
    let finished = commit(cron, |store| {
        store.prepare_finish_run_with_summary(job_id, run_id, status.clone(), None, Some(summary))
    })?
    .value;
    log_finished(job_id, run_id, &status, finished);
    Ok(finished.fresh)
}

enum Waited<T> {
    Settled(Result<T, StorageError>),
    Expired(Completion<T>),
}

/// Wakes the committing thread when the completion settles.
struct Signal(mpsc::Sender<()>);

impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        let _ = self.0.send(());
    }
}

/// Waits for the completion's own wake-up; at the deadline the still-pending completion
/// is handed back, because the operation may yet commit.
fn wait_retaining<T>(mut completion: Completion<T>, deadline: Duration) -> Waited<T> {
    let (sender, woken) = mpsc::channel();
    let waker = Waker::from(Arc::new(Signal(sender)));
    let mut context = Context::from_waker(&waker);
    let until = Instant::now().checked_add(deadline);
    loop {
        if let Poll::Ready(result) = Pin::new(&mut completion).poll(&mut context) {
            return Waited::Settled(result);
        }
        let woke = match until {
            Some(until) => woken
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .is_ok(),
            None => woken.recv().is_ok(),
        };
        if !woke {
            return Waited::Expired(completion);
        }
    }
}

struct SettlementNotifier(Option<Arc<dyn Fn() + Send + Sync>>);

impl Wake for SettlementNotifier {
    fn wake(self: Arc<Self>) {
        if let Some(notify) = &self.0 {
            notify();
        }
    }
}

fn register_settlement_wake<T>(
    mut completion: Completion<T>,
    notifier: Option<Arc<dyn Fn() + Send + Sync>>,
) -> Waited<T> {
    let waker = Waker::from(Arc::new(SettlementNotifier(notifier)));
    let mut context = Context::from_waker(&waker);
    match Pin::new(&mut completion).poll(&mut context) {
        Poll::Ready(result) => Waited::Settled(result),
        Poll::Pending => Waited::Expired(completion),
    }
}

fn notify_fence_settled(cron: &CronState) {
    let notifier = cron
        .lock()
        .storage()
        .and_then(|storage| storage.settle_notify.clone());
    if let Some(notify) = notifier {
        notify();
    }
}

fn mark_unavailable(cron: &CronState, reason: String) {
    cron.lock().mark_scheduler_unavailable(reason);
}

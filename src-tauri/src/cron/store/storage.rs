//! Persistence boundary of the cron cache. The legacy JSON file stays the authority until
//! the all-domain cutover; only `open_storage` builds a Storage-backed store.
//!
//! Storage changes are prepared under the cron lock, committed with the lock released,
//! and published into the cache only after COMMIT, fenced by each job identity's revision.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{mpsc, Arc};
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

use super::crud::{log_finished, CronChange, Prepared, RunSession};
use super::{
    CronCreateParams, CronFile, CronJob, CronRunInfo, CronRunStatus, CronScope, CronStore,
};
use crate::cron::clock::SharedClock;
use crate::cron::scheduler::CronState;
use crate::storage::{
    CommitReceipt, Committed, Completion, CronApplied, CronHistoryDisposition, CronHistoryImport,
    CronImport, CronJobId, CronJobImport, CronJobRecord, CronJobState, CronRepository, CronRunId,
    CronRunRecord, CronRunState, CronWrite, Revision, Storage, StorageError, StoredCronJob,
};

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
    Legacy(PathBuf),
    Storage(Box<StorageCron>),
}

impl Backend {
    pub(super) fn legacy_path(&self) -> Result<&Path, String> {
        match self {
            Backend::Legacy(path) => Ok(path),
            Backend::Storage(_) => Err(STAGED_ONLY.to_string()),
        }
    }

    pub(super) fn require_legacy(&self) -> Result<(), String> {
        self.legacy_path().map(|_| ())
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
    /// Last published identity revision per job, kept after removal to fence late publishes.
    revisions: HashMap<String, Revision>,
    fence: Fence,
    draft: Option<PortDraft>,
    probe: Probe,
}

/// Definitions as the mem scheduler port sees them while Storage owns persistence; port
/// edits collect here and commit at `settle_port`, never under the caller's cron lock.
pub(super) struct PortDraft {
    original: Vec<CronJob>,
    jobs: Vec<CronJob>,
    bases: HashMap<String, Revision>,
}

impl PortDraft {
    fn open(jobs: Vec<CronJob>, revisions: &HashMap<String, Revision>) -> Self {
        let bases = jobs
            .iter()
            .filter_map(|job| revisions.get(&job.id).map(|base| (job.id.clone(), *base)))
            .collect();
        PortDraft {
            original: jobs.clone(),
            jobs,
            bases,
        }
    }
}

struct Plan {
    writes: Vec<CronWrite>,
    bases: Vec<(String, Option<Revision>)>,
}

impl CronStore {
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

    fn storage(&self) -> Option<&StorageCron> {
        match &self.backend {
            Backend::Storage(storage) => Some(storage),
            Backend::Legacy(_) => None,
        }
    }

    fn storage_mut(&mut self) -> Option<&mut StorageCron> {
        match &mut self.backend {
            Backend::Storage(storage) => Some(storage),
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

    /// Run rows that changed; the last run is written through its own row and only a
    /// changed definition rewrites the job row.
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
        if let Some(job) = job {
            let original = self.get(job_id);
            let previous_last = original.and_then(|original| original.last_run.as_ref());
            if let Some(last) = job
                .last_run
                .as_ref()
                .filter(|last| previous_last != Some(*last))
            {
                match history.iter().find(|run| run.run_id == last.run_id) {
                    Some(listed) if listed != last => {
                        return Err(StorageError::Invalid {
                            field: "cron last run",
                            reason: format!("run {} diverges from its history row", last.run_id),
                        })
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

    pub(crate) fn port_jobs(&self) -> &[CronJob] {
        match &self.backend {
            Backend::Storage(storage) => storage
                .draft
                .as_ref()
                .map_or(self.file.jobs.as_slice(), |draft| draft.jobs.as_slice()),
            Backend::Legacy(_) => self.file.jobs.as_slice(),
        }
    }

    fn open_draft(&mut self) -> Result<&mut PortDraft, String> {
        let Backend::Storage(storage) = &mut self.backend else {
            return Err(STAGED_ONLY.to_string());
        };
        let StorageCron {
            draft, revisions, ..
        } = &mut **storage;
        let jobs = &self.file.jobs;
        Ok(draft.get_or_insert_with(|| PortDraft::open(jobs.clone(), revisions)))
    }

    /// Legacy: an in-memory edit the port saves later. Storage: an edit of the draft.
    pub(crate) fn port_update(&mut self, job: CronJob) -> Result<(), String> {
        if self.backend.require_legacy().is_ok() {
            self.apply_change(&CronChange::Patch(job));
            return Ok(());
        }
        let draft = self.open_draft()?;
        let slot = draft
            .jobs
            .iter_mut()
            .find(|slot| slot.id == job.id)
            .ok_or_else(|| format!("cron job not found: {}", job.id))?;
        *slot = job;
        Ok(())
    }

    pub(crate) fn port_create(&mut self, params: CronCreateParams) -> Result<(), String> {
        if self.backend.require_legacy().is_ok() {
            return self.create(params).map(|_| ());
        }
        let job = self.build_job(params)?;
        self.open_draft()?.jobs.push(job);
        Ok(())
    }

    pub(crate) fn port_delete(&mut self, id: &str) -> Result<(), String> {
        if self.backend.require_legacy().is_ok() {
            return self.delete(id);
        }
        let draft = self.open_draft()?;
        let job = draft
            .jobs
            .iter()
            .find(|job| job.id == id)
            .ok_or_else(|| format!("Cron job not found: {}", id))?;
        if job.protected {
            return Err(format!(
                "Cron job '{}' is protected and cannot be deleted. Use disable instead.",
                job.name
            ));
        }
        draft.jobs.retain(|job| job.id != id);
        Ok(())
    }

    /// Legacy saves the file; Storage commits the draft only at `settle_port`.
    pub(crate) fn port_save(&mut self) -> Result<(), String> {
        if self.backend.require_legacy().is_ok() {
            return self.save();
        }
        Ok(())
    }

    /// A draft applies while every definition it edits or removes is still at the revision
    /// it read; definitions it left untouched may move.
    fn draft_applies(&self, draft: &PortDraft) -> bool {
        self.storage().is_some_and(|storage| {
            draft
                .original
                .iter()
                .filter(|job| draft.jobs.iter().find(|next| next.id == job.id) != Some(*job))
                .all(|job| storage.revisions.get(&job.id) == draft.bases.get(&job.id))
        })
    }

    fn prepare_draft(&self, draft: &PortDraft) -> Result<Prepared<()>, String> {
        if !self.draft_applies(draft) {
            return Err("cron jobs changed while the scheduler port draft was open".to_string());
        }
        let mut changes = Vec::new();
        for job in &draft.original {
            match draft.jobs.iter().find(|next| next.id == job.id) {
                None => changes.push(CronChange::Remove {
                    id: job.id.clone(),
                    keep_history: false,
                }),
                Some(next) if next != job => changes.push(CronChange::Patch(next.clone())),
                Some(_) => {}
            }
        }
        for job in &draft.jobs {
            if !draft.original.iter().any(|original| original.id == job.id) {
                changes.push(CronChange::Create(job.clone()));
            }
        }
        Ok(Prepared { changes, value: () })
    }
}

/// Legacy JSON applies and saves under the cron lock as before. Storage blocks off-lock
/// for COMMIT and publishes only what committed: call it off the async runtime.
pub(crate) fn commit<T>(
    cron: &CronState,
    mut prepare: impl FnMut(&mut CronStore) -> Result<Prepared<T>, String>,
) -> Result<CronCommitted<T>, CronCommitError> {
    {
        let mut store = cron.lock();
        if store.backend.require_legacy().is_ok() {
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
    }
    settle_fence(cron)?;
    for _ in 0..MAX_ATTEMPTS {
        let (prepared, plan, repo, deadline, probe) = {
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
        match wait_retaining(completion, deadline) {
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
            Ok(Err(_)) => {}
        }
    }
    let reread_error = if needs_reread {
        reread(cron).err()
    } else {
        None
    };
    let still_unresolved = !unresolved.is_empty();
    fence(cron, |fence| {
        fence.resolving = false;
        fence.in_flight.extend(unresolved);
        fence.reread |= reread_error.is_some();
    });
    if still_unresolved {
        return Err(CronCommitError::Fenced(
            "an earlier cron commit is still unresolved".to_string(),
        ));
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
    let read = repo.snapshot().wait(deadline).map_err(|error| {
        CronCommitError::Unconfirmed(format!("committed cron state was not reread: {error}"))
    })?;
    cron.lock()
        .merge(read.value.states, read.revision)
        .map_err(CronCommitError::NotApplied)
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

/// What became of the scheduler-port draft at `settle_port`; only `Retained` keeps it.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortSettlement {
    /// No draft was open.
    Empty,
    /// Committed in one transaction and published.
    Committed,
    /// Nothing was written; the draft is kept unchanged for an explicit later settle.
    Retained(CronCommitError),
    /// Nothing was written and the draft no longer applies to committed state.
    Discarded(CronCommitError),
    /// Admitted with an unknown outcome: never replayed, the fence publishes what committed.
    Unconfirmed(CronCommitError),
}

/// Commits the mem scheduler port's draft in one transaction. Call after the lock that
/// held the port was released.
pub fn settle_port(cron: &CronState) -> PortSettlement {
    let draft = cron
        .lock()
        .storage_mut()
        .and_then(|storage| storage.draft.take());
    let Some(draft) = draft else {
        return PortSettlement::Empty;
    };
    let error = match commit(cron, |store| store.prepare_draft(&draft)) {
        Ok(_) => return PortSettlement::Committed,
        Err(error @ CronCommitError::Unconfirmed(_)) => return PortSettlement::Unconfirmed(error),
        Err(error) => error,
    };
    let mut store = cron.lock();
    if store.draft_applies(&draft) {
        if let Some(storage) = store
            .storage_mut()
            .filter(|storage| storage.draft.is_none())
        {
            storage.draft = Some(draft);
            return PortSettlement::Retained(error);
        }
    }
    PortSettlement::Discarded(error)
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

fn stored_scope(scope: &CronScope) -> crate::storage::CronScope {
    match scope {
        CronScope::Workspace(id) => crate::storage::CronScope::Workspace(id.clone()),
        CronScope::Team(id) => crate::storage::CronScope::Team(id.clone()),
    }
}

/// Definition values without the last-run mirror, which lives in its own row.
fn definition_of(job: &CronJob) -> CronJob {
    CronJob {
        last_run: None,
        ..job.clone()
    }
}

fn state_of(status: &CronRunStatus) -> CronRunState {
    match status {
        CronRunStatus::Running => CronRunState::Running,
        CronRunStatus::AwaitingOwnerPrompt => CronRunState::AwaitingOwnerPrompt,
        CronRunStatus::Success => CronRunState::Success,
        CronRunStatus::Failed => CronRunState::Failed,
        CronRunStatus::Cancelled => CronRunState::Cancelled,
    }
}

fn status_of(state: CronRunState) -> CronRunStatus {
    match state {
        CronRunState::Running => CronRunStatus::Running,
        CronRunState::AwaitingOwnerPrompt => CronRunStatus::AwaitingOwnerPrompt,
        CronRunState::Success => CronRunStatus::Success,
        CronRunState::Failed => CronRunStatus::Failed,
        CronRunState::Cancelled => CronRunStatus::Cancelled,
    }
}

fn run_record(run: &CronRunInfo) -> Result<CronRunRecord, StorageError> {
    Ok(CronRunRecord {
        run_id: CronRunId::new(run.run_id.clone())?,
        started_at: run.started_at,
        finished_at: run.finished_at,
        status: state_of(&run.status),
        tab_id: run.tab_id.clone(),
        session_id: run.session_id.clone(),
        cwd: run.cwd.clone(),
        summary: run.summary.clone(),
        report_delivery: run.report_delivery.clone(),
    })
}

fn run_info(run: CronRunRecord) -> CronRunInfo {
    CronRunInfo {
        run_id: run.run_id.as_str().to_string(),
        started_at: run.started_at,
        finished_at: run.finished_at,
        status: status_of(run.status),
        tab_id: run.tab_id,
        session_id: run.session_id,
        cwd: run.cwd,
        summary: run.summary,
        report_delivery: run.report_delivery,
    }
}

fn job_record(job: &CronJob) -> Result<CronJobRecord, StorageError> {
    Ok(CronJobRecord {
        id: CronJobId::new(job.id.clone())?,
        name: job.name.clone(),
        description: job.description.clone(),
        workspace_id: job.workspace_id.clone(),
        team_id: job.team_id.clone(),
        schedule: job.schedule.clone(),
        enabled: job.enabled,
        agent: job.agent.clone(),
        model: job.model.clone(),
        reasoning_effort: job.reasoning_effort.clone(),
        task: job.task.clone(),
        cwd: job.cwd.clone(),
        role: job.role.clone(),
        skill: job.skill.clone(),
        creator_tab_id: job.creator_tab_id.clone(),
        report_target_tab_id: job.report_target_tab_id.clone(),
        oneshot: job.oneshot,
        max_history: u64::try_from(job.max_history).map_err(|_| StorageError::Invalid {
            field: "cron max history",
            reason: "exceeds the 64-bit range".to_string(),
        })?,
        created_at: job.created_at,
        last_run_id: job
            .last_run
            .as_ref()
            .map(|run| CronRunId::new(run.run_id.clone()))
            .transpose()?,
        next_run: job.next_run,
        jitter_max_ms: job.jitter_max_ms,
        tz: job.tz.clone(),
        timeout_seconds: job.timeout_seconds,
        consecutive_failures: job.consecutive_failures,
        protected: job.protected,
    })
}

fn job_info(stored: StoredCronJob) -> Result<CronJob, String> {
    let StoredCronJob { record, last_run } = stored;
    Ok(CronJob {
        id: record.id.as_str().to_string(),
        name: record.name,
        description: record.description,
        workspace_id: record.workspace_id,
        team_id: record.team_id,
        schedule: record.schedule,
        enabled: record.enabled,
        agent: record.agent,
        model: record.model,
        reasoning_effort: record.reasoning_effort,
        task: record.task,
        cwd: record.cwd,
        role: record.role,
        skill: record.skill,
        creator_tab_id: record.creator_tab_id,
        report_target_tab_id: record.report_target_tab_id,
        oneshot: record.oneshot,
        max_history: usize::try_from(record.max_history)
            .map_err(|_| "cron max history exceeds this platform".to_string())?,
        created_at: record.created_at,
        last_run: last_run.map(run_info),
        next_run: record.next_run,
        jitter_max_ms: record.jitter_max_ms,
        tz: record.tz,
        timeout_seconds: record.timeout_seconds,
        consecutive_failures: record.consecutive_failures,
        protected: record.protected,
    })
}

/// Why an original crons.json cannot be imported; a genuinely absent file is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyCronError {
    Unreadable(String),
    Corrupt(String),
    Unsupported(String),
    /// The original holds facts that cannot be stored as given.
    Conflict(String),
}

impl fmt::Display for LegacyCronError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LegacyCronError::Unreadable(detail) => write!(f, "crons.json unreadable: {detail}"),
            LegacyCronError::Corrupt(detail) => write!(f, "crons.json corrupt: {detail}"),
            LegacyCronError::Unsupported(detail) => write!(f, "crons.json unsupported: {detail}"),
            LegacyCronError::Conflict(detail) => write!(f, "crons.json conflict: {detail}"),
        }
    }
}

/// Reads the original without recovery, quarantine or rewrite; `Ok(None)` only when the
/// file is genuinely absent.
pub fn read_legacy_cron_file(path: &Path) -> Result<Option<CronImport>, LegacyCronError> {
    match std::fs::read(path) {
        Ok(bytes) => decode_legacy_cron_file(&bytes).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(LegacyCronError::Unreadable(error.to_string())),
    }
}

/// Decodes original bytes into a conserved import: definition order, each history list
/// in order, nulls and the last-run mirror as stored. Nothing is rescheduled.
pub fn decode_legacy_cron_file(bytes: &[u8]) -> Result<CronImport, LegacyCronError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| LegacyCronError::Corrupt(error.to_string()))?;
    match value.get("version").and_then(serde_json::Value::as_str) {
        Some("1") => {}
        Some(other) => return Err(LegacyCronError::Unsupported(format!("version {other}"))),
        None => return Err(LegacyCronError::Corrupt("missing version".to_string())),
    }
    let file: CronFile = serde_json::from_value(value)
        .map_err(|error| LegacyCronError::Corrupt(error.to_string()))?;
    let conflict = |error: StorageError| LegacyCronError::Conflict(error.to_string());
    let mut jobs = Vec::with_capacity(file.jobs.len());
    for job in &file.jobs {
        jobs.push(CronJobImport {
            record: job_record(job).map_err(conflict)?,
            last_run: job
                .last_run
                .as_ref()
                .map(run_record)
                .transpose()
                .map_err(conflict)?,
        });
    }
    let mut histories = Vec::with_capacity(file.history.len());
    for (job_id, runs) in &file.history {
        histories.push(CronHistoryImport {
            job_id: CronJobId::new(job_id.clone()).map_err(conflict)?,
            runs: runs
                .iter()
                .map(run_record)
                .collect::<Result<_, _>>()
                .map_err(conflict)?,
        });
    }
    // History is a map: sorting by identity makes the import deterministic.
    histories.sort_by(|a, b| a.job_id.cmp(&b.job_id));
    let import = CronImport { jobs, histories };
    import.validate().map_err(conflict)?;
    Ok(import)
}

//! The mem scheduler port's draft: edits collect off the cache while Storage owns
//! persistence, and commit in one transaction once the caller released the cron lock.

use std::collections::HashMap;

use super::super::crud::{CronChange, Prepared};
use super::super::{CronCreateParams, CronJob, CronStore};
use super::{commit, Backend, CronCommitError, StorageCron, STAGED_ONLY};
use crate::cron::scheduler::CronState;
use crate::storage::Revision;

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

impl CronStore {
    pub(crate) fn port_jobs(&self) -> &[CronJob] {
        match &self.backend {
            Backend::Storage(storage) => storage
                .draft
                .as_ref()
                .map_or(self.file.jobs.as_slice(), |draft| draft.jobs.as_slice()),
            Backend::Unavailable(_) => self.file.jobs.as_slice(),
            #[cfg(test)]
            Backend::Legacy(_) => self.file.jobs.as_slice(),
        }
    }

    fn open_draft(&mut self) -> Result<&mut PortDraft, String> {
        if let Some(reason) = self.unavailable_reason() {
            return Err(format!("cron storage unavailable: {reason}"));
        }
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
        if let Some(reason) = self.unavailable_reason() {
            return Err(format!("cron storage unavailable: {reason}"));
        }
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
        if let Some(reason) = self.unavailable_reason() {
            return Err(format!("cron storage unavailable: {reason}"));
        }
        if self.backend.require_legacy().is_ok() {
            return self.create(params).map(|_| ());
        }
        let job = self.build_job(params)?;
        self.open_draft()?.jobs.push(job);
        Ok(())
    }

    pub(crate) fn port_delete(&mut self, id: &str) -> Result<(), String> {
        if let Some(reason) = self.unavailable_reason() {
            return Err(format!("cron storage unavailable: {reason}"));
        }
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
        if let Some(reason) = self.unavailable_reason() {
            return Err(format!("cron storage unavailable: {reason}"));
        }
        if self.backend.require_legacy().is_ok() {
            #[cfg(test)]
            return self.save();
            #[cfg(not(test))]
            return Err("legacy cron writer is test-only".to_string());
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

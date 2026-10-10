//! Typed cron definitions and ordered run rows. Each identity's revision covers its
//! definition and runs; history outlives the definition as a retired identity, and an
//! imported last run that differs from its history row is kept as its own copy.

mod mutations;
mod queries;
mod records;

pub(crate) use mutations::apply as apply_mutations;

pub(crate) use mutations::write_import;
pub use records::{
    CronApplied, CronDivergenceOrigin, CronHistoryDisposition, CronHistoryImport, CronIdentity,
    CronImport, CronJobId, CronJobImport, CronJobRecord, CronJobState, CronLastRunDivergence,
    CronRunField, CronRunId, CronRunRecord, CronRunState, CronScope, CronSnapshot, CronWrite,
    StoredCronJob, MAX_CRON_BATCH, MAX_CRON_HISTORY, MAX_CRON_JOBS,
};

use crate::owner::Storage;
use crate::repositories::{Committed, Completion, Read, Revision};

impl Storage {
    pub fn crons(&self) -> CronRepository {
        CronRepository {
            storage: self.clone(),
        }
    }

    /// Seeds `root` with a database built by the migrations before the cron schema.
    #[cfg(feature = "test-support")]
    pub fn seed_before_cron_schema(root: &std::path::Path) -> Result<i32, crate::StorageError> {
        crate::schema::cron::seed_before_cron(root)
    }
}

/// Callers submit already-authorized domain writes; each read sees one committed revision.
#[derive(Clone)]
pub struct CronRepository {
    storage: Storage,
}

impl CronRepository {
    pub fn snapshot(&self) -> Completion<Read<CronSnapshot>> {
        self.storage
            .submit_read(|cx| queries::load(cx.conn, None).map(|states| CronSnapshot { states }))
    }

    pub fn job(&self, id: CronJobId) -> Completion<Read<Option<CronJobState>>> {
        self.storage
            .submit_read(move |cx| queries::load(cx.conn, Some(&id)).map(|mut states| states.pop()))
    }

    pub fn create(&self, job: CronJobRecord) -> Completion<Committed<CronApplied>> {
        self.apply(vec![CronWrite::Create(job)])
    }

    pub fn patch(
        &self,
        job: CronJobRecord,
        expect: Revision,
    ) -> Completion<Committed<CronApplied>> {
        self.apply(vec![CronWrite::Patch { job, expect }])
    }

    pub fn start_run(
        &self,
        job: CronJobRecord,
        run: CronRunRecord,
        expect: Revision,
    ) -> Completion<Committed<CronApplied>> {
        self.apply(vec![CronWrite::StartRun { job, run, expect }])
    }

    pub fn update_runs(
        &self,
        job_id: CronJobId,
        expect: Revision,
        runs: Vec<CronRunRecord>,
        job: Option<CronJobRecord>,
    ) -> Completion<Committed<CronApplied>> {
        self.apply(vec![CronWrite::UpdateRuns {
            job_id,
            expect,
            runs,
            job,
            last_run: None,
        }])
    }

    pub fn remove(
        &self,
        id: CronJobId,
        expect: Revision,
        history: CronHistoryDisposition,
    ) -> Completion<Committed<CronApplied>> {
        self.apply(vec![CronWrite::Remove {
            id,
            expect,
            history,
        }])
    }

    pub fn remove_workspace(&self, workspace_id: String) -> Completion<Committed<CronApplied>> {
        self.apply(vec![CronWrite::RemoveScope(CronScope::Workspace(
            workspace_id,
        ))])
    }

    pub fn remove_team(&self, team_id: String) -> Completion<Committed<CronApplied>> {
        self.apply(vec![CronWrite::RemoveScope(CronScope::Team(team_id))])
    }

    /// One transaction for the whole batch; any refused write rejects every write.
    pub fn apply(&self, batch: Vec<CronWrite>) -> Completion<Committed<CronApplied>> {
        let cost = match records::validate_batch(&batch) {
            Ok(cost) => cost,
            Err(error) => return Completion::settled(Err(error)),
        };
        self.storage
            .submit_mutation(cost, move |cx| mutations::apply(cx, batch))
    }

    /// Writes an original domain into an empty cron domain exactly as given.
    pub fn import(&self, import: CronImport) -> Completion<Committed<()>> {
        let cost = match import.validate() {
            Ok(cost) => cost,
            Err(error) => return Completion::settled(Err(error)),
        };
        self.storage
            .submit_mutation(cost, move |cx| write_import(cx, &import))
    }
}

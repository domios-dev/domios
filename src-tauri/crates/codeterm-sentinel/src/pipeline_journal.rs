use super::storage::{finishing, Failure, Staged};
use super::store::Store;
use codeterm_storage::{
    BlueprintJournalEntry, BlueprintJournalWrite, Completion, Fence, MutationOutcome, Read,
    SentinelId,
};
use std::collections::BTreeMap;

impl Store {
    pub fn stage_pipeline_journal(
        &mut self,
        id: &str,
        write: BlueprintJournalWrite,
    ) -> Staged<bool> {
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        let Some(job) = self.get(id).filter(|job| job.pipeline().is_some()) else {
            return Staged::ready(Err("blueprint pipeline not found".into()));
        };
        let id = match SentinelId::new(id.to_string()) {
            Ok(id) => id,
            Err(error) => return Staged::ready(Err(error.to_string())),
        };
        let scope = (job.workspace_id.clone(), job.team_id.clone());
        self.submit(
            move |repo| repo.write_blueprint_journal(id, write),
            finishing(move |store, outcome| match outcome {
                Ok(commit) => {
                    let applied = commit.outcome == MutationOutcome::Applied;
                    let changed = applied
                        || matches!(commit.outcome, MutationOutcome::Fenced(Fence::Claimed));
                    if changed {
                        if let Some(revision) = store.committed_revisions.get(&scope).cloned() {
                            store.changes.insert(scope.clone(), revision);
                        }
                    }
                    Ok(applied)
                }
                Err(Failure::Failed(message) | Failure::Refused(message)) => Err(message),
            }),
        )
    }

    pub fn typed_pipeline_log(
        &self,
        id: &str,
        limit: u16,
    ) -> Result<Completion<Read<Vec<BlueprintJournalEntry>>>, String> {
        let Some(job) = self.get(id) else {
            return Err("blueprint not found".into());
        };
        if job.pipeline().is_none() {
            return Err("blueprint has no pipeline journal".into());
        }
        let Some(backend) = self.backend.sqlite() else {
            return Err("blueprint journal requires typed Storage".into());
        };
        let id = SentinelId::new(job.id.clone()).map_err(|error| error.to_string())?;
        Ok(backend.repo.blueprint_log(id, limit))
    }

    pub fn typed_pipeline_logs(
        &self,
        ids: Vec<String>,
        limit: u16,
    ) -> Result<Completion<Read<BTreeMap<String, Vec<BlueprintJournalEntry>>>>, String> {
        let Some(backend) = self.backend.sqlite() else {
            return Err("blueprint journal requires typed Storage".into());
        };
        let ids = ids
            .into_iter()
            .map(SentinelId::new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok(backend.repo.blueprint_logs(ids, limit))
    }
}

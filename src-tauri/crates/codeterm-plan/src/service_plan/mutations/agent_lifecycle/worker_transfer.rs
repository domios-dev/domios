use super::*;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedWorkerTransferBinding {
    pub plan_id: String,
    pub team_id: String,
    pub owner_tab_id: String,
    pub owner_session_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub worker_tab_id: String,
    pub workspace_id: String,
    pub worktree_path: String,
    pub session_id: String,
    pub binding_nonce: String,
    pub provider: String,
    pub owner_epoch: u64,
}

impl PlanStore {
    pub fn with_verified_worker_transfer_binding<F>(
        &self,
        expected: &VerifiedWorkerTransferBinding,
        commit_staged_mux: F,
    ) -> Result<(), PlanError>
    where
        F: FnOnce(),
    {
        let _writer_turn = self.mutation_writer.enter();
        let read = self.database.worker_transfer_projection(expected)?;
        if read.assignments != 1
            || read.lanes != 1
            || read.worktrees != 1
            || read.open_attempts != 1
        {
            return Err(refused(
                "worker assignment, lane, or open attempt is not unique",
            ));
        }
        let Some(subject) = read.subject else {
            return Err(refused("the verified task attempt no longer exists"));
        };
        if !subject.matches(expected) {
            return Err(refused(
                "the task attempt no longer matches its verified owner",
            ));
        }

        commit_staged_mux();
        Ok(())
    }
}

fn refused(reason: &str) -> PlanError {
    PlanError::Io(format!("worker transfer refused: {reason}"))
}

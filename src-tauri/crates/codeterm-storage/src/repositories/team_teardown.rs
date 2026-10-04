use crate::owner::{Storage, WriteCx};
use crate::repositories::cron::{apply_mutations, CronApplied, CronScope, CronWrite};
use crate::repositories::desk::remove_team;
use crate::repositories::sentinel::runtime::remove_team_rows;
use crate::repositories::sentinel::{MutationOutcome, SentinelCommit};
use crate::repositories::{Committed, Completion, Revision};
use crate::StorageError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamTeardown {
    pub team_id: String,
    pub expected_desk_team_revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamTeardownApplied {
    pub cron: CronApplied,
    pub sentinel: SentinelCommit,
}

impl Storage {
    pub fn team_teardown(
        &self,
        teardown: TeamTeardown,
    ) -> Completion<Committed<TeamTeardownApplied>> {
        let cost = teardown.team_id.len();
        self.submit_mutation(cost, move |cx: &mut WriteCx<'_>| {
            apply_team_teardown(cx, teardown)
        })
    }
}

fn apply_team_teardown(
    cx: &mut WriteCx<'_>,
    teardown: TeamTeardown,
) -> Result<TeamTeardownApplied, StorageError> {
    remove_team(cx, &teardown.team_id, teardown.expected_desk_team_revision)?;
    let cron = apply_mutations(
        cx,
        vec![CronWrite::RemoveScope(CronScope::Team(
            teardown.team_id.clone(),
        ))],
    )?;
    let removed = remove_team_rows(cx, &teardown.team_id)?;
    let sentinel = SentinelCommit {
        outcome: if removed.is_empty() {
            MutationOutcome::Unchanged
        } else {
            MutationOutcome::Applied
        },
        sentinels: Vec::new(),
        removed,
    };
    Ok(TeamTeardownApplied { cron, sentinel })
}

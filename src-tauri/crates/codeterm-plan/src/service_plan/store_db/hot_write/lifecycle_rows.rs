use super::*;
use crate::plan::{
    OrchestratorRef, PlanLifecycleEvent, PlanOrphan, PlanStatus, PlannerInboxEntry, PlannerRef,
    PlannerRequest, TransferRecord,
};
use crate::service_plan::mutations::agent_lifecycle::AgentLifecyclePlanChange;
use crate::service_plan::store_db::queue_operation;
use crate::service_plan::PlanMatch;
use rusqlite::TransactionBehavior;

mod persist;

pub(in crate::service_plan) type TeamOwners = Vec<(String, Option<(String, String)>)>;

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub(in crate::service_plan) struct LifecyclePlanRows {
    pub id: String,
    pub title: String,
    pub project_path: String,
    pub status: PlanStatus,
    pub owner: OrchestratorRef,
    pub planner: Option<PlannerRef>,
    pub team_id: Option<String>,
    pub orphaned: Option<PlanOrphan>,
    pub updated_at: u64,
    #[serde(default)]
    pub revision: i64,
    #[serde(default)]
    pub owner_history_len: usize,
    #[serde(default)]
    pub lifecycle_events_len: usize,
    #[serde(default)]
    pub stride_count: usize,
    #[serde(default)]
    pub stride_digest: i64,
    #[serde(default)]
    pub stride_digest_invalid: bool,
    #[serde(default)]
    pub owner_history: Vec<TransferRecord>,
    #[serde(default)]
    pub lifecycle_events: Vec<PlanLifecycleEvent>,
    #[serde(default)]
    pub planner_inbox: Vec<PlannerInboxEntry>,
    #[serde(default)]
    pub planner_requests: Vec<PlannerRequest>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub strides: Vec<Milestone>,
    #[serde(default)]
    pub unfinished_task_ids: Vec<String>,
    #[serde(default)]
    pub decoded_bytes: usize,
    #[serde(default)]
    pub projected_text_bytes: usize,
}

#[derive(Clone, Debug)]
pub(in crate::service_plan) struct LifecyclePatch {
    pub before: LifecyclePlanRows,
    pub after: LifecyclePlanRows,
}

#[derive(Debug)]
pub(in crate::service_plan) struct LifecycleUndo {
    pub patches: Vec<LifecyclePatch>,
    pub revisions: Vec<(String, i64)>,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn observe_lifecycle_rows(
        &self,
        selector: &PlanMatch,
        changes: &[AgentLifecyclePlanChange],
    ) -> Result<(Vec<LifecyclePlanRows>, TeamOwners), PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        let tabs: Vec<_> = changes.iter().map(|change| change.tab_id.clone()).collect();
        let mut plans = read_rows(&tx, selector, &tabs, None, true)?;
        for plan in &mut plans {
            let planner_changes = changes.iter().any(|change| {
                matches!(&plan.planner, Some(PlannerRef::Tab { tab_id, .. }) if tab_id == &change.tab_id)
                    || change.planner_binding.as_ref().is_some_and(|(id, planner)| id == &plan.id && planner != &plan.planner)
                    || change.plan_rebind.as_ref().is_some_and(|rebind| rebind.plan_id == plan.id && rebind.clear_planner && plan.planner.is_some())
            });
            if planner_changes {
                let mut work = CommittedRows::default();
                plan.planner_inbox =
                    header_edits::read_field(&tx, &plan.id, "$.planner_inbox", &mut work)?;
                plan.planner_requests =
                    header_edits::read_field(&tx, &plan.id, "$.planner_requests", &mut work)?;
                plan.decoded_bytes += work.decoded_bytes;
            }
        }
        let teams: BTreeSet<_> = changes
            .iter()
            .filter_map(|change| {
                change
                    .plan_rebind
                    .as_ref()
                    .map(|rebind| rebind.team_id.clone())
            })
            .collect();
        let owners = read_team_owners(&tx, teams.iter().map(String::as_str))?;
        tx.commit().map_err(db_error)?;
        Ok((plans, owners))
    }

    pub(in crate::service_plan) fn observe_team_rows(
        &self,
        selector: &PlanMatch,
        team: Option<&str>,
        tabs: &[String],
        close_tasks: bool,
    ) -> Result<Vec<LifecyclePlanRows>, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        let plans = read_rows(&tx, selector, tabs, team, close_tasks)?;
        tx.commit().map_err(db_error)?;
        Ok(plans)
    }

    pub(in crate::service_plan) fn commit_lifecycle_rows(
        &self,
        selector: &PlanMatch,
        expected: &[(String, i64)],
        team_owners: &TeamOwners,
        patches: Vec<LifecyclePatch>,
    ) -> Result<(LifecycleUndo, Vec<CommittedRows>), PlanError> {
        let receipts = self.lifecycle_transaction(&patches, false, |tx| {
            if revisions(tx, selector)? != expected
                || read_team_owners(tx, team_owners.iter().map(|(team, _)| team.as_str()))?
                    != *team_owners
            {
                return Err(conflict());
            }
            Ok(())
        })?;
        let revisions = receipts
            .iter()
            .map(|receipt| (receipt.plan_id.clone(), receipt.revision))
            .collect();
        Ok((LifecycleUndo { patches, revisions }, receipts))
    }

    pub(in crate::service_plan) fn rollback_lifecycle_rows(
        &self,
        undo: LifecycleUndo,
    ) -> Result<Vec<CommittedRows>, PlanError> {
        self.lifecycle_transaction(&undo.patches, true, |tx| {
            for (id, expected) in &undo.revisions {
                if super::super::change_feed::revision(tx, id)? != *expected {
                    return Err(conflict());
                }
            }
            Ok(())
        })
    }

    fn lifecycle_transaction(
        &self,
        patches: &[LifecyclePatch],
        rollback: bool,
        validate: impl FnOnce(&Transaction<'_>) -> Result<(), PlanError>,
    ) -> Result<Vec<CommittedRows>, PlanError> {
        let mut connection = self.connection.lock();
        let operations: Vec<_> = patches
            .iter()
            .map(|patch| {
                let now = if rollback {
                    patch.before.updated_at
                } else {
                    patch.after.updated_at
                };
                queue_operation(&connection, &patch.before.id, "lifecycle_rows", now)
                    .map(|id| (id, now))
            })
            .collect::<Result<_, _>>()?;
        let result: Result<Vec<CommittedRows>, PlanError> = (|| {
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db_error)?;
            validate(&tx)?;
            let mut receipts = Vec::new();
            for (patch, (operation, now)) in patches.iter().zip(&operations) {
                tx.execute(
                    "UPDATE plan_operations SET state='running' WHERE id=?1",
                    [operation],
                )
                .map_err(db_error)?;
                let previous_revision = super::super::change_feed::revision(&tx, &patch.before.id)?;
                let mut receipt = CommittedRows {
                    plan_id: patch.before.id.clone(),
                    updated_at: *now,
                    previous_revision,
                    revision: super::super::change_feed::next_revision(
                        previous_revision,
                        *operation,
                    )?,
                    decoded_bytes: patch.before.decoded_bytes,
                    projected_text_bytes: patch.before.projected_text_bytes,
                    ..Default::default()
                };
                persist::apply(&tx, patch, rollback, &mut receipt)?;
                touch_header(&tx, &mut receipt)?;
                tx.execute(
                    "UPDATE plan_operations SET state='committed',finished_at=?2 WHERE id=?1",
                    params![operation, now],
                )
                .map_err(db_error)?;
                receipts.push(receipt);
            }
            tx.commit().map_err(db_error)?;
            for receipt in &receipts {
                self.record_row_change(receipt);
            }
            Ok(receipts)
        })();
        if let Err(error) = &result {
            for (operation, now) in operations {
                let _ = connection.execute(
                    "UPDATE plan_operations SET state='failed',finished_at=?2,error=?3 WHERE id=?1",
                    params![operation, now, error.to_string()],
                );
            }
        }
        result
    }
}

pub(in crate::service_plan) fn expected(plans: &[LifecyclePlanRows]) -> Vec<(String, i64)> {
    plans
        .iter()
        .map(|plan| (plan.id.clone(), plan.revision))
        .collect()
}

fn revisions(tx: &Transaction<'_>, selector: &PlanMatch) -> Result<Vec<(String, i64)>, PlanError> {
    let (clause, values) = crate::service_plan::plan_match::compile(selector);
    let mut statement = tx.prepare_cached(&format!("SELECT p.id,coalesce(json_extract(p.header_json,'$._revision'),0) FROM plans p WHERE {clause} ORDER BY p.id")).map_err(db_error)?;
    let result = statement
        .query_map(rusqlite::params_from_iter(values), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(db_error)?
        .collect::<Result<_, _>>()
        .map_err(db_error);
    result
}

pub(in crate::service_plan) fn read_team_owners<'a>(
    tx: &Transaction<'_>,
    teams: impl Iterator<Item = &'a str>,
) -> Result<TeamOwners, PlanError> {
    teams.map(|team| {
        let owner = tx.query_row("SELECT id,title FROM plans WHERE team_id=?1 AND status='active' ORDER BY id LIMIT 1", [team], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(db_error)?;
        Ok((team.to_string(), owner))
    }).collect()
}

fn read_rows(
    tx: &Transaction<'_>,
    selector: &PlanMatch,
    tabs: &[String],
    team: Option<&str>,
    close_tasks: bool,
) -> Result<Vec<LifecyclePlanRows>, PlanError> {
    let mut out = Vec::new();
    let tabs_json = serde_json::to_string(tabs).map_err(json_error)?;
    for (id, _) in revisions(tx, selector)? {
        let (document, revision, owner_history_len, lifecycle_events_len, stride_count, stride_digest, stride_digest_invalid): (String, i64, usize, usize, usize, i64, bool) = tx.query_row("SELECT json_object('id',id,'title',title,'project_path',project_path,'status',status,'team_id',team_id,'owner',json(coalesce(json_extract(header_json,'$.owner'),'{}')),'planner',json(json_extract(header_json,'$.planner')),'orphaned',json(json_extract(header_json,'$.orphaned')),'updated_at',updated_at), coalesce(json_extract(header_json,'$._revision'),0), coalesce(json_array_length(header_json,'$.owner_history'),0), coalesce(json_array_length(header_json,'$.lifecycle_events'),0), stride_count, stride_digest, coalesce(json_extract(header_json,'$._stride_digest_invalid'),0) FROM plans WHERE id=?1", [&id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?))).map_err(db_error)?;
        let mut plan: LifecyclePlanRows = serde_json::from_str(&document).map_err(json_error)?;
        plan.revision = revision;
        plan.owner_history_len = owner_history_len;
        plan.lifecycle_events_len = lifecycle_events_len;
        plan.stride_count = stride_count;
        plan.stride_digest = stride_digest;
        plan.stride_digest_invalid = stride_digest_invalid;
        plan.decoded_bytes = document.len();
        let mut statement = tx.prepare_cached("SELECT task_id FROM plan_tasks t WHERE plan_id=?1 AND (assigned_tab_id IN (SELECT value FROM json_each(?2)) OR json_extract(row_json,'$.lane.tab_id') IN (SELECT value FROM json_each(?2)) OR (?3 IS NOT NULL AND (EXISTS(SELECT 1 FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.closed_at') IS NULL AND a.executor_tab_id IN (SELECT value FROM json_each(?2))) OR (?3=(SELECT team_id FROM plans WHERE id=?1) AND status='in_progress' AND assigned_tab_id IS NULL AND json_extract(row_json,'$.lane.tab_id') IS NULL)))) ORDER BY position").map_err(db_error)?;
        let ids: Vec<String> = if close_tasks {
            statement
                .query_map(params![id, tabs_json, team], |row| row.get(0))
                .map_err(db_error)?
                .collect::<Result<_, _>>()
                .map_err(db_error)?
        } else {
            Vec::new()
        };
        let mut work = CommittedRows::default();
        for task_id in ids {
            let mut task = read_task(tx, &id, &task_id, &mut work)?;
            task.execution_attempts = review_rows::read_attempts(
                tx,
                &id,
                &task_id,
                &TaskHistory::LiveAndReported,
                &mut work,
            )?
            .into_iter()
            .map(|(_, attempt)| attempt)
            .collect();
            plan.tasks.push(task);
        }
        if team.is_some() {
            let mut statement = tx.prepare_cached("SELECT task_id FROM plan_tasks WHERE plan_id=?1 AND status NOT IN ('done','cancelled') ORDER BY position").map_err(db_error)?;
            plan.unfinished_task_ids = statement
                .query_map([&id], |row| row.get(0))
                .map_err(db_error)?
                .collect::<Result<_, _>>()
                .map_err(db_error)?;
            work.projected_text_bytes += plan
                .unfinished_task_ids
                .iter()
                .map(String::len)
                .sum::<usize>();
        }
        plan.decoded_bytes += work.decoded_bytes;
        plan.projected_text_bytes += work.projected_text_bytes;
        out.push(plan);
    }
    Ok(out)
}

fn conflict() -> PlanError {
    PlanError::Io("lifecycle conflict: plan changed during preparation; retry by intent".into())
}

pub(in crate::service_plan) fn patch(
    before: LifecyclePlanRows,
    mut after: LifecyclePlanRows,
    now: u64,
) -> Option<LifecyclePatch> {
    if before == after {
        return None;
    }
    after.updated_at = now;
    if before.status != after.status {
        let kind = if after.status == PlanStatus::Active {
            "activated"
        } else {
            "archived"
        };
        let author = if after.status == PlanStatus::Archived {
            before.owner.clone()
        } else {
            after.owner.clone()
        };
        after.lifecycle_events.push(PlanLifecycleEvent {
            id: format!("lifecycle-{kind}-{}", before.lifecycle_events_len),
            kind: kind.into(),
            at: now,
            author,
        });
    }
    Some(LifecyclePatch { before, after })
}

#[cfg(test)]
pub(in crate::service_plan) fn from_test_plan(plan: &crate::plan::Plan) -> LifecyclePlanRows {
    let mut document = serde_json::to_value(plan).unwrap();
    for (key, value) in [
        ("revision", serde_json::json!(0)),
        (
            "owner_history_len",
            serde_json::json!(plan.owner_history.len()),
        ),
        (
            "lifecycle_events_len",
            serde_json::json!(plan.lifecycle_events.len()),
        ),
        ("stride_count", serde_json::json!(plan.strides.len())),
        ("stride_digest", serde_json::json!(0)),
        ("stride_digest_invalid", serde_json::json!(false)),
        (
            "unfinished_task_ids",
            serde_json::json!(plan
                .tasks
                .iter()
                .filter(|task| !task.status.is_terminal())
                .map(|task| task.id.clone())
                .collect::<Vec<_>>()),
        ),
    ] {
        document[key] = value;
    }
    serde_json::from_value(document).unwrap()
}

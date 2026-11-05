use super::{column, decode, record_query};
use crate::plan::WorkerPlacement;
use crate::service_plan::{PlanError, PlanStore};
use rusqlite::OptionalExtension;

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TransferLaneRead {
    pub tab_id: Option<String>,
    pub workspace_id: Option<String>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TransferInterruptionRead {
    pub owner_epoch: u64,
    pub owner_tab_id: Option<String>,
    pub owner_session_id: Option<String>,
    pub handoff_required: bool,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TransferAttemptRead {
    pub executor_tab_id: String,
    pub session_id: Option<String>,
    pub session_binding_nonce: Option<String>,
    pub provider: Option<String>,
    pub placement: WorkerPlacement,
    pub acknowledged_at: Option<u64>,
    pub closed_at: Option<u64>,
    pub retired_at: Option<u64>,
    pub has_mesh_peer: bool,
    pub has_remote_dispatch_state: bool,
    pub worktree_path: Option<String>,
    pub owner_epoch: u64,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct TransferTaskAuthorityRead {
    pub lane: TransferLaneRead,
    pub interruption: TransferInterruptionRead,
    pub has_checkpoint: bool,
    pub attempt: Option<TransferAttemptRead>,
}

impl PlanStore {
    pub fn transfer_task_authority_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<TransferTaskAuthorityRead, PlanError> {
        self.database
            .transfer_task_authority_projection(plan, task, attempt)
    }
}

impl super::PlanDatabase {
    pub(in crate::service_plan) fn transfer_task_authority_projection(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
    ) -> Result<TransferTaskAuthorityRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let encoded: String = connection.query_row(
            "SELECT json_object('lane',json_object(
                'tab_id',json_extract(t.row_json,'$.lane.tab_id'),'workspace_id',json_extract(t.row_json,'$.lane.workspace_id'),
                'worktree_path',json_extract(t.row_json,'$.lane.worktree_path'),'branch',json_extract(t.row_json,'$.lane.branch')),
                'interruption',json_object('owner_epoch',coalesce(json_extract(t.row_json,'$.interruption.owner_epoch'),0),
                    'owner_tab_id',json_extract(t.row_json,'$.interruption.owner_tab_id'),
                    'owner_session_id',json_extract(t.row_json,'$.interruption.owner_session_id'),
                    'handoff_required',json(coalesce(t.row_json->'$.interruption.handoff_required','false'))),
                'has_checkpoint',json(CASE WHEN json_extract(t.row_json,'$.interruption.checkpoint') IS NOT NULL THEN 'true' ELSE 'false' END),
                'attempt',json((SELECT json_object('executor_tab_id',a.executor_tab_id,'session_id',a.session_id,
                    'session_binding_nonce',a.session_binding_nonce,'provider',json_extract(a.row_json,'$.provider'),
                    'placement',coalesce(json_extract(a.row_json,'$.placement'),'isolated_lane'),
                    'acknowledged_at',json_extract(a.row_json,'$.acknowledged_at'),'closed_at',json_extract(a.row_json,'$.closed_at'),
                    'retired_at',json_extract(a.row_json,'$.retired_at'),
                    'has_mesh_peer',json(CASE WHEN json_extract(a.row_json,'$.mesh_peer') IS NOT NULL THEN 'true' ELSE 'false' END),
                    'has_remote_dispatch_state',json(CASE WHEN json_extract(a.row_json,'$.remote_dispatch_state') IS NOT NULL THEN 'true' ELSE 'false' END),
                    'worktree_path',json_extract(a.row_json,'$.worktree_path'),
                    'owner_epoch',coalesce(json_extract(a.row_json,'$.interruption.owner_epoch'),0))
                    FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.id')=?3 ORDER BY a.position LIMIT 1)))
             FROM plan_tasks t WHERE t.plan_id=?1 AND t.task_id=?2 ORDER BY t.position LIMIT 1",
            rusqlite::params![plan,task,attempt], |row| column(row,0),
        ).optional().map_err(super::super::db_error)?.ok_or(PlanError::TaskNotFound)?;
        decode(plan, &encoded)
    }
}

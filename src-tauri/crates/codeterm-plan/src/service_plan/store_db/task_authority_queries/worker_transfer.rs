use super::*;
use crate::service_plan::mutations::agent_lifecycle::VerifiedWorkerTransferBinding;
use crate::service_plan::task_authority_projection::worker_transfer::WorkerTransferRead;

impl PlanDatabase {
    pub(in crate::service_plan) fn worker_transfer_projection(
        &self,
        expected: &VerifiedWorkerTransferBinding,
    ) -> Result<WorkerTransferRead, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let (assignments,lanes,worktrees,open_attempts,subject): (u64,u64,u64,u64,Option<String>) = connection.query_row(
            "SELECT
                (SELECT count(*) FROM plan_tasks WHERE status IN ('pending','in_progress','in_review','blocked','paused') AND assigned_tab_id=?1),
                (SELECT count(*) FROM plan_tasks WHERE status IN ('pending','in_progress','in_review','blocked','paused') AND json_extract(row_json,'$.lane.tab_id')=?1),
                (SELECT count(*) FROM plan_tasks WHERE status IN ('pending','in_progress','in_review','blocked','paused') AND lane_worktree_path=?2),
                (SELECT count(*) FROM plan_task_attempts WHERE executor_tab_id=?1 AND open=1),
                (SELECT json_object(
                    'plan_status',p.status,'plan_mode',coalesce(json_extract(p.header_json,'$.mode'),'planning'),
                    'team_id',p.team_id,'plan_owner_tab',json_extract(p.header_json,'$.owner.tab_id'),
                    'plan_owner_session',json_extract(p.header_json,'$.owner.session_id'),
                    'task_status',t.status,'assigned_tab',t.assigned_tab_id,
                    'lane_tab',json_extract(t.row_json,'$.lane.tab_id'),
                    'workspace_id',json_extract(t.row_json,'$.lane.workspace_id'),
                    'lane_path',t.lane_worktree_path,'lane_branch',t.lane_branch,'lane_base',json_extract(t.row_json,'$.lane.base'),
                    'handoff_required',json(coalesce(t.row_json->'$.interruption.handoff_required','false')),
                    'has_checkpoint',json(CASE WHEN json_extract(t.row_json,'$.interruption.checkpoint') IS NOT NULL THEN 'true' ELSE 'false' END),
                    'task_owner_tab',json_extract(t.row_json,'$.interruption.owner_tab_id'),
                    'task_owner_session',json_extract(t.row_json,'$.interruption.owner_session_id'),
                    'task_owner_epoch',coalesce(json_extract(t.row_json,'$.interruption.owner_epoch'),0),
                    'executor_tab',a.executor_tab_id,'session_id',a.session_id,'nonce',a.session_binding_nonce,
                    'provider',json_extract(a.row_json,'$.provider'),'agent_type',json_extract(a.row_json,'$.agent_type'),
                    'acknowledged_at',json_extract(a.row_json,'$.acknowledged_at'),
                    'closed_at',json_extract(a.row_json,'$.closed_at'),
                    'retired_at',json_extract(a.row_json,'$.retired_at'),
                    'attempt_owner_epoch',coalesce(json_extract(a.row_json,'$.interruption.owner_epoch'),0),
                    'placement',coalesce(json_extract(a.row_json,'$.placement'),'isolated_lane'),
                    'has_remote_metadata',json(CASE WHEN json_extract(a.row_json,'$.mesh_peer') IS NOT NULL
                        OR json_extract(a.row_json,'$.remote_dispatch_state') IS NOT NULL
                        OR json_extract(a.row_json,'$.remote_worktree_path') IS NOT NULL
                        OR json_extract(a.row_json,'$.remote_lane_tip') IS NOT NULL
                        OR json_extract(a.row_json,'$.remote_merge_commit') IS NOT NULL THEN 'true' ELSE 'false' END),
                    'attempt_path',json_extract(a.row_json,'$.worktree_path'),
                    'attempt_branch',json_extract(a.row_json,'$.branch'),'attempt_base',json_extract(a.row_json,'$.base'))
                 FROM plans p JOIN plan_tasks t ON t.plan_id=p.id
                    JOIN plan_task_attempts a ON a.plan_id=t.plan_id AND a.task_id=t.task_id
                 WHERE p.id=?3 AND t.task_id=?4 AND json_extract(a.row_json,'$.id')=?5
                 ORDER BY a.position DESC LIMIT 1)",
            params![expected.worker_tab_id,expected.worktree_path,expected.plan_id,expected.task_id,expected.attempt_id],
            |row| Ok((column(row,0)?,column(row,1)?,column(row,2)?,column(row,3)?,column(row,4)?)),
        ).map_err(db_error)?;
        Ok(WorkerTransferRead {
            assignments,
            lanes,
            worktrees,
            open_attempts,
            subject: subject
                .map(|raw| decode(&expected.plan_id, &raw))
                .transpose()?,
        })
    }
}

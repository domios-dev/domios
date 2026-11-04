use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{LoopSpec, Milestone, OrchestratorRef, Task};
use crate::service_plan::worker_charter::WorkerAssignmentRead;
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OptionalExtension};

#[derive(serde::Deserialize)]
struct OwnerRead {
    id: String,
    owner: OrchestratorRef,
}

fn active_owned_plan(connection: &Connection, tab: &str) -> Result<Option<String>, PlanError> {
    if tab.trim().is_empty() {
        return Ok(None);
    }
    record_query();
    let document: Option<(String,String)>=connection.query_row(
        "SELECT p.id,json_object('id',p.id,'owner',json_object('tab_id',json_extract(p.header_json,'$.owner.tab_id')))
         FROM plans p WHERE p.status='active' AND json_extract(p.header_json,'$.mode')='execution' AND p.owner_tab_id=?1
            AND EXISTS(SELECT 1 FROM plan_tasks t WHERE t.plan_id=p.id AND t.status IN ('pending','in_progress','in_review','blocked'))
         ORDER BY p.updated_at DESC,p.id LIMIT 1",[tab],|row|Ok((column(row,0)?,column(row,1)?)),
    ).optional().map_err(db_error)?;
    document
        .map(|(id, document)| {
            let selected: OwnerRead = decode(&id, &document)?;
            if selected.owner.tab_id.as_deref() != Some(tab) {
                return Err(PlanError::Validation("owner binding changed".into()));
            }
            Ok(selected.id)
        })
        .transpose()
}

impl PlanDatabase {
    pub(in crate::service_plan) fn active_owned_open_plan(
        &self,
        tab: &str,
    ) -> Result<Option<String>, PlanError> {
        active_owned_plan(&self.connection.lock(), tab)
    }

    pub(in crate::service_plan) fn worker_assignment_projection(
        &self,
        tab: &str,
    ) -> Result<Option<WorkerAssignmentRead>, PlanError> {
        if tab.trim().is_empty() {
            return Ok(None);
        }
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        if active_owned_plan(&tx, tab)?.is_some() {
            return Ok(None);
        }
        record_query();
        let selected=tx.query_row(
            "SELECT p.id,t.task_id,
                json_object('id',json_extract(t.row_json,'$.id'),'name',json_extract(t.row_json,'$.name'),'type','','status','pending',
                    'personality',json_extract(t.row_json,'$.personality'),'role_guidance',json_extract(t.row_json,'$.role_guidance')),
                json_object('id',p.id,'status',p.status,'project_path',p.project_path,
                    'canonical_project_path',CASE WHEN json_type(p.header_json,'$.canonical_project_path') IS NULL THEN '' ELSE json_extract(p.header_json,'$.canonical_project_path') END),
                (SELECT json_object('id',json_extract(l.value,'$.id'),'target',CASE WHEN json_type(l.value,'$.target')='text' THEN '' ELSE json_extract(l.value,'$.target') END,'budget',json_extract(l.value,'$.budget'),
                    'count',json_extract(l.value,'$.count'),'status','running','stagnation',0,'stop_when',json_extract(l.value,'$.stop_when'))
                    FROM json_each(p.header_json,'$.loops') l WHERE json_extract(l.value,'$.status')='running'
                    AND trim(json_extract(l.value,'$.target'),?2)=json_extract(t.row_json,'$.id') ORDER BY l.key LIMIT 1),
                (SELECT COUNT(*) FROM plan_task_attempts a WHERE a.plan_id=p.id AND a.task_id=t.task_id AND json_extract(a.row_json,'$.closed_at') IS NULL)
             FROM plans p JOIN plan_tasks t ON t.plan_id=p.id WHERE p.status='active' AND json_extract(p.header_json,'$.mode')='execution'
                AND t.status IN ('pending','in_progress','blocked') AND (CASE WHEN t.assigned_tab_id=?1
                    THEN CASE WHEN json_type(t.row_json,'$.assigned_tab_id')='text' THEN 1 ELSE json('invalid task binding') END ELSE 0 END
                    OR json_extract(t.row_json,'$.lane.tab_id')=?1 OR EXISTS(SELECT 1 FROM plan_task_attempts a
                        WHERE a.plan_id=p.id AND a.task_id=t.task_id AND CASE WHEN a.executor_tab_id=?1
                            THEN CASE WHEN json_type(a.row_json,'$.executor_tab_id')='text' THEN 1 ELSE json('invalid executor binding') END ELSE 0 END
                        AND json_extract(a.row_json,'$.closed_at') IS NULL))
             ORDER BY p.updated_at DESC,p.id,t.position LIMIT 1",
            params![tab,super::status_queries::WHITESPACE],|row|Ok((column::<String>(row,0)?,column::<String>(row,1)?,column::<String>(row,2)?,column::<String>(row,3)?,column::<Option<String>>(row,4)?,row.get::<_,usize>(5)?)),
        ).optional().map_err(db_error)?;
        let Some((plan_id, task_id, document, storage, loop_spec, live_attempt_count)) = selected
        else {
            return Ok(None);
        };
        let mut task: Task = decode(&plan_id, &document)?;
        let storage: super::auxiliary_queries::StorageMetadata = decode(&plan_id, &storage)?;
        let loop_spec = loop_spec
            .map(|document| decode::<LoopSpec>(&plan_id, &document))
            .transpose()?;
        record_query();
        let mut statement=tx.prepare_cached(
            "SELECT json_object('id',json_extract(row_json,'$.id'),'from',json('{}'),'kind',json_extract(row_json,'$.kind'),
                'text',json_extract(row_json,'$.text'),'ts',json(row_json->'$.ts')) FROM plan_task_milestones
             WHERE plan_id=?1 AND task_id=?2 AND domios_worker_stride_rank(json_extract(row_json,'$.kind')) IS NOT NULL
                AND trim(coalesce(json_extract(row_json,'$.text'),''),?3)<>''
             ORDER BY domios_worker_stride_rank(json_extract(row_json,'$.kind')),domios_u64_order(row_json->'$.ts') DESC,json_extract(row_json,'$.id') DESC,seq LIMIT ?4"
        ).map_err(db_error)?;
        for document in statement
            .query_map(
                params![
                    plan_id,
                    task_id,
                    super::status_queries::WHITESPACE,
                    crate::service_plan::stride_context::WORKER_STRIDE_LIMIT
                ],
                |row| column::<String>(row, 0),
            )
            .map_err(db_error)?
        {
            task.milestones
                .push(decode::<Milestone>(&plan_id, &document.map_err(db_error)?)?);
        }
        drop(statement);
        tx.commit().map_err(db_error)?;
        drop(connection);
        Ok(Some(WorkerAssignmentRead {
            plan_id,
            state_dir: storage.state_dir_unchecked(),
            task,
            loop_spec,
            live_attempt_count,
        }))
    }
}

use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{Plan, Section, Task};
use crate::service_plan::plan_match::{self, PlanMatch};
use crate::service_plan::task_queries::{include_task, TaskListFilter};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OptionalExtension};

pub(super) const TASK_HISTORY_LIMIT: usize = 50;
pub(super) const TASK_ATTEMPT_LIMIT: usize = 8;

const HEADER: &str = "json_object(
    'id',p.id,'title',p.title,'status',p.status,'project_path',p.project_path,
    'owner',json_extract(p.header_json,'$.owner'),
    'created_at',json_extract(p.header_json,'$.created_at'),'updated_at',p.updated_at,
    'task_groups',json(coalesce(json_extract(p.header_json,'$.task_groups'),'[]')))";

const TASK_METADATA: &str = "json_object(
    'id',task_id,'name',json_extract(row_json,'$.name'),'type',json_extract(row_json,'$.type'),
    'status',status,'assigned_tab_id',assigned_tab_id,
    'key',json_extract(row_json,'$.key'),'handle',json_extract(row_json,'$.handle'),
    'parent_task_id',json_extract(row_json,'$.parent_task_id'),
    'depends_on',json(coalesce(json_extract(row_json,'$.depends_on'),'[]')),
    'group',json_extract(row_json,'$.group'),
    'review_stage',coalesce(json_extract(row_json,'$.review_stage'),'review'),
    'qa_state',json_extract(row_json,'$.qa_state'))";

impl PlanDatabase {
    pub(in crate::service_plan) fn task_list_snapshot(
        &self,
        predicate: &PlanMatch,
        filter: &TaskListFilter,
    ) -> Result<Vec<Plan>, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        let mut plans = task_contexts(&transaction, predicate)?;
        for plan in &mut plans {
            if !super::super::task_queries::plan_in_scope(filter, plan) {
                continue;
            }
            for task in &mut plan.tasks {
                if include_task(filter, task) {
                    *task = selected_task(&transaction, &plan.id, &task.id, true)?;
                }
            }
        }
        transaction.commit().map_err(db_error)?;
        Ok(plans)
    }

    pub(in crate::service_plan) fn task_detail_snapshot(
        &self,
        predicate: &PlanMatch,
        task_ref: &str,
        bounded: bool,
    ) -> Result<Option<Plan>, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        let mut result = None;
        for mut plan in task_contexts(&transaction, predicate)? {
            let Some(index) = crate::plan::task_key::find_task_index(&plan.tasks, task_ref) else {
                continue;
            };
            plan.tasks[index] =
                selected_task(&transaction, &plan.id, &plan.tasks[index].id, bounded)?;
            result = Some(plan);
            break;
        }
        transaction.commit().map_err(db_error)?;
        Ok(result)
    }
}

fn task_contexts(connection: &Connection, predicate: &PlanMatch) -> Result<Vec<Plan>, PlanError> {
    let (clause, params) = plan_match::compile(predicate);
    record_query();
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT p.id, {HEADER} FROM plans p WHERE {clause} ORDER BY p.updated_at DESC, p.id"
        ))
        .map_err(db_error)?;
    let headers = statement
        .query_map(rusqlite::params_from_iter(params), |row| {
            Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    let mut plans = Vec::new();
    for (id, header) in headers {
        let mut plan: Plan = decode(&id, &header)?;
        record_query();
        let mut tasks = connection
            .prepare_cached(&format!(
                "SELECT {TASK_METADATA}, coalesce(json_extract(row_json, '$.interruption.handoff_required'), 0) FROM plan_tasks WHERE plan_id = ?1 ORDER BY position"
            ))
            .map_err(db_error)?;
        for document in tasks
            .query_map([&id], |row| {
                Ok((column::<String>(row, 0)?, column::<bool>(row, 1)?))
            })
            .map_err(db_error)?
        {
            let (document, handoff_required) = document.map_err(db_error)?;
            let mut task: Task = decode(&id, &document)?;
            task.interruption.handoff_required = handoff_required;
            plan.tasks.push(task);
        }
        record_query();
        let mut sections = connection.prepare_cached(
            "SELECT json_object('id',json_extract(j.value,'$.id'),'title',json_extract(j.value,'$.title'))
             FROM plan_sections s, json_tree(s.row_json) j WHERE s.plan_id = ?1
               AND j.type = 'object' AND (j.fullkey = '$' OR j.path LIKE '%.subsections')
               AND json_type(j.value,'$.id') = 'text' ORDER BY s.position, j.id"
        ).map_err(db_error)?;
        for document in sections
            .query_map([&id], |row| column::<String>(row, 0))
            .map_err(db_error)?
        {
            plan.sections
                .push(decode::<Section>(&id, &document.map_err(db_error)?)?);
        }
        plans.push(plan);
    }
    Ok(plans)
}

pub(super) fn selected_task(
    connection: &Connection,
    plan_id: &str,
    task_id: &str,
    bounded: bool,
) -> Result<Task, PlanError> {
    record_query();
    let history_limit = if bounded {
        TASK_HISTORY_LIMIT as i64
    } else {
        i64::MAX
    };
    let attempt_limit = if bounded {
        TASK_ATTEMPT_LIMIT as i64
    } else {
        i64::MAX
    };
    let document: String = connection.query_row(
        "SELECT json_set(row_json,
            '$.review_evidence',json((SELECT json_group_array(json(value)) FROM (
                SELECT value FROM (SELECT key,value FROM json_each(t.row_json,'$.review_evidence') ORDER BY key DESC LIMIT ?3) ORDER BY key))),
            '$.review_audit',json((SELECT json_group_array(json(value)) FROM (
                SELECT value FROM (SELECT key,value FROM json_each(t.row_json,'$.review_audit') ORDER BY key DESC LIMIT ?3) ORDER BY key))))
         FROM plan_tasks t WHERE plan_id = ?1 AND task_id = ?2",
        params![plan_id,task_id,history_limit], |row| column(row,0),
    ).optional().map_err(db_error)?.ok_or(PlanError::TaskNotFound)?;
    let mut task: Task = decode(plan_id, &document)?;
    record_query();
    let mut milestones = connection
        .prepare_cached(
            "SELECT row_json FROM (SELECT seq,row_json FROM plan_task_milestones
         WHERE plan_id=?1 AND task_id=?2 ORDER BY seq DESC LIMIT ?3) ORDER BY seq",
        )
        .map_err(db_error)?;
    for document in milestones
        .query_map(params![plan_id, task_id, history_limit], |row| {
            column::<String>(row, 0)
        })
        .map_err(db_error)?
    {
        task.milestones
            .push(decode(plan_id, &document.map_err(db_error)?)?);
    }
    record_query();
    let mut attempts = connection
        .prepare_cached(
            "SELECT row_json FROM (SELECT position,row_json FROM plan_task_attempts
         WHERE plan_id=?1 AND task_id=?2 ORDER BY position DESC LIMIT ?3) ORDER BY position",
        )
        .map_err(db_error)?;
    for document in attempts
        .query_map(params![plan_id, task_id, attempt_limit], |row| {
            column::<String>(row, 0)
        })
        .map_err(db_error)?
    {
        task.execution_attempts
            .push(decode(plan_id, &document.map_err(db_error)?)?);
    }
    Ok(task)
}

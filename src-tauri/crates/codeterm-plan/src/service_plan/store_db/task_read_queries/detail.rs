use super::super::read_projections::{column, decode, record_query};
use super::super::{db_error, PlanDatabase};
use crate::plan::{OrchestratorRef, TaskStatus};
use crate::service_plan::state::{project_task_facts, DependencyWait};
use crate::service_plan::task_queries::{TaskDetail, TaskLabel, TaskSection};
use crate::service_plan::PlanError;
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;

#[derive(Deserialize)]
struct DisplayDependencyRead {
    task_id: String,
    name: String,
    status: Option<TaskStatus>,
    missing: bool,
}

#[derive(Deserialize)]
struct DisplayTaskLabelRead {
    id: String,
    key: Option<String>,
    name: String,
    status: TaskStatus,
}

impl DisplayTaskLabelRead {
    fn into_label(self) -> TaskLabel {
        let display_key = self
            .key
            .unwrap_or_else(|| crate::plan::task_key::fallback_key(&self.id));
        TaskLabel {
            id: self.id,
            display_key,
            name: self.name,
            status: self.status,
        }
    }
}

#[derive(Deserialize)]
struct TaskDisplayContextRead {
    task_id: String,
    plan_title: String,
    plan_owner: OrchestratorRef,
    parent: Option<DisplayTaskLabelRead>,
    siblings: Vec<DisplayTaskLabelRead>,
    subtasks: Vec<DisplayTaskLabelRead>,
    dependency_waits: Vec<DisplayDependencyRead>,
    has_open_subtasks: bool,
    parked_for_later_group: bool,
}

const LABEL: &str = "json_object('id',r.task_id,'key',r.key,'name',r.name,'status',r.status)";

impl PlanDatabase {
    pub(in crate::service_plan) fn task_detail_projection(
        &self,
        reference: &str,
    ) -> Result<Option<TaskDetail>, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        record_query();
        let raw:Option<(String,String)>=tx.query_row(&format!("{}, target AS MATERIALIZED (
            SELECT selected_task.*,json_extract(row_json,'$.parent_task_id') AS parent_id,
                json_extract(row_json,'$.group') AS group_key,json_extract(row_json,'$.status') AS status
            FROM selected_task
         ), task_metadata AS MATERIALIZED (
            SELECT task_id,position,status,json_extract(row_json,'$.key') AS key,
                json_extract(row_json,'$.name') AS name,json_extract(row_json,'$.parent_task_id') AS parent_id,
                json_extract(row_json,'$.group') AS group_key FROM plan_tasks
            WHERE plan_id=(SELECT plan_id FROM target)
         ), declared AS MATERIALIZED (
            SELECT CAST(g.key AS INTEGER) AS position,json_extract(g.value,'$.key') AS group_key
            FROM plans p,json_each(p.header_json,'$.task_groups') g WHERE p.id=(SELECT plan_id FROM target)
         ) SELECT p.id,json_object(
            'plan_title',p.title,'plan_owner',json_extract(p.header_json,'$.owner'),'task_id',t.task_id,
            'parent',json((SELECT {LABEL} FROM task_metadata r WHERE r.task_id=t.parent_id ORDER BY r.position LIMIT 1)),
            'siblings',json((SELECT json_group_array(json(label)) FROM (SELECT {LABEL} AS label FROM task_metadata r
                WHERE r.task_id<>t.task_id AND r.parent_id IS t.parent_id ORDER BY r.position))),
            'subtasks',json((SELECT json_group_array(json(label)) FROM (SELECT {LABEL} AS label FROM task_metadata r
                WHERE r.parent_id=t.task_id ORDER BY r.position))),
            'dependency_waits',json((SELECT json_group_array(json(wait)) FROM (
                SELECT json_object('task_id',d.value,'name',CASE WHEN r.task_id IS NULL THEN 'missing dependency '||d.value ELSE r.name END,
                    'status',r.status,'missing',json(CASE WHEN r.task_id IS NULL THEN 'true' ELSE 'false' END)) AS wait
                FROM json_each(t.row_json,'$.depends_on') d LEFT JOIN task_metadata r ON r.task_id=d.value
                WHERE r.task_id IS NULL OR r.status NOT IN ('done','cancelled') ORDER BY d.key))),
            'has_open_subtasks',json(CASE WHEN EXISTS(SELECT 1 FROM task_metadata r WHERE r.parent_id=t.task_id AND r.status NOT IN ('done','cancelled')) THEN 'true' ELSE 'false' END),
            'parked_for_later_group',json(CASE WHEN t.status='pending' AND
                (SELECT min(position) FROM declared WHERE group_key=t.group_key) >
                (SELECT min(g.position) FROM declared g WHERE EXISTS(SELECT 1 FROM task_metadata r
                    WHERE r.group_key=g.group_key AND r.status NOT IN ('done','cancelled')))
                THEN 'true' ELSE 'false' END))
            FROM target t JOIN plans p ON p.id=t.plan_id",super::super::task_authority_queries::SELECTED_ACTIVE_TASK),
            [reference],|row|Ok((column(row,0)?,column(row,1)?))).optional().map_err(db_error)?;
        let Some((plan_id, raw)) = raw else {
            tx.commit().map_err(db_error)?;
            return Ok(None);
        };
        let context: TaskDisplayContextRead = decode(&plan_id, &raw)?;
        let task = super::selected_task(&tx, &plan_id, &context.task_id)?;
        record_query();
        let mut statement=tx.prepare("WITH RECURSIVE sections(path,node) AS (
            SELECT printf('%020d',position),row_json FROM plan_sections WHERE plan_id=?1
            UNION ALL SELECT sections.path||'.'||printf('%020d',child.key),child.value FROM sections,json_each(sections.node,'$.subsections') child)
            SELECT json_extract(node,'$.id'),json_extract(node,'$.title') FROM sections
            WHERE json_type(node)='object' AND json_extract(node,'$.id') IN
                (SELECT value FROM plan_tasks t,json_each(t.row_json,'$.section_ids') WHERE t.plan_id=?1 AND t.task_id=?2)
            ORDER BY path").map_err(db_error)?;
        let rows = statement
            .query_map(params![plan_id, context.task_id], |row| {
                Ok(TaskSection {
                    id: column(row, 0)?,
                    title: column(row, 1)?,
                })
            })
            .map_err(db_error)?;
        let sections = rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?;
        drop(statement);
        let waits = context
            .dependency_waits
            .into_iter()
            .map(|wait| DependencyWait {
                task_id: wait.task_id,
                name: wait.name,
                status: wait.status,
                missing: wait.missing,
            })
            .collect();
        let projection = project_task_facts(
            &task.status,
            waits,
            &task.review_stage,
            &task.qa_state,
            task.interruption.handoff_required,
            context.has_open_subtasks,
            context.parked_for_later_group,
        );
        let result = TaskDetail {
            plan_id,
            plan_title: context.plan_title,
            plan_owner: context.plan_owner,
            display_key: task.display_key(),
            sections,
            parent: context.parent.map(DisplayTaskLabelRead::into_label),
            siblings: context
                .siblings
                .into_iter()
                .map(DisplayTaskLabelRead::into_label)
                .collect(),
            subtasks: context
                .subtasks
                .into_iter()
                .map(DisplayTaskLabelRead::into_label)
                .collect(),
            presentation: projection.presentation,
            dependency_waits: projection.dependency_waits,
            task,
        };
        tx.commit().map_err(db_error)?;
        Ok(Some(result))
    }
}

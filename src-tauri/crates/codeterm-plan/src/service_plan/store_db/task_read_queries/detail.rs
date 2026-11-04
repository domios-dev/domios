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

const LABEL: &str = "json_object('id',r.task_id,'key',json_extract(r.row_json,'$.key'),
    'name',json_extract(r.row_json,'$.name'),'status',r.status)";

impl PlanDatabase {
    pub(in crate::service_plan) fn task_detail_projection(
        &self,
        reference: &str,
    ) -> Result<Option<TaskDetail>, PlanError> {
        let mut connection = self.connection.lock();
        let tx = connection.transaction().map_err(db_error)?;
        record_query();
        let raw:Option<(String,String)>=tx.query_row(&format!("{} SELECT p.id,json_object(
            'plan_title',p.title,'plan_owner',json_extract(p.header_json,'$.owner'),'task_id',t.task_id,
            'parent',json((SELECT {LABEL} FROM plan_tasks r WHERE r.plan_id=t.plan_id AND r.task_id=json_extract(t.row_json,'$.parent_task_id') ORDER BY r.position LIMIT 1)),
            'siblings',json((SELECT json_group_array(json(label)) FROM (SELECT {LABEL} AS label FROM plan_tasks r
                WHERE r.plan_id=t.plan_id AND r.task_id<>t.task_id AND json_extract(r.row_json,'$.parent_task_id') IS json_extract(t.row_json,'$.parent_task_id') ORDER BY r.position))),
            'subtasks',json((SELECT json_group_array(json(label)) FROM (SELECT {LABEL} AS label FROM plan_tasks r
                WHERE r.plan_id=t.plan_id AND json_extract(r.row_json,'$.parent_task_id')=t.task_id ORDER BY r.position))),
            'dependency_waits',json((SELECT json_group_array(json(wait)) FROM (
                SELECT json_object('task_id',d.value,'name',CASE WHEN r.task_id IS NULL THEN 'missing dependency '||d.value ELSE json_extract(r.row_json,'$.name') END,
                    'status',r.status,'missing',json(CASE WHEN r.task_id IS NULL THEN 'true' ELSE 'false' END)) AS wait
                FROM json_each(t.row_json,'$.depends_on') d LEFT JOIN plan_tasks r ON r.plan_id=t.plan_id AND r.task_id=d.value
                WHERE r.task_id IS NULL OR r.status NOT IN ('done','cancelled') ORDER BY d.key))),
            'has_open_subtasks',json(CASE WHEN EXISTS(SELECT 1 FROM plan_tasks r WHERE r.plan_id=t.plan_id AND json_extract(r.row_json,'$.parent_task_id')=t.task_id AND r.status NOT IN ('done','cancelled')) THEN 'true' ELSE 'false' END),
            'parked_for_later_group',json(CASE WHEN json_extract(t.row_json,'$.status')='pending' AND
                (SELECT min(CAST(g.key AS INTEGER)) FROM json_each(p.header_json,'$.task_groups') g WHERE json_extract(g.value,'$.key')=json_extract(t.row_json,'$.group')) >
                (SELECT min(CAST(g.key AS INTEGER)) FROM json_each(p.header_json,'$.task_groups') g WHERE EXISTS(SELECT 1 FROM plan_tasks r WHERE r.plan_id=t.plan_id AND json_extract(r.row_json,'$.group')=json_extract(g.value,'$.key') AND r.status NOT IN ('done','cancelled')))
                THEN 'true' ELSE 'false' END))
            FROM selected_task t JOIN plans p ON p.id=t.plan_id",super::super::task_authority_queries::SELECTED_ACTIVE_TASK),
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

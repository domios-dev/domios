use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{OrchestratorRef, PlanStatus};
use crate::service_plan::task_queries::{
    TaskListEmptyReason, TaskListFilter, TaskListResult, TaskListTask, TaskSummary,
};
use crate::service_plan::PlanError;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;

const LIST_TASK: &str = "json_object(
    'id',t.task_id,'name',json_extract(t.row_json,'$.name'),'type',json_extract(t.row_json,'$.type'),
    'status',t.status,'assigned_tab_id',t.assigned_tab_id,
    'key',json_extract(t.row_json,'$.key'),'handle',json_extract(t.row_json,'$.handle'),
    'parent_task_id',json_extract(t.row_json,'$.parent_task_id'),
    'pause_reason',json_extract(t.row_json,'$.pause_reason'),
    'status_label',json_extract(t.row_json,'$.status_label'),
    'section_ids',json(CASE WHEN json_type(t.row_json,'$.section_ids') IS NULL THEN '[]' ELSE json_extract(t.row_json,'$.section_ids') END),
    'external_refs',json(CASE WHEN json_type(t.row_json,'$.external_refs') IS NULL THEN '[]' ELSE json_extract(t.row_json,'$.external_refs') END),
    'lane',json(CASE WHEN json_type(t.row_json,'$.lane')='object' THEN json_object(
        'tab_id',json_extract(t.row_json,'$.lane.tab_id'),
        'workspace_id',json_extract(t.row_json,'$.lane.workspace_id'),
        'branch',json_extract(t.row_json,'$.lane.branch'),
        'cleanup_status',json_extract(t.row_json,'$.lane.cleanup_status'))
        WHEN json_type(t.row_json,'$.lane') IS NULL THEN '{}' ELSE json_extract(t.row_json,'$.lane') END))";

#[derive(Deserialize)]
struct ListRow {
    plan_id: String,
    plan_title: String,
    plan_owner: OrchestratorRef,
    task: TaskListTask,
    section_titles: Vec<String>,
}

fn scope(filter: &TaskListFilter) -> String {
    format!(
        "p.status='active' AND {} AND {}",
        if filter.project_path.is_some() {
            "p.project_path=?1"
        } else {
            "?1 IS NULL"
        },
        if filter.plan_id.is_some() {
            "p.id=?2"
        } else {
            "?2 IS NULL"
        }
    )
}

fn selection(filter: &TaskListFilter) -> String {
    format!(
        "{} AND {} AND {} AND {}",
        if filter.active_only {
            "(?3=1 AND t.status IN ('pending','in_progress','in_review','blocked'))"
        } else {
            "?3=0"
        },
        if filter.unassigned {
            "(?4=1 AND t.assigned_tab_id IS NULL)"
        } else {
            "?4=0"
        },
        if filter.status.is_some() {
            "t.status=?5"
        } else {
            "?5 IS NULL"
        },
        if filter.mine_tabs.is_empty() {
            "?6='[]'"
        } else {
            "t.assigned_tab_id IS NOT NULL AND t.assigned_tab_id IN (SELECT value FROM json_each(?6))"
        }
    )
}

fn mine_json(filter: &TaskListFilter) -> Result<String, PlanError> {
    let mut tabs: Vec<_> = filter.mine_tabs.iter().collect();
    tabs.sort();
    serde_json::to_string(&tabs).map_err(|error| PlanError::Io(error.to_string()))
}

impl PlanDatabase {
    pub(in crate::service_plan) fn task_list_snapshot(
        &self,
        filter: &TaskListFilter,
    ) -> Result<TaskListResult, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        let task_source = if filter.mine_tabs.is_empty() {
            "plan_tasks t"
        } else {
            "plan_tasks t INDEXED BY plan_tasks_assigned_tab_id"
        };
        let sql = format!(
            "WITH selected AS MATERIALIZED (
            SELECT p.id AS plan_id,p.title,p.updated_at,t.position,{LIST_TASK} AS task
            FROM {task_source} JOIN plans p ON p.id=t.plan_id
            WHERE {} AND {}),
        headers AS MATERIALIZED (
            SELECT p.id,p.header_json->'$.owner' AS owner FROM plans p
            WHERE p.id IN (SELECT plan_id FROM selected)),
        memberships AS MATERIALIZED (
            SELECT DISTINCT q.plan_id,json_extract(q.task,'$.id') AS task_id,r.value AS section_id
            FROM selected q,json_each(q.task,'$.section_ids') r),
        labels AS MATERIALIZED (
            SELECT s.plan_id,s.position,j.id AS ordinal,json_extract(j.value,'$.id') AS id,
                   json_extract(j.value,'$.title') AS title
            FROM plan_sections s,json_tree(s.row_json) j
            WHERE s.plan_id IN (SELECT plan_id FROM selected)
              AND j.type='object' AND (j.fullkey='$' OR j.path LIKE '%.subsections')
              AND json_type(j.value,'$.id')='text'
              AND json_extract(j.value,'$.id') IN (SELECT section_id FROM memberships WHERE plan_id=s.plan_id))
        SELECT x.plan_id,json_object('plan_id',x.plan_id,'plan_title',x.title,
            'plan_owner',json(p.owner),'task',json(x.task),
            'section_titles',json((SELECT json_group_array(title) FROM (
                SELECT l.title FROM labels l WHERE l.plan_id=x.plan_id
                  AND l.id IN (SELECT section_id FROM memberships WHERE plan_id=x.plan_id AND task_id=json_extract(x.task,'$.id'))
                ORDER BY l.position,l.ordinal))))
        FROM selected x JOIN headers p ON p.id=x.plan_id
        ORDER BY x.title COLLATE BINARY,json_extract(x.task,'$.name') COLLATE BINARY,
                 x.updated_at DESC,x.plan_id,x.position",
            scope(filter),
            selection(filter)
        );
        let mine = mine_json(filter)?;
        let status = filter.status.as_ref().map(|status| status.key());
        record_query();
        let mut statement = transaction.prepare_cached(&sql).map_err(db_error)?;
        let documents = statement
            .query_map(
                rusqlite::params![
                    filter.project_path,
                    filter.plan_id,
                    filter.active_only,
                    filter.unassigned,
                    status,
                    mine
                ],
                |row| Ok((column::<String>(row, 0)?, column::<String>(row, 1)?)),
            )
            .map_err(db_error)?;
        let mut tasks = Vec::new();
        for document in documents {
            let (id, encoded) = document.map_err(db_error)?;
            let row: ListRow = decode(&id, &encoded)?;
            let display_key = row
                .task
                .key
                .clone()
                .unwrap_or_else(|| crate::plan::task_key::fallback_key(&row.task.id));
            tasks.push(TaskSummary {
                plan_id: row.plan_id,
                plan_title: row.plan_title,
                plan_owner: row.plan_owner,
                display_key,
                task: row.task,
                section_titles: row.section_titles,
            });
        }
        drop(statement);
        let empty_reason = if tasks.is_empty() {
            Some(empty_reason(&transaction, filter)?)
        } else {
            None
        };
        transaction.commit().map_err(db_error)?;
        Ok(TaskListResult {
            tasks,
            empty_reason,
        })
    }
}

fn empty_reason(
    connection: &Connection,
    filter: &TaskListFilter,
) -> Result<TaskListEmptyReason, PlanError> {
    if let Some(id) = filter.plan_id.as_deref() {
        record_query();
        let header = connection.query_row("SELECT json_object('status',status,'project_path',project_path) FROM plans WHERE id=?1",[id],|row| column::<String>(row,0)).optional().map_err(db_error)?;
        let Some(header) = header else {
            return Ok(TaskListEmptyReason::PlanNotFound { plan_id: id.into() });
        };
        #[derive(Deserialize)]
        struct Header {
            status: PlanStatus,
            project_path: String,
        }
        let header: Header = decode(id, &header)?;
        if header.status == PlanStatus::Archived {
            return Ok(TaskListEmptyReason::PlanArchived { plan_id: id.into() });
        }
        if let Some(project) = filter
            .project_path
            .as_ref()
            .filter(|project| **project != header.project_path)
        {
            return Ok(TaskListEmptyReason::PlanOutsideProject {
                plan_id: id.into(),
                plan_project: header.project_path,
                requested_project: project.clone(),
            });
        }
    }
    record_query();
    let (plans,tasks): (usize,usize) = connection.query_row(&format!("SELECT count(*),coalesce(sum((SELECT count(*) FROM plan_tasks t WHERE t.plan_id=p.id)),0) FROM plans p WHERE {}",scope(filter)),rusqlite::params![filter.project_path,filter.plan_id],|row| Ok((row.get(0)?,row.get(1)?))).map_err(db_error)?;
    if plans == 0 {
        return Ok(TaskListEmptyReason::NoPlansInScope {
            requested_project: filter.project_path.clone(),
        });
    }
    if tasks == 0 {
        return Ok(TaskListEmptyReason::NoTasksInScope {
            plan_id: filter.plan_id.clone(),
        });
    }
    if filter.active_only {
        let relaxed = TaskListFilter {
            active_only: false,
            ..filter.clone()
        };
        record_query();
        let hidden = connection.query_row(&format!("SELECT count(*) FROM plan_tasks t JOIN plans p ON p.id=t.plan_id WHERE {} AND {}",scope(filter),selection(&relaxed)),rusqlite::params![filter.project_path,filter.plan_id,false,filter.unassigned,filter.status.as_ref().map(|status| status.key()),mine_json(filter)?],|row| row.get::<_,usize>(0)).map_err(db_error)?;
        if hidden > 0 {
            return Ok(TaskListEmptyReason::AllTasksInactive { hidden });
        }
    }
    Ok(TaskListEmptyReason::NoMatchForFilters)
}

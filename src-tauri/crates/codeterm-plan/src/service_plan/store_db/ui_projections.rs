use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
mod views;

use crate::service_plan::ui_projections::PlanViewProjection;
use crate::service_plan::PlanError;
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
pub(super) use views::{ListCursorView, ListWindowView, TaskView};
use views::{PlanDetailsView, PlanHeaderView, PlanView, SectionView};

const HEADER: &str = "json_object(
    'id',p.id,'title',p.title,'status',p.status,'project_path',p.project_path,
    'canonical_project_path',json_extract(p.header_json,'$.canonical_project_path'),
    'created_at',json_extract(p.header_json,'$.created_at'),'updated_at',p.updated_at,
    'owner',json_extract(p.header_json,'$.owner'),'planner',json_extract(p.header_json,'$.planner'),
    'mode',json_extract(p.header_json,'$.mode'),'planning_state',json_extract(p.header_json,'$.planning_state'),
    'team_id',p.team_id,'canvas',json_extract(p.header_json,'$.canvas'))";

pub(super) const TASK_SUMMARY: &str = "json_object(
    'id',t.task_id,'name',json_extract(t.row_json,'$.name'),'type',json_extract(t.row_json,'$.type'),
    'key',json_extract(t.row_json,'$.key'),'handle',json_extract(t.row_json,'$.handle'),
    'status',t.status,'status_label',json_extract(t.row_json,'$.status_label'),
    'review_stage',coalesce(json_extract(t.row_json,'$.review_stage'),'review'),
    'qa_state',json_extract(t.row_json,'$.qa_state'),
    'assigned_tab_id',t.assigned_tab_id,'lane',json_extract(t.row_json,'$.lane'),
    'parent_task_id',json_extract(t.row_json,'$.parent_task_id'),
    'depends_on',json(coalesce(json_extract(t.row_json,'$.depends_on'),'[]')),
    'section_ids',json(coalesce(json_extract(t.row_json,'$.section_ids'),'[]')),
    'group',json_extract(t.row_json,'$.group'),'model',json_extract(t.row_json,'$.model'),
    'peer',json_extract(t.row_json,'$.peer'),'priority',json_extract(t.row_json,'$.priority'),
    'created_at',json_extract(t.row_json,'$.created_at'),'updated_at',t.updated_at,
    'status_changed_at',json_extract(t.row_json,'$.status_changed_at'),
    'tags',json(coalesce(json_extract(t.row_json,'$.tags'),'[]')),
    'canvas',json_extract(t.row_json,'$.canvas'),
    'criteria_count',coalesce(json_array_length(t.row_json,'$.acceptance_criteria'),0),
    'milestone_count',(SELECT count(*) FROM plan_task_milestones m WHERE m.plan_id=t.plan_id AND m.task_id=t.task_id),
    'execution_attempts',json((SELECT json_group_array(json(document)) FROM (
        SELECT json_object('id',attempt_id,'executor_tab_id',executor_tab_id,
            'tab_id',json_extract(row_json,'$.tab_id'),'executor_title',json_extract(row_json,'$.executor_title'),
            'tab_title',json_extract(row_json,'$.tab_title'),'role',json_extract(row_json,'$.role'),
            'model',json_extract(row_json,'$.model'),'assigned_at',json_extract(row_json,'$.assigned_at'),
            'closed_at',json_extract(row_json,'$.closed_at')) AS document
        FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id ORDER BY position DESC LIMIT 1
    ))))";

impl PlanDatabase {
    pub(in crate::service_plan) fn ui_plan_projection(
        &self,
        id: &str,
        view: PlanViewProjection,
    ) -> Result<Value, PlanError> {
        if view == PlanViewProjection::Chat {
            return self
                .chat_projection(None, None, None, Some(id), None)?
                .pop()
                .ok_or(PlanError::NotFound);
        }
        let mut guard = self.connection.lock();
        let transaction = guard.transaction().map_err(db_error)?;
        let value = projection(&transaction, id, view)?;
        transaction.commit().map_err(db_error)?;
        serde_json::to_value(value).map_err(super::json_error)
    }

    pub(in crate::service_plan) fn ui_plan_summaries(
        &self,
        project: Option<&str>,
    ) -> Result<Vec<Value>, PlanError> {
        let mut guard = self.connection.lock();
        let transaction = guard.transaction().map_err(db_error)?;
        let ids = {
            record_query();
            let mut statement = transaction.prepare_cached(
                "SELECT id FROM plans WHERE ?1 IS NULL OR project_path=?1 ORDER BY updated_at DESC,id"
            ).map_err(db_error)?;
            let rows = statement
                .query_map([project], |row| column::<String>(row, 0))
                .map_err(db_error)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?
        };
        let values = ids
            .iter()
            .map(|id| {
                let view = projection(&transaction, id, PlanViewProjection::Summary)?;
                serde_json::to_value(view).map_err(super::json_error)
            })
            .collect::<Result<_, _>>()?;
        transaction.commit().map_err(db_error)?;
        Ok(values)
    }
}

pub(super) fn projection(
    connection: &Connection,
    id: &str,
    view: PlanViewProjection,
) -> Result<PlanView, PlanError> {
    projection_with_list(connection, id, view, None)
}

pub(super) fn projection_with_list(
    connection: &Connection,
    id: &str,
    view: PlanViewProjection,
    list: Option<&crate::service_plan::ui_projections::ListWindowQuery>,
) -> Result<PlanView, PlanError> {
    record_query();
    let header: String = connection
        .query_row(
            &format!("SELECT {HEADER} FROM plans p WHERE p.id=?1"),
            [id],
            |row| column(row, 0),
        )
        .optional()
        .map_err(db_error)?
        .ok_or(PlanError::NotFound)?;
    let header: PlanHeaderView = decode(id, &header)?;
    let details = if !matches!(
        view,
        PlanViewProjection::Summary | PlanViewProjection::TaskMetadata
    ) {
        record_query();
        let selected: String = connection.query_row(
            "SELECT json_object('goal',CASE WHEN ?2 THEN json_extract(header_json,'$.goal')
                WHEN json_type(header_json,'$.goal')='object' THEN json_object('vector',json_extract(header_json,'$.goal.vector')) END,
                'spec_links',json(CASE WHEN ?2 THEN coalesce(json_extract(header_json,'$.spec_links'),'[]') ELSE '[]' END),
                'primary_spec_id',json_extract(header_json,'$.primary_spec_id'),
                'spec_refs',json(coalesce(json_extract(header_json,'$.spec_refs'),'[]')),
                'task_groups',json(coalesce(json_extract(header_json,'$.task_groups'),'[]')),
                'tag_facets',json(coalesce(json_extract(header_json,'$.tag_facets'),'[]')),
                'milestone_kinds',json(coalesce(json_extract(header_json,'$.milestone_kinds'),'[]')))
             FROM plans WHERE id=?1", rusqlite::params![id, view == PlanViewProjection::Document], |row| column(row, 0),
        ).map_err(db_error)?;
        Some(decode::<PlanDetailsView>(id, &selected)?)
    } else {
        None
    };
    let mut sections = Vec::<SectionView>::new();
    let mut tasks = Vec::<TaskView>::new();
    if view == PlanViewProjection::Document {
        record_query();
        let mut statement = connection
            .prepare_cached("SELECT row_json FROM plan_sections WHERE plan_id=?1 ORDER BY position")
            .map_err(db_error)?;
        for row in statement
            .query_map([id], |row| column::<String>(row, 0))
            .map_err(db_error)?
        {
            sections.push(decode::<SectionView>(id, &row.map_err(db_error)?)?);
        }
    } else if view == PlanViewProjection::List {
        sections = super::read_projections::section_outline_rows(connection, id)?
            .into_iter()
            .map(|section| serde_json::from_value(section).map_err(super::json_error))
            .collect::<Result<_, _>>()?;
    }
    if view == PlanViewProjection::Board {
        record_query();
        let predicate = "AND ((t.assigned_tab_id IS NOT NULL AND t.status NOT IN ('done','cancelled')) OR t.status IN ('in_review','blocked'))";
        let mut statement = connection.prepare_cached(&format!(
            "SELECT {TASK_SUMMARY} FROM plan_tasks t WHERE t.plan_id=?1 {predicate} ORDER BY t.position"
        )).map_err(db_error)?;
        for row in statement
            .query_map([id], |row| column::<String>(row, 0))
            .map_err(db_error)?
        {
            tasks.push(decode::<TaskView>(id, &row.map_err(db_error)?)?);
        }
    }
    let task_window = if matches!(
        view,
        PlanViewProjection::List | PlanViewProjection::TaskMetadata
    ) {
        Some(super::list_window_queries::window(
            connection,
            id,
            list.unwrap_or(&Default::default()),
        )?)
    } else {
        None
    };
    let task_window = task_window.map(|(selected, window)| {
        tasks = selected;
        window
    });
    Ok(PlanView {
        header,
        details,
        sections,
        tasks,
        task_window,
        task_counts: super::board_state_queries::task_counts(connection, id)?,
        revision: super::change_feed::revision(connection, id)?.to_string(),
    })
}

#[cfg(test)]
mod tests;

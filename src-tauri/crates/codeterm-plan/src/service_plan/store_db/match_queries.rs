//! Id queries and small projections behind `PlanMatch`: SQL names the rows a
//! consumer needs, so no plan outside them is parsed.

use super::{db_error, PlanDatabase};
use crate::service_plan::plan_match::{self, PlanMatch};
use crate::service_plan::PlanError;

/// Reference fields of one task row, for resolving a task by id, key or handle.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRefRow {
    pub plan_id: String,
    pub task_id: String,
    pub key: Option<String>,
    pub handle: Option<String>,
    pub name: String,
}

/// Reference fields of one plan section at any depth.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionRefRow {
    pub plan_id: String,
    pub section_id: String,
    pub handle: Option<String>,
    pub title: String,
}

/// A text field of one task row, with the row's plan and task ids.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskFieldRow {
    pub plan_id: String,
    pub task_id: String,
    pub value: String,
}

/// A text field of one attempt row, with the row's plan, task and attempt ids.
#[derive(Debug, Clone, PartialEq)]
pub struct AttemptFieldRow {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub value: String,
}

// Mirrors `paths::plan_sidecar_root`: a blank canonical path falls back to the project path.
const ROOT: &str = "coalesce(CASE WHEN json_valid(header_json)
    AND trim(coalesce(json_extract(header_json, '$.canonical_project_path'), '')) <> ''
    THEN json_extract(header_json, '$.canonical_project_path') END, project_path)";

impl PlanDatabase {
    /// Newest first, so callers that take the first match keep a stable choice.
    pub(in crate::service_plan) fn plan_ids_matching(
        &self,
        predicate: &PlanMatch,
    ) -> Result<Vec<String>, PlanError> {
        let (clause, params) = plan_match::compile(predicate);
        let sql =
            format!("SELECT p.id FROM plans p WHERE {clause} ORDER BY p.updated_at DESC, p.id");
        let connection = self.connection.lock();
        let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(params), |row| {
                row.get::<_, String>(0)
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    /// Each plan's project root: the canonical path when set, else the project path.
    pub(in crate::service_plan) fn plan_roots(&self) -> Result<Vec<(String, String)>, PlanError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare_cached(&format!("SELECT id, {ROOT} FROM plans"))
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    pub(in crate::service_plan) fn task_field_values(
        &self,
        path: &str,
    ) -> Result<Vec<TaskFieldRow>, PlanError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare_cached(
                "SELECT plan_id, task_id, json_extract(row_json, ?1) FROM plan_tasks
                 WHERE json_type(row_json, ?1) = 'text'",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([path], |row| {
                Ok(TaskFieldRow {
                    plan_id: row.get(0)?,
                    task_id: row.get(1)?,
                    value: row.get(2)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    pub(in crate::service_plan) fn attempt_field_values(
        &self,
        path: &str,
    ) -> Result<Vec<AttemptFieldRow>, PlanError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare_cached(
                "SELECT plan_id, task_id, attempt_id, json_extract(row_json, ?1) FROM plan_task_attempts
                 WHERE json_type(row_json, ?1) = 'text'",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([path], |row| {
                Ok(AttemptFieldRow {
                    plan_id: row.get(0)?,
                    task_id: row.get(1)?,
                    attempt_id: row.get(2)?,
                    value: row.get(3)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    pub(in crate::service_plan) fn task_ref_rows(&self) -> Result<Vec<TaskRefRow>, PlanError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare_cached(
                "SELECT plan_id, task_id, json_extract(row_json, '$.key'),
                    json_extract(row_json, '$.handle'), coalesce(json_extract(row_json, '$.name'), '')
                 FROM plan_tasks ORDER BY plan_id, position",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(TaskRefRow {
                    plan_id: row.get(0)?,
                    task_id: row.get(1)?,
                    key: row.get(2)?,
                    handle: row.get(3)?,
                    name: row.get(4)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    /// Sections and their nested subsections, read from the section rows.
    pub(in crate::service_plan) fn section_ref_rows(
        &self,
    ) -> Result<Vec<SectionRefRow>, PlanError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare_cached(
                "SELECT s.plan_id, json_extract(j.value, '$.id'), json_extract(j.value, '$.handle'),
                    coalesce(json_extract(j.value, '$.title'), '')
                 FROM plan_sections s, json_tree(s.row_json) j
                 WHERE j.type = 'object' AND (j.fullkey = '$' OR j.path LIKE '%.subsections')
                   AND json_type(j.value, '$.id') = 'text'
                 ORDER BY s.plan_id, s.position, j.id",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(SectionRefRow {
                    plan_id: row.get(0)?,
                    section_id: row.get(1)?,
                    handle: row.get(2)?,
                    title: row.get(3)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    /// `(plan_id, loop_id, target)` of every loop in a plan header.
    pub(in crate::service_plan) fn loop_rows(
        &self,
    ) -> Result<Vec<(String, String, String)>, PlanError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare_cached(
                "SELECT p.id, json_extract(l.value, '$.id'), coalesce(json_extract(l.value, '$.target'), '')
                 FROM plans p, json_each(CASE WHEN json_valid(p.header_json) THEN p.header_json END, '$.loops') l
                 WHERE json_type(l.value, '$.id') = 'text'
                 ORDER BY p.id, l.key",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }
}

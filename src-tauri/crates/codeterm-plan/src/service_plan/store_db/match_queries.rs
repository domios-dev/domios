//! Purpose-specific identity and small metadata queries.

use super::{db_error, PlanDatabase};
use crate::service_plan::PlanError;

/// Reference fields of one plan section at any depth.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionRefRow {
    pub plan_id: String,
    pub section_id: String,
    pub handle: Option<String>,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttemptLanePathRead {
    pub attempt_id: String,
    pub worktree_path: String,
}

// Mirrors `paths::plan_sidecar_root`: a blank canonical path falls back to the project path.
pub(super) const ROOT: &str = "coalesce(CASE WHEN json_valid(header_json)
    AND trim(coalesce(json_extract(header_json, '$.canonical_project_path'), '')) <> ''
    THEN json_extract(header_json, '$.canonical_project_path') END, project_path)";

impl PlanDatabase {
    pub(in crate::service_plan) fn curator_bootstrap_plan_ids(
        &self,
    ) -> Result<Vec<String>, PlanError> {
        let connection = self.connection.lock();
        super::read_projections::record_query();
        let mut statement = connection
            .prepare_cached(
                "SELECT id FROM plans WHERE status='active'
             OR json_extract(header_json,'$.control_loop.running') IS NOT NULL
             OR json_extract(header_json,'$.control_loop.cron_id') IS NOT NULL
             OR json_extract(header_json,'$.control_loop.next_at') IS NOT NULL
             ORDER BY updated_at DESC,id",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| super::read_projections::column::<String>(row, 0))
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

    pub(in crate::service_plan) fn attempt_lane_paths(
        &self,
    ) -> Result<Vec<AttemptLanePathRead>, PlanError> {
        let connection = self.connection.lock();
        super::read_projections::record_query();
        let mut statement = connection
            .prepare_cached(
                "SELECT plan_id,attempt_id,row_json->'$.worktree_path' FROM plan_task_attempts
             WHERE json_type(row_json,'$.worktree_path') IS NOT NULL
                AND json_type(row_json,'$.worktree_path')<>'null'",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    super::read_projections::column::<String>(row, 0)?,
                    super::read_projections::column::<String>(row, 1)?,
                    super::read_projections::column::<String>(row, 2)?,
                ))
            })
            .map_err(db_error)?;
        rows.map(|row| {
            let (plan_id, attempt_id, raw) = row.map_err(db_error)?;
            Ok(AttemptLanePathRead {
                attempt_id,
                worktree_path: super::read_projections::decode(&plan_id, &raw)?,
            })
        })
        .collect()
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

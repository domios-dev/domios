//! SQL-only reads behind the light index and `list_index`: no plan row and no
//! plan document is parsed in Rust.

use super::read_projections::{column, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{OrchestratorRef, PlanStatus};
use crate::service_plan::{IndexEntry, PlanError};
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;

/// The only per-plan state the store keeps in memory.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::service_plan) struct PlanIndexRow {
    pub id: String,
    pub status: PlanStatus,
    pub team_id: Option<String>,
    pub project_path: String,
    pub title: String,
    pub updated_at: u64,
}

const INDEX_COLUMNS: &str = "id, status, team_id, project_path, title, updated_at";

/// Open work as `TaskStatus::counts_as_open_work`; assignments of finished tasks are not live.
const OPEN_TASK_STATUSES: &str = "('pending', 'in_progress', 'in_review', 'blocked')";
const FINISHED_TASK_STATUSES: &str = "('done', 'cancelled')";

// A damaged header must not fail the listing: the guard keeps the JSON functions off it.
const LIST_INDEX_SQL: &str = "
    SELECT p.id, p.status, p.team_id, p.title, p.updated_at, p.project_path,
        CASE WHEN json_valid(p.header_json) THEN json_extract(p.header_json, '$.owner.tab_id') END,
        CASE WHEN json_valid(p.header_json) THEN json_extract(p.header_json, '$.owner.session_id') END,
        CASE WHEN json_valid(p.header_json) THEN json_extract(p.header_json, '$.owner.title') END,
        CASE WHEN json_valid(p.header_json)
            THEN nullif(trim(coalesce(json_extract(p.header_json, '$.canonical_project_path'), '')), '') END,
        coalesce(CASE WHEN json_valid(p.header_json) THEN json_array_length(p.header_json, '$.loops') END, 0),
        coalesce(CASE WHEN json_valid(p.header_json) THEN json_type(p.header_json, '$.goal') = 'object' END, 0),
        CASE WHEN json_valid(p.header_json) THEN json_extract(p.header_json, '$.orphaned.reason') END,
        (SELECT count(*) FROM plan_tasks t WHERE t.plan_id = p.id),
        (SELECT count(*) FROM plan_tasks t WHERE t.plan_id = p.id AND t.status IN OPEN_TASKS),
        (SELECT count(*) FROM plan_sections s WHERE s.plan_id = p.id),
        (SELECT count(*) FROM plan_elements e WHERE e.plan_id = p.id)
    FROM plans p";

fn parse_status(raw: &str) -> Option<PlanStatus> {
    match raw {
        "active" => Some(PlanStatus::Active),
        "archived" => Some(PlanStatus::Archived),
        _ => None,
    }
}

fn index_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, String, PlanIndexColumns)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        PlanIndexColumns {
            team_id: row.get(2)?,
            project_path: row.get(3)?,
            title: row.get(4)?,
            updated_at: row.get::<_, i64>(5)? as u64,
        },
    ))
}

struct PlanIndexColumns {
    team_id: Option<String>,
    project_path: String,
    title: String,
    updated_at: u64,
}

fn finish_index_row(id: String, status: &str, columns: PlanIndexColumns) -> Option<PlanIndexRow> {
    let Some(status) = parse_status(status) else {
        log::error!(
            "[plan.store] plan record {id} has unknown status {status:?}; left out of the index"
        );
        return None;
    };
    Some(PlanIndexRow {
        id,
        status,
        team_id: columns.team_id,
        project_path: columns.project_path,
        title: columns.title,
        updated_at: columns.updated_at,
    })
}

impl PlanDatabase {
    pub(in crate::service_plan) fn load_index_rows(&self) -> Result<Vec<PlanIndexRow>, PlanError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare(&format!("SELECT {INDEX_COLUMNS} FROM plans"))
            .map_err(db_error)?;
        let rows = statement
            .query_map([], index_row)
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, status, columns)| finish_index_row(id, &status, columns))
            .collect())
    }

    pub(in crate::service_plan) fn load_index_row(
        &self,
        id: &str,
    ) -> Result<Option<PlanIndexRow>, PlanError> {
        let connection = self.connection.lock();
        let row = connection
            .query_row(
                &format!("SELECT {INDEX_COLUMNS} FROM plans WHERE id = ?1"),
                [id],
                index_row,
            )
            .optional()
            .map_err(db_error)?;
        Ok(row.and_then(|(id, status, columns)| finish_index_row(id, &status, columns)))
    }

    /// Active plans that are not orphaned: the stored Team binding or an orphan marker decides.
    pub(in crate::service_plan) fn live_active_plan_ids(&self) -> Result<Vec<String>, PlanError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare_cached(
                "SELECT id FROM plans WHERE status = 'active' AND NOT (
                    team_id IS NULL AND coalesce(
                        CASE WHEN json_valid(header_json) THEN json_type(header_json, '$.orphaned') = 'object' END, 0))",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    /// Whether any plan, task, milestone or section already carries this id.
    pub(in crate::service_plan) fn id_in_use(&self, id: &str) -> Result<bool, PlanError> {
        let connection = self.connection.lock();
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE id = ?1)
                    OR EXISTS(SELECT 1 FROM plan_tasks WHERE task_id = ?1)
                    OR EXISTS(SELECT 1 FROM plan_task_milestones WHERE json_extract(row_json, '$.id') = ?1)
                    OR EXISTS(SELECT 1 FROM plan_sections s, json_tree(s.row_json) j
                              WHERE j.key = 'id' AND j.value = ?1)",
                params![id],
                |row| row.get(0),
            )
            .map_err(db_error)
    }

    /// `list_index` rows: counts and assigned tabs are SQL aggregates over task, section
    /// and element rows, so no plan is loaded.
    pub(in crate::service_plan) fn list_index_entries(&self) -> Result<Vec<IndexEntry>, PlanError> {
        let connection = self.connection.lock();
        let mut assigned = assigned_tab_ids(&connection)?;
        let sql = LIST_INDEX_SQL.replace("OPEN_TASKS", OPEN_TASK_STATUSES);
        let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
        let path = self.path.clone();
        let rows = statement
            .query_map([], |row| list_index_entry(row, &path, &mut assigned))
            .map_err(db_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|(status, mut entry)| {
                let Some(status) = parse_status(&status) else {
                    log::error!(
                        "[plan.store] plan record {} has unknown status {status:?}; left out of the listing",
                        entry.plan_id
                    );
                    return None;
                };
                entry.status = status;
                Some(entry)
            })
            .collect())
    }
}

/// Tabs of unfinished assigned tasks per plan, in task order.
fn assigned_tab_ids(
    connection: &rusqlite::Connection,
) -> Result<HashMap<String, Vec<String>>, PlanError> {
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT plan_id, trim(assigned_tab_id) FROM plan_tasks
             WHERE assigned_tab_id IS NOT NULL AND status NOT IN {FINISHED_TASK_STATUSES}
               AND trim(assigned_tab_id) <> ''
             ORDER BY plan_id, position"
        ))
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut assigned: HashMap<String, Vec<String>> = HashMap::new();
    for row in rows {
        let (plan_id, tab_id) = row.map_err(db_error)?;
        assigned.entry(plan_id).or_default().push(tab_id);
    }
    Ok(assigned)
}

fn list_index_entry(
    row: &rusqlite::Row<'_>,
    path: &std::path::Path,
    assigned: &mut HashMap<String, Vec<String>>,
) -> rusqlite::Result<(String, IndexEntry)> {
    let id: String = column(row, 0)?;
    let status: String = column(row, 1)?;
    let project_path: String = column(row, 5)?;
    let canonical: Option<String> = column(row, 9)?;
    let orphaned: Option<String> = column(row, 12)?;
    Ok((
        status,
        IndexEntry {
            file_path: path.to_path_buf(),
            canonical_project_path: canonical.unwrap_or_else(|| project_path.clone()),
            owner: OrchestratorRef {
                tab_id: column(row, 6)?,
                session_id: column(row, 7)?,
                title: column(row, 8)?,
            },
            team_id: column(row, 2)?,
            title: column(row, 3)?,
            status: PlanStatus::Active,
            updated_at: column::<i64>(row, 4)? as u64,
            assigned_tab_ids: assigned.remove(&id).unwrap_or_default(),
            task_count: column::<i64>(row, 13)? as usize,
            open_task_count: column::<i64>(row, 14)? as usize,
            section_count: column::<i64>(row, 15)? as usize,
            element_count: column::<i64>(row, 16)? as usize,
            loop_count: column::<i64>(row, 10)? as usize,
            has_goal: column::<i64>(row, 11)? != 0,
            orphaned,
            plan_id: id,
        },
    ))
}

const INDEX_FILTER: &str = "(?1 IS NULL OR p.project_path=?1)
    AND (?2 IS NULL OR p.status=?2)
    AND (?3=0 OR (CASE WHEN json_valid(p.header_json) THEN p.owner_tab_id END IS NULL
        AND CASE WHEN json_valid(p.header_json) THEN json_extract(p.header_json,'$.owner.session_id') END IS NULL))
    AND (?4 IS NULL OR CASE WHEN json_valid(p.header_json) THEN p.owner_tab_id END=?4
        OR CASE WHEN json_valid(p.header_json) THEN json_extract(p.header_json,'$.owner.session_id') END=?4)";

impl PlanDatabase {
    pub(in crate::service_plan) fn plan_list_index(
        &self,
        project: Option<&str>,
        status: Option<PlanStatus>,
        unowned: bool,
        owner: Option<&str>,
    ) -> Result<Vec<IndexEntry>, PlanError> {
        let status = status_filter(status.as_ref());
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        let mut assigned = HashMap::<String, Vec<String>>::new();
        {
            record_query();
            let sql = format!(
                "SELECT t.plan_id,trim(t.assigned_tab_id) FROM plans p
                JOIN plan_tasks t ON t.plan_id=p.id WHERE {INDEX_FILTER}
                AND t.assigned_tab_id IS NOT NULL AND t.status NOT IN {FINISHED_TASK_STATUSES}
                AND trim(t.assigned_tab_id)<>'' ORDER BY t.plan_id,t.position"
            );
            let mut statement = transaction.prepare_cached(&sql).map_err(db_error)?;
            let rows = statement
                .query_map(params![project, status, unowned, owner], |row| {
                    Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
                })
                .map_err(db_error)?;
            for row in rows {
                let (id, tab) = row.map_err(db_error)?;
                assigned.entry(id).or_default().push(tab);
            }
        }
        record_query();
        let sql = format!("{LIST_INDEX_SQL} WHERE {INDEX_FILTER} ORDER BY p.rowid")
            .replace("OPEN_TASKS", OPEN_TASK_STATUSES);
        let path = self.path.clone();
        let mut result = Vec::new();
        {
            let mut statement = transaction.prepare_cached(&sql).map_err(db_error)?;
            let rows = statement
                .query_map(params![project, status, unowned, owner], |row| {
                    list_index_entry(row, &path, &mut assigned)
                })
                .map_err(db_error)?;
            for row in rows {
                let (status, mut entry) = row.map_err(db_error)?;
                entry.status = parse_status(&status).ok_or_else(|| PlanError::Quarantined {
                    plan_id: entry.plan_id.clone(),
                    reason: format!("unknown plan status {status:?}"),
                })?;
                result.push(entry);
            }
        }
        transaction.commit().map_err(db_error)?;
        Ok(result)
    }
}

pub(super) fn status_filter(status: Option<&PlanStatus>) -> Option<&'static str> {
    status.map(|status| match status {
        PlanStatus::Active => "active",
        PlanStatus::Archived => "archived",
    })
}

pub(super) fn filtered_plan_ids(
    connection: &rusqlite::Connection,
    project: Option<&str>,
    status: Option<&str>,
    unowned: bool,
    owner: Option<&str>,
) -> Result<Vec<String>, PlanError> {
    record_query();
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT p.id,p.status FROM plans p WHERE {INDEX_FILTER} ORDER BY p.rowid"
        ))
        .map_err(db_error)?;
    let rows = statement
        .query_map(params![project, status, unowned, owner], |row| {
            Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(db_error)?;
    rows.map(|row| {
        let (id, status) = row.map_err(db_error)?;
        parse_status(&status).ok_or_else(|| PlanError::Quarantined {
            plan_id: id.clone(),
            reason: format!("unknown plan status {status:?}"),
        })?;
        Ok(id)
    })
    .collect()
}

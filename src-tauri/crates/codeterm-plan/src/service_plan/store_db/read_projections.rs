mod agent_transition;
pub(in crate::service_plan) mod close_inventory;
mod curator_lifecycle;
pub(in crate::service_plan) mod hierarchy;
pub(in crate::service_plan) mod hierarchy_routing;
pub(in crate::service_plan) mod launch_restore;
pub(in crate::service_plan) mod reminder;
pub(in crate::service_plan::store_db) mod reminder_digests;
pub(in crate::service_plan) mod runtime_identity;
mod tab;
pub(in crate::service_plan) mod team_close;
pub(in crate::service_plan) mod team_mutation;
pub(in crate::service_plan) mod transfer_authority;
use super::{db_error, PlanDatabase};
use crate::plan::Section;
use crate::service_plan::PlanError;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

pub(in crate::service_plan) mod checkout;
mod compete_preflight;
pub(in crate::service_plan) mod competition_lane;
mod executor_source;
pub(in crate::service_plan) mod lane_clean;
pub(in crate::service_plan) mod lane_readers;
pub(in crate::service_plan) mod lane_settlement;
pub(in crate::service_plan) mod lane_stats;
pub(in crate::service_plan) mod lane_watchdog;
pub(in crate::service_plan) mod lifecycle;
pub(in crate::service_plan) mod provider_switch;
mod respawn_remote_source;
mod section_removal;
pub(in crate::service_plan) mod task_drafts;
pub(in crate::service_plan) mod team_readers;

pub(super) const SECTION_ROWS: &str = "SELECT s.position, j.id AS traversal,
    json_extract(j.value, '$.id') AS id,
    json_extract(j.value, '$.handle') AS handle,
    coalesce(json_extract(j.value, '$.title'), '') AS title,
    j.value AS document, j.fullkey AS path
    FROM plan_sections s, json_tree(s.row_json) j
    WHERE s.plan_id = ?1 AND j.type = 'object'
      AND (j.fullkey = '$' OR j.path LIKE '%.subsections')
      AND json_type(j.value, '$.id') = 'text'";

impl PlanDatabase {
    pub(in crate::service_plan) fn section_outline(&self, id: &str) -> Result<Value, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        record_query();
        let title: String = transaction
            .query_row("SELECT title FROM plans WHERE id = ?1", [id], |row| {
                column(row, 0)
            })
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::NotFound)?;
        let sections = section_outline_rows(&transaction, id)?;
        transaction.commit().map_err(db_error)?;
        Ok(json!({"id": id, "title": title, "sections": sections}))
    }

    pub(in crate::service_plan) fn selected_section(
        &self,
        plan_id: &str,
        selector: &str,
    ) -> Result<Section, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let document: String = connection.query_row(
            &format!("SELECT document FROM ({SECTION_ROWS}) WHERE id = ?2 OR handle = ?2 ORDER BY position, traversal LIMIT 1"),
            params![plan_id, selector], |row| column(row, 0),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        decode(plan_id, &document)
    }

    pub(in crate::service_plan) fn scoped_section_references(
        &self,
        plan_id: &str,
    ) -> Result<Vec<super::match_queries::SectionRefRow>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection
            .prepare_cached(&format!(
                "SELECT id, handle, title FROM ({SECTION_ROWS}) ORDER BY position, traversal"
            ))
            .map_err(db_error)?;
        let rows = statement
            .query_map([plan_id], |row| {
                Ok(super::match_queries::SectionRefRow {
                    plan_id: plan_id.into(),
                    section_id: column(row, 0)?,
                    handle: column(row, 1)?,
                    title: column(row, 2)?,
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    pub(in crate::service_plan) fn element_references(
        &self,
        plan_id: &str,
    ) -> Result<Vec<(String, String)>, PlanError> {
        self.reference_pairs("SELECT element_id, json_extract(row_json, '$.kind') FROM plan_elements WHERE plan_id = ?1 ORDER BY position", [plan_id])
    }

    pub(in crate::service_plan) fn milestone_references(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Result<Vec<(String, String)>, PlanError> {
        self.reference_pairs("SELECT json_extract(row_json, '$.id'), json_extract(row_json, '$.kind') FROM plan_task_milestones WHERE plan_id = ?1 AND task_id = ?2 ORDER BY seq", [plan_id, task_id])
    }

    fn reference_pairs(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
    ) -> Result<Vec<(String, String)>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        let mut statement = connection.prepare_cached(sql).map_err(db_error)?;
        let rows = statement
            .query_map(params, |row| Ok((column(row, 0)?, column(row, 1)?)))
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }
}

pub(super) fn decode<T: serde::de::DeserializeOwned>(
    plan_id: &str,
    document: &str,
) -> Result<T, PlanError> {
    let value =
        super::super::store_sync::parse(document).map_err(|reason| PlanError::Quarantined {
            plan_id: plan_id.into(),
            reason,
        })?;
    serde_json::from_value(value).map_err(|error| PlanError::Quarantined {
        plan_id: plan_id.into(),
        reason: error.to_string(),
    })
}

pub(super) fn section_outline_rows(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Vec<Value>, PlanError> {
    record_query();
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT id, title, path FROM ({SECTION_ROWS}) ORDER BY position, traversal"
        ))
        .map_err(db_error)?;
    let rows = statement
        .query_map([id], |row| {
            Ok((
                column::<String>(row, 0)?,
                column::<String>(row, 1)?,
                column::<String>(row, 2)?,
            ))
        })
        .map_err(db_error)?;
    let mut roots = Vec::<Value>::new();
    for row in rows {
        let (id, title, path) = row.map_err(db_error)?;
        let node = json!({"id": id, "title": title, "subsections": []});
        let depth = path.matches(".subsections[").count();
        append_outline(&mut roots, depth, node);
    }
    Ok(roots)
}

fn append_outline(roots: &mut Vec<Value>, depth: usize, node: Value) {
    if depth == 0 {
        roots.push(node);
    } else if let Some(children) = roots
        .last_mut()
        .and_then(|root| root["subsections"].as_array_mut())
    {
        append_outline(children, depth - 1, node);
    }
}

#[cfg(test)]
mod tests;

pub(super) fn record_query() {
    #[cfg(test)]
    METRICS.with(|metrics| metrics.borrow_mut().queries += 1);
}

pub(super) fn column<T: rusqlite::types::FromSql>(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    #[cfg(test)]
    if let rusqlite::types::ValueRef::Text(bytes) = row.get_ref(index)? {
        METRICS.with(|metrics| metrics.borrow_mut().materialized_bytes += bytes.len());
    }
    row.get(index)
}

#[cfg(test)]
#[derive(Debug, Default, PartialEq, Eq)]
pub(in crate::service_plan::store_db) struct ReadMetrics {
    pub(in crate::service_plan::store_db) queries: usize,
    pub(in crate::service_plan::store_db) materialized_bytes: usize,
}

#[cfg(test)]
thread_local! {
    static METRICS: std::cell::RefCell<ReadMetrics> = const { std::cell::RefCell::new(ReadMetrics { queries: 0, materialized_bytes: 0 }) };
}

#[cfg(test)]
pub(in crate::service_plan::store_db) fn take_metrics() -> ReadMetrics {
    METRICS.with(|metrics| std::mem::take(&mut *metrics.borrow_mut()))
}

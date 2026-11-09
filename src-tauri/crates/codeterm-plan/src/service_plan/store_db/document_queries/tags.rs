use super::{column, db_error, decode, record_query, PlanDatabase};
use crate::service_plan::{tag_model::TagReportInput, PlanError};
use rusqlite::{Connection, OptionalExtension};
use std::collections::BTreeMap;

impl PlanDatabase {
    pub(in crate::service_plan) fn tag_report_input(
        &self,
        id: &str,
    ) -> Result<TagReportInput, PlanError> {
        let mut guard = self.connection.lock();
        let connection = guard.transaction().map_err(db_error)?;
        record_query();
        let document: String=connection.query_row(
            "SELECT CASE WHEN json_type(header_json,'$.tag_facets') IS NULL THEN '[]' ELSE header_json->'$.tag_facets' END FROM plans WHERE id=?1",
            [id],|row| column(row,0),
        ).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        let facets = decode(id, &document)?;
        let section_counts=tag_counts(&connection,id,
            "SELECT CASE WHEN json_type(j.value,'$.tags') IS NULL THEN '[]'
                WHEN json_type(j.value,'$.tags')='array' THEN j.value->'$.tags' ELSE json('invalid section tags') END AS tags
             FROM plan_sections s,json_tree(s.row_json) j WHERE s.plan_id=?1 AND j.type='object'
                AND (j.fullkey='$' OR j.path LIKE '%.subsections')")?;
        let task_counts=tag_counts(&connection,id,
            "SELECT CASE WHEN json_type(row_json,'$.tags') IS NULL THEN '[]'
                WHEN json_type(row_json,'$.tags')='array' THEN row_json->'$.tags' ELSE json('invalid task tags') END AS tags
             FROM plan_tasks WHERE plan_id=?1")?;
        connection.commit().map_err(db_error)?;
        Ok(TagReportInput {
            facets,
            section_counts,
            task_counts,
        })
    }
}

fn tag_counts(
    connection: &Connection,
    id: &str,
    source: &str,
) -> Result<BTreeMap<String, usize>, PlanError> {
    record_query();
    let sql=format!("WITH source AS ({source}) SELECT CASE WHEN tag.type='text' THEN tag.value ELSE json('invalid tag value') END,count(*) FROM source,json_each(source.tags) tag GROUP BY tag.type,tag.value ORDER BY tag.value");
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let rows = statement
        .query_map([id], |row| {
            Ok((column::<String>(row, 0)?, row.get::<_, usize>(1)?))
        })
        .map_err(db_error)?;
    rows.collect::<Result<_, _>>().map_err(db_error)
}

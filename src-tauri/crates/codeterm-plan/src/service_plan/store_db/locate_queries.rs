use super::read_projections::{column, decode, record_query, SECTION_ROWS};
use super::{db_error, PlanDatabase};
use crate::service_plan::locate::LocateTarget;
use crate::service_plan::PlanError;
use rusqlite::params;

impl PlanDatabase {
    pub(in crate::service_plan) fn locate_body_projection(
        &self,
        plan: &str,
        target: LocateTarget<'_>,
    ) -> Result<String, PlanError> {
        let (selector, query) = match target {
            LocateTarget::Task(selector) => (selector, format!(
                "{} SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('body',CASE WHEN json_type(row_json,'$.body') IS NULL
                    THEN '' ELSE json_extract(row_json,'$.body') END) FROM selected_task)",
                super::task_authority_queries::SELECTED_TASK_IN_PLAN)),
            LocateTarget::PlanSection(selector) => (selector, format!(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('body',CASE WHEN json_type(document,'$.body') IS NULL
                    THEN '' ELSE json_extract(document,'$.body') END)
                FROM ({SECTION_ROWS}) WHERE id=?2 OR handle=?2 ORDER BY position,traversal LIMIT 1)")),
            LocateTarget::SpecSection(selector) => (selector, String::from(
                "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1),
                (SELECT json_object('body',CASE WHEN json_type(j.value,'$.body') IS NULL
                    THEN '' ELSE json_extract(j.value,'$.body') END)
                FROM specs s,json_tree(s.row_json,'$.sections') j
                WHERE s.plan_id=?1 AND j.type='object'
                    AND (j.path='$.sections' OR j.path LIKE '%.subsections')
                    AND (json_extract(j.value,'$.id')=?2 OR json_extract(j.value,'$.handle')=?2)
                ORDER BY j.id LIMIT 1)")),
        };
        let connection = self.connection.lock();
        record_query();
        let (exists, raw): (bool, Option<String>) = connection
            .query_row(&query, params![plan, selector], |row| {
                Ok((column(row, 0)?, column(row, 1)?))
            })
            .map_err(db_error)?;
        if !exists {
            return Err(PlanError::NotFound);
        }
        #[derive(serde::Deserialize)]
        struct Body {
            body: String,
        }
        let body: Body = decode(plan, &raw.ok_or(PlanError::NotFound)?)?;
        Ok(body.body)
    }
}

#[cfg(test)]
mod tests;

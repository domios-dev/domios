use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OpenFlags};
use std::path::Path;

pub(in crate::service_plan) struct AttachmentReferenceRead {
    connection: Connection,
}

#[derive(serde::Deserialize)]
struct ProjectPaths {
    project_path: String,
    canonical_project_path: String,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn attachment_reference_read(
        &self,
    ) -> Result<AttachmentReferenceRead, PlanError> {
        let connection = Connection::open_with_flags(
            self.path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(db_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        connection
            .execute_batch("BEGIN DEFERRED TRANSACTION;")
            .map_err(db_error)?;
        Ok(AttachmentReferenceRead { connection })
    }
}

impl AttachmentReferenceRead {
    pub(in crate::service_plan) fn project_plan_ids(
        &self,
        project: &Path,
    ) -> Result<Vec<String>, PlanError> {
        record_query();
        let mut statement = self
            .connection
            .prepare_cached(&format!(
                "SELECT id,{} FROM plans ORDER BY updated_at DESC,id",
                super::match_queries::ROOT
            ))
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?;
        let mut ids = Vec::new();
        for row in rows {
            let (id, root) = row.map_err(db_error)?;
            if Path::new(&root) != project {
                continue;
            }
            record_query();
            let document: String=self.connection.query_row(
                "SELECT json_object('project_path',project_path,'canonical_project_path',CASE WHEN json_type(header_json,'$.canonical_project_path') IS NULL THEN '' ELSE json_extract(header_json,'$.canonical_project_path') END) FROM plans WHERE id=?1",
                [&id],|row|column(row,0),
            ).map_err(db_error)?;
            let paths: ProjectPaths = decode(&id, &document)?;
            if crate::service_plan::paths::sidecar_root(
                &paths.project_path,
                &paths.canonical_project_path,
            ) == project
            {
                ids.push(id)
            }
        }
        Ok(ids)
    }

    pub(in crate::service_plan) fn known_directories(
        &self,
        id: &str,
        candidates: &[(&str, &str)],
    ) -> Result<Vec<bool>, PlanError> {
        record_query();
        let invalid: bool=self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=?1 AND json_type(row_json,'$.id') IS NOT 'text'
                UNION ALL SELECT 1 FROM plan_task_milestones WHERE plan_id=?1 AND json_type(row_json,'$.id') IS NOT 'text'
                UNION ALL SELECT 1 FROM plan_sections s,json_tree(s.row_json) j WHERE s.plan_id=?1 AND j.type='object'
                    AND (j.fullkey='$' OR j.path LIKE '%.subsections') AND json_type(j.value,'$.id') IS NOT 'text')",
            [id],|row|row.get(0),
        ).map_err(db_error)?;
        if invalid {
            return Err(PlanError::Quarantined {
                plan_id: id.into(),
                reason: "invalid attachment reference ID".into(),
            });
        }
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let input =
            serde_json::to_string(candidates).map_err(|error| PlanError::Io(error.to_string()))?;
        record_query();
        let mut statement=self.connection.prepare_cached(
            "WITH known_targets(kind,id) AS MATERIALIZED (
                SELECT 'tasks',json_extract(row_json,'$.id') FROM plan_tasks WHERE plan_id=?1
                UNION ALL SELECT 'sections',json_extract(j.value,'$.id') FROM plan_sections s,json_tree(s.row_json) j
                    WHERE s.plan_id=?1 AND j.type='object' AND (j.fullkey='$' OR j.path LIKE '%.subsections')
                UNION ALL SELECT 'milestones',json_extract(row_json,'$.id') FROM plan_task_milestones WHERE plan_id=?1
             ) SELECT CASE WHEN json_extract(c.value,'$[0]') IN ('tasks','sections','milestones')
                THEN EXISTS(SELECT 1 FROM known_targets target WHERE target.kind=json_extract(c.value,'$[0]') AND target.id=json_extract(c.value,'$[1]'))
                ELSE json('invalid attachment target') END FROM json_each(?2) c ORDER BY c.key"
        ).map_err(db_error)?;
        let rows = statement
            .query_map(params![id, input], |row| row.get(0))
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    pub(in crate::service_plan) fn finish(self) -> Result<(), PlanError> {
        self.connection.execute_batch("COMMIT;").map_err(db_error)
    }
}

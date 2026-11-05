use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::PlanMode;
use crate::service_plan::{PlanError, PlanMatch};
use rusqlite::{functions::FunctionFlags, Connection, OptionalExtension};
use serde::Deserialize;

pub(super) const LEARNING_WINDOW: &str = "WITH learning_rows AS MATERIALIZED (
    SELECT l.key,l.value,json_extract(l.value,'$.tag') AS tag,json_extract(l.value,'$.id') AS id
    FROM plan_heartbeat h,json_each(h.row_json,'$.learnings') l WHERE h.plan_id=?1
), recent_learnings AS MATERIALIZED (
    SELECT key FROM learning_rows WHERE coalesce(tag,'')<>'owner' ORDER BY key DESC LIMIT 50
)";

pub(super) fn register_functions(connection: &Connection) -> Result<(), PlanError> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_scalar_function("domios_heartbeat_priority", 2, flags, |context| {
            let mode: String = context.get(0)?;
            let mode = PlanMode::deserialize(serde::de::value::StrDeserializer::<
                serde::de::value::Error,
            >::new(&mode))
            .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            Ok(
                crate::service_plan::heartbeat::heartbeat_resolution_priority(
                    &mode,
                    context.get::<bool>(1)?,
                ),
            )
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_heartbeat_updated", 1, flags, |context| {
            let value: i64 = context.get(0)?;
            u64::try_from(value)
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            Ok(value)
        })
        .map_err(db_error)
}

impl PlanDatabase {
    pub(in crate::service_plan) fn heartbeat_show_projection(
        &self,
        id: &str,
        now: u64,
    ) -> Result<crate::service_plan::heartbeat::HeartbeatShowResult, PlanError> {
        let mut guard = self.connection.lock();
        let connection = guard.transaction().map_err(db_error)?;
        let parts =
            super::curator_notice_queries::heartbeat_display_snapshot(&connection, id, now)?;
        record_query();
        let document:Option<String>=connection.query_row(
            &format!("{LEARNING_WINDOW} SELECT json_object('learnings',json((SELECT json_group_array(json(value)) FROM (
                SELECT key,value FROM learning_rows WHERE tag='owner' OR key IN (SELECT key FROM recent_learnings)
                ORDER BY key)))) FROM plan_heartbeat h WHERE plan_id=?1"),
            [id],|row|column(row,0)).optional().map_err(db_error)?;
        let heartbeat: crate::plan::OrchestratorHeartbeat = match document {
            Some(document) => decode(id, &document)?,
            None => Default::default(),
        };
        let reinjection = parts.render_block(
            &crate::service_plan::heartbeat::view::learning_lines(&heartbeat),
            &[],
        );
        let read = crate::service_plan::heartbeat::HeartbeatShowResult {
            plan_id: id.into(),
            heartbeat,
            reinjection,
            blocked_or_gated_count: parts.blocked_or_gated_count,
        };
        connection.commit().map_err(db_error)?;
        Ok(read)
    }
    pub(in crate::service_plan) fn heartbeat_projection(
        &self,
        id: &str,
        now: u64,
        selection: Option<&[String]>,
    ) -> Result<crate::service_plan::heartbeat::view::HeartbeatRead, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        let read =
            super::curator_notice_queries::heartbeat_snapshot(&transaction, id, now, selection)?;
        transaction.commit().map_err(db_error)?;
        Ok(read)
    }

    pub(in crate::service_plan) fn resolve_heartbeat_plan(
        &self,
        plan_id: Option<&str>,
        tab_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<String, PlanError> {
        let connection = self.connection.lock();
        if let Some(id) = plan_id.filter(|id| !id.trim().is_empty()) {
            record_query();
            return connection
                .query_row("SELECT id FROM plans WHERE id=?1", [id], |row| {
                    column(row, 0)
                })
                .optional()
                .map_err(db_error)?
                .ok_or(PlanError::NotFound);
        }
        let mut owners = Vec::new();
        if let Some(tab) = tab_id.filter(|tab| !tab.trim().is_empty()) {
            owners.push(PlanMatch::owner(tab));
        }
        if let Some(session) = session_id.filter(|session| !session.trim().is_empty()) {
            owners.push(PlanMatch::OwnerSession(session.into()));
        }
        let predicate = PlanMatch::active().and(PlanMatch::Any(owners));
        let (clause, params) = crate::service_plan::plan_match::compile(&predicate);
        record_query();
        connection.query_row(&format!("WITH candidates AS MATERIALIZED (
            SELECT p.id,domios_heartbeat_updated(p.updated_at) AS updated,
                domios_heartbeat_priority(CASE WHEN json_type(p.header_json,'$.owner')<>'object'
                    OR (json_type(p.header_json,'$.owner.tab_id') IS NOT NULL AND json_type(p.header_json,'$.owner.tab_id') NOT IN ('text','null'))
                    OR (json_type(p.header_json,'$.owner.session_id') IS NOT NULL AND json_type(p.header_json,'$.owner.session_id') NOT IN ('text','null'))
                    THEN json('invalid heartbeat owner') WHEN json_type(p.header_json,'$.mode') IS NULL
                    THEN 'planning' WHEN json_type(p.header_json,'$.mode')='text'
                    THEN json_extract(p.header_json,'$.mode') ELSE json('invalid heartbeat mode') END,
                    EXISTS(SELECT 1 FROM plan_tasks t WHERE t.plan_id=p.id
                        AND t.status IN ('pending','in_progress','in_review','blocked'))) AS priority
            FROM plans p WHERE {clause})
            SELECT id FROM candidates WHERE priority>0 ORDER BY priority DESC,updated DESC,id LIMIT 1"),
            rusqlite::params_from_iter(params),|row|column(row,0))
            .optional().map_err(db_error)?.ok_or(PlanError::NotFound)
    }
}

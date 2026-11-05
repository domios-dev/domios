use super::{column, db_error, decode, record_query, PlanDatabase};
use crate::service_plan::{
    milestone_gates::{RegressionGroupInput, RegressionInput},
    PlanError,
};
use rusqlite::OptionalExtension;

impl PlanDatabase {
    pub(in crate::service_plan) fn retrospective_presence_projection(
        &self,
        id: &str,
    ) -> Result<Option<bool>, PlanError> {
        let connection = self.connection.lock();
        record_query();
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM plan_strides s WHERE s.plan_id=p.id
                AND CASE WHEN json_extract(s.row_json,'$.kind')='retrospective'
                    THEN CASE WHEN json_type(s.row_json,'$.text')='text'
                        THEN trim(json_extract(s.row_json,'$.text'),?2)<>?3
                        ELSE json('invalid retrospective text') END ELSE 0 END)
             FROM plans p WHERE p.id=?1",
                rusqlite::params![
                    id,
                    super::super::status_queries::WHITESPACE,
                    crate::service_plan::retrospective::retrospective_supersede_note()
                ],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)
    }

    pub(in crate::service_plan) fn regression_gate_input(
        &self,
        id: &str,
    ) -> Result<Option<RegressionInput>, PlanError> {
        let mut guard = self.connection.lock();
        let transaction = guard.transaction().map_err(db_error)?;
        let result = regression_gate_input(&transaction, id)?;
        transaction.commit().map_err(db_error)?;
        Ok(result)
    }
}

pub(in crate::service_plan::store_db) fn regression_gate_input(
    connection: &rusqlite::Connection,
    id: &str,
) -> Result<Option<RegressionInput>, PlanError> {
    record_query();
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM plans WHERE id=?1)",
            [id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    if !exists {
        return Ok(None);
    }
    let completed = unsigned_timestamp("t.row_json", "status_changed_at", true);
    let stride_at = unsigned_timestamp("s.row_json", "ts", false);
    let override_at = unsigned_timestamp("o.value", "ts", false);
    let links = string_array("s.row_json", "links", true);
    let gates = string_array("o.value", "gates", false);
    let completed_groups = completed_group_facts();
    let sql = format!(
        r#"
            {completed_groups}, declared AS MATERIALIZED (
                SELECT CAST(d.key AS INTEGER) AS position,json_extract(d.value,'$.key') AS group_key,
                    json_extract(d.value,'$.label') AS label
                FROM plans p,json_each(p.header_json,'$.task_groups') d WHERE p.id=?1
            ), stride_rows AS MATERIALIZED (
                SELECT json_extract(s.row_json,'$.kind') AS kind,{links} AS links,
                    json_object('ts',json(s.row_json->'$.ts')) AS row_json
                FROM plan_strides s WHERE s.plan_id=?1
                    AND json_extract(s.row_json,'$.kind') IN ('regression','group_close')
                    AND EXISTS(SELECT 1 FROM completed)
            ), evidence AS MATERIALIZED (
                SELECT g.group_key,s.kind,max(printf('%20s',{stride_at})) AS at
                FROM stride_rows s,json_each(s.links) l JOIN completed g
                    ON l.type='text' AND l.value='group:'||g.group_key GROUP BY g.group_key,s.kind
            ), override_rows AS MATERIALIZED (
                SELECT {gates} AS gates,json_object('ts',json(o.value->'$.ts')) AS value
                FROM plans p,json_each(p.header_json,'$.regression_gate_overrides') o
                WHERE p.id=?1 AND EXISTS(SELECT 1 FROM completed)
            ), waivers AS MATERIALIZED (
                SELECT g.group_key,max(printf('%20s',{override_at})) AS at
                FROM override_rows o,json_each(o.gates) k JOIN completed g
                    ON k.type='text' AND k.value=g.group_key GROUP BY g.group_key
            ) SELECT json_object('key',g.group_key,'label',
                CASE WHEN EXISTS(SELECT 1 FROM declared d WHERE d.group_key=g.group_key)
                THEN (SELECT d.label FROM declared d WHERE d.group_key=g.group_key ORDER BY d.position LIMIT 1)
                ELSE g.group_key END,
                'completed_at',json(trim((SELECT max(printf('%20s',{completed})) FROM members t WHERE t.group_key=g.group_key))),
                'regression_at',json(trim((SELECT at FROM evidence WHERE group_key=g.group_key AND kind='regression'))),
                'retrospective_at',json(trim((SELECT at FROM evidence WHERE group_key=g.group_key AND kind='group_close'))),
                'override_at',json(trim((SELECT at FROM waivers WHERE group_key=g.group_key))))
            FROM completed g ORDER BY g.position
        "#
    );
    record_query();
    let groups = {
        let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
        let rows = statement
            .query_map([id], |row| column::<String>(row, 0))
            .map_err(db_error)?;
        rows.map(|row| decode::<RegressionGroupInput>(id, &row.map_err(db_error)?))
            .collect::<Result<Vec<_>, PlanError>>()?
    };
    Ok(Some(RegressionInput { groups }))
}

// Padded decimal tokens preserve unsigned ordering above SQLite's signed integer range.
pub(in crate::service_plan::store_db) fn unsigned_timestamp(
    document: &str,
    field: &str,
    default_zero: bool,
) -> String {
    let token = format!("{document}->'$.{field}'");
    let absent = if default_zero {
        "'0'"
    } else {
        "json('missing unsigned timestamp')"
    };
    format!(
        "CASE WHEN json_type({document},'$.{field}') IS NULL THEN {absent}
        WHEN json_type({document},'$.{field}')='integer' AND {token} NOT GLOB '*[^0-9]*'
            AND length({token}) BETWEEN 1 AND 20
            AND (length({token})<20 OR {token}<='18446744073709551615')
        THEN {token} ELSE json('invalid unsigned timestamp') END"
    )
}

fn string_array(document: &str, field: &str, default_empty: bool) -> String {
    let absent = if default_empty {
        "'[]'"
    } else {
        "json('missing string array')"
    };
    format!(
        "CASE WHEN json_type({document},'$.{field}') IS NULL THEN {absent}
        WHEN json_type({document},'$.{field}')='array' THEN {document}->'$.{field}'
        ELSE json('invalid string array') END"
    )
}

pub(in crate::service_plan::store_db) fn completed_group_facts() -> &'static str {
    "WITH members AS MATERIALIZED (
                SELECT position,status,json_object('status_changed_at',json(CASE
                    WHEN json_type(row_json,'$.status_changed_at') IS NULL THEN '0'
                    ELSE row_json->'$.status_changed_at' END)) AS row_json,CASE WHEN json_type(row_json,'$.group')='text'
                    THEN json_extract(row_json,'$.group') ELSE json('invalid group key') END AS group_key
                FROM plan_tasks WHERE plan_id=?1 AND json_type(row_json,'$.group') IS NOT NULL
                    AND json_type(row_json,'$.group')<>'null'
            ), completed AS MATERIALIZED (
                SELECT group_key,min(position) AS position FROM members GROUP BY group_key
                HAVING sum(CASE WHEN status IN ('done','cancelled') THEN 0
                    WHEN status IN ('pending','in_progress','blocked','in_review','paused') THEN 1
                    ELSE json('invalid task status') END)=0 AND sum(status='done')>0
            )"
}

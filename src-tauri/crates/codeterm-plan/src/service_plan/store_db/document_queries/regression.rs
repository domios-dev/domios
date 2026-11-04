use super::{column, db_error, decode, record_query, PlanDatabase};
use crate::service_plan::{
    milestone_gates::{RegressionGroupInput, RegressionInput},
    PlanError,
};

impl PlanDatabase {
    pub(in crate::service_plan) fn regression_gate_input(
        &self,
        id: &str,
    ) -> Result<Option<RegressionInput>, PlanError> {
        let mut guard = self.connection.lock();
        let transaction = guard.transaction().map_err(db_error)?;
        record_query();
        let exists: bool = transaction
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
        let sql = format!(
            r#"
            WITH members AS (
                SELECT position,status,row_json,CASE WHEN json_type(row_json,'$.group')='text'
                    THEN json_extract(row_json,'$.group') ELSE json('invalid group key') END AS group_key
                FROM plan_tasks WHERE plan_id=?1 AND json_type(row_json,'$.group') IS NOT NULL
                    AND json_type(row_json,'$.group')<>'null'
            ), completed AS (
                SELECT group_key,min(position) AS position FROM members GROUP BY group_key
                HAVING sum(CASE WHEN status IN ('done','cancelled') THEN 0
                    WHEN status IN ('pending','in_progress','blocked','in_review','paused') THEN 1
                    ELSE json('invalid task status') END)=0 AND sum(status='done')>0
            )
            SELECT json_object('key',g.group_key,'label',
                CASE WHEN EXISTS(SELECT 1 FROM plans p,json_each(p.header_json,'$.task_groups') d
                    WHERE p.id=?1 AND json_extract(d.value,'$.key')=g.group_key)
                THEN (SELECT json_extract(d.value,'$.label') FROM plans p,json_each(p.header_json,'$.task_groups') d
                    WHERE p.id=?1 AND json_extract(d.value,'$.key')=g.group_key ORDER BY d.key LIMIT 1)
                ELSE g.group_key END,
                'completed_at',json(trim((SELECT max(printf('%20s',{completed})) FROM members t WHERE t.group_key=g.group_key))),
                'regression_at',json(trim((SELECT max(printf('%20s',{stride_at})) FROM plan_strides s
                    WHERE s.plan_id=?1 AND json_extract(s.row_json,'$.kind')='regression'
                    AND EXISTS(SELECT 1 FROM json_each({links}) l WHERE l.type='text' AND l.value='group:'||g.group_key)))),
                'retrospective_at',json(trim((SELECT max(printf('%20s',{stride_at})) FROM plan_strides s
                    WHERE s.plan_id=?1 AND json_extract(s.row_json,'$.kind')='group_close'
                    AND EXISTS(SELECT 1 FROM json_each({links}) l WHERE l.type='text' AND l.value='group:'||g.group_key)))),
                'override_at',json(trim((SELECT max(printf('%20s',{override_at})) FROM plans p,json_each(p.header_json,'$.regression_gate_overrides') o
                    WHERE p.id=?1 AND EXISTS(SELECT 1 FROM json_each({gates}) k WHERE k.type='text' AND k.value=g.group_key)))))
            FROM completed g ORDER BY g.position
        "#
        );
        record_query();
        let groups = {
            let mut statement = transaction.prepare_cached(&sql).map_err(db_error)?;
            let rows = statement
                .query_map([id], |row| column::<String>(row, 0))
                .map_err(db_error)?;
            rows.map(|row| decode::<RegressionGroupInput>(id, &row.map_err(db_error)?))
                .collect::<Result<Vec<_>, PlanError>>()?
        };
        transaction.commit().map_err(db_error)?;
        Ok(Some(RegressionInput { groups }))
    }
}

// Padded decimal tokens preserve unsigned ordering above SQLite's signed integer range.
pub(super) fn unsigned_timestamp(document: &str, field: &str, default_zero: bool) -> String {
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

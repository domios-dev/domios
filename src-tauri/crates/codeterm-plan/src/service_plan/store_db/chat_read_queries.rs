use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::service_plan::chat_snapshot::read::{ChatPlanRead, ChatTaskRead};
use crate::service_plan::PlanError;
use rusqlite::{params, Connection};
use serde_json::Value;

const CHAT_FIELDS: &str = "p.id, json_object(
    'id',p.id,'title',p.title,'status',p.status,'mode',json_extract(p.header_json,'$.mode'),
    'project_path',p.project_path,'owner',json_extract(p.header_json,'$.owner'),
    'planner',json_extract(p.header_json,'$.planner'),'team_id',p.team_id,
    'created_at',json_extract(p.header_json,'$.created_at'),'updated_at',p.updated_at,
    'canvas',json(coalesce(json_extract(p.header_json,'$.canvas'),'{}'))),
    CASE WHEN h.row_json IS NOT NULL THEN json_set(h.row_json, '$.learnings',json((
        SELECT json_group_array(json(value)) FROM (
            SELECT key,value FROM (SELECT key,value FROM json_each(h.row_json,'$.learnings') ORDER BY key DESC LIMIT 10) ORDER BY key
        )))) END";

const CHAT_FILTERS: &str = "(?1 IS NULL OR p.project_path=?1) AND (?2 IS NULL OR coalesce(p.team_id,'')=?2)
      AND (?4 IS NULL OR p.id=?4)
      AND (?3 IS NULL OR (p.status='active' AND coalesce(json_extract(p.header_json,'$.mode'),'planning') <> 'complete'))";

const CHAT_ORDER: &str = "ORDER BY p.updated_at DESC,p.id";

const BOUND_PLAN_IDS: &str = "SELECT id FROM plans INDEXED BY plans_owner_tab_id WHERE owner_tab_id=?5
    UNION SELECT id FROM plans INDEXED BY plans_planner_tab_id WHERE json_extract(header_json,'$.planner.tab_id')=?5
    UNION SELECT plan_id FROM plan_tasks INDEXED BY plan_tasks_assigned_tab_id WHERE assigned_tab_id=?5
    UNION SELECT plan_id FROM plan_tasks INDEXED BY plan_tasks_lane_tab_id WHERE json_extract(row_json,'$.lane.tab_id')=?5";

pub(super) fn header_sql(tab_scoped: bool) -> String {
    if tab_scoped {
        // Keep membership outermost so an unrelated tab never visits plan rows.
        format!(
            "WITH bound_plan_ids(plan_id) AS ({BOUND_PLAN_IDS}) SELECT {CHAT_FIELDS}
            FROM bound_plan_ids b CROSS JOIN plans p
            LEFT JOIN plan_heartbeat h ON h.plan_id=p.id
            WHERE p.id=b.plan_id AND {CHAT_FILTERS} {CHAT_ORDER}"
        )
    } else {
        format!(
            "SELECT {CHAT_FIELDS} FROM plans p LEFT JOIN plan_heartbeat h ON h.plan_id=p.id
            WHERE ?5 IS NULL AND {CHAT_FILTERS} {CHAT_ORDER}"
        )
    }
}

impl PlanDatabase {
    pub(in crate::service_plan) fn chat_projection(
        &self,
        project: Option<&str>,
        team: Option<&str>,
        query: Option<&str>,
        plan_id: Option<&str>,
        tab_id: Option<&str>,
    ) -> Result<Vec<Value>, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        record_query();
        let headers = {
            let sql = header_sql(tab_id.is_some());
            let mut statement = transaction.prepare_cached(&sql).map_err(db_error)?;
            let rows = statement
                .query_map(params![project, team, query, plan_id, tab_id], |row| {
                    Ok((
                        column::<String>(row, 0)?,
                        column::<String>(row, 1)?,
                        column::<Option<String>>(row, 2)?,
                    ))
                })
                .map_err(db_error)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?
        };
        let mut remaining = 50;
        let mut result = Vec::new();
        for (id, header, heartbeat) in headers {
            let mut plan: ChatPlanRead = decode(&id, &header)?;
            if let Some(heartbeat) = heartbeat {
                plan.heartbeat = decode(&id, &heartbeat)?;
            }
            let tasks = chat_tasks(
                &transaction,
                &id,
                query,
                if tab_id.is_some() {
                    1
                } else if query.is_some() {
                    remaining
                } else {
                    remaining.min(20)
                },
                tab_id,
            )?;
            if query.is_none_or(|query| {
                plan.title.to_lowercase().contains(query)
                    || plan.id.contains(query)
                    || !tasks.is_empty()
            }) {
                let mut value =
                    super::super::chat_snapshot::read::snapshot(&plan, &tasks, &mut remaining);
                value["revision"] =
                    serde_json::json!(super::change_feed::revision(&transaction, &id)?.to_string());
                value["task_counts"] =
                    serde_json::json!(super::board_state_queries::task_counts(&transaction, &id)?);
                super::list_window_queries::apply_task_counts(&transaction, &id, &mut value)?;
                result.push(value);
            }
            if query.is_some() && result.len() >= 50 {
                break;
            }
        }
        transaction.commit().map_err(db_error)?;
        Ok(result)
    }
}

fn chat_tasks(
    connection: &Connection,
    plan_id: &str,
    query: Option<&str>,
    limit: usize,
    tab_id: Option<&str>,
) -> Result<Vec<ChatTaskRead>, PlanError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    record_query();
    let mut statement = connection.prepare_cached(
        "SELECT task_id FROM plan_tasks WHERE plan_id=?1 AND (?4 IS NULL OR (status NOT IN ('done','cancelled','paused') AND (assigned_tab_id=?4 OR json_extract(row_json,'$.lane.tab_id')=?4))) AND (?2 IS NULL OR instr(domios_casefold(task_id || ' ' || coalesce(json_extract(row_json,'$.key'),CASE WHEN substr(task_id,1,5)='task_' THEN substr(task_id,6) ELSE task_id END) || ' ' || coalesce(json_extract(row_json,'$.name'),'')),?2)>0)
         ORDER BY CASE WHEN ?4 IS NOT NULL THEN CASE status WHEN 'in_progress' THEN 0 WHEN 'in_review' THEN 1 WHEN 'blocked' THEN 2 ELSE 3 END ELSE 0 END,
            CASE WHEN ?4 IS NOT NULL THEN coalesce(json_extract(row_json,'$.status_changed_at'),json_extract(row_json,'$.updated_at'),json_extract(row_json,'$.created_at'),0) ELSE updated_at END DESC,
            CASE WHEN ?4 IS NULL THEN -position ELSE position END,task_id LIMIT ?3"
    ).map_err(db_error)?;
    let ids = statement
        .query_map(params![plan_id, query, limit as i64, tab_id], |row| {
            column::<String>(row, 0)
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    let mut tasks = Vec::new();
    for id in ids {
        record_query();
        let document: String = connection.query_row(
            "SELECT json_object(
                'id',task_id,'key',json_extract(row_json,'$.key'),'name',json_extract(row_json,'$.name'),
                'type',json_extract(row_json,'$.type'),'status',status,'status_label',json_extract(row_json,'$.status_label'),
                'model',json_extract(row_json,'$.model'),'assigned_tab_id',assigned_tab_id,
                'lane',json(coalesce(json_extract(row_json,'$.lane'),'{}')),
                'parent_task_id',json_extract(row_json,'$.parent_task_id'),'peer',json_extract(row_json,'$.peer'),
                'created_at',coalesce(json_extract(row_json,'$.created_at'),0),
                'updated_at',coalesce(json_extract(row_json,'$.updated_at'),0),
                'status_changed_at',coalesce(json_extract(row_json,'$.status_changed_at'),0),
                'section_ids',json(coalesce(json_extract(row_json,'$.section_ids'),'[]')),
                'canvas',json(coalesce(json_extract(row_json,'$.canvas'),'{}')),
                'acceptance_criteria',json((SELECT json_group_array('') FROM (SELECT value FROM json_each(t.row_json,'$.acceptance_criteria') LIMIT 1))),
                'milestones',json((SELECT json_group_array(json(document)) FROM (
                    SELECT seq,json_object('id',json_extract(row_json,'$.id'),'ts',json_extract(row_json,'$.ts'),
                        'kind',json_extract(row_json,'$.kind'),'from',json_extract(row_json,'$.from'),
                        'text','','links',json('[]'),'loop_id',json_extract(row_json,'$.loop_id'),
                        'iteration',json_extract(row_json,'$.iteration'),'canvas',json(coalesce(json_extract(row_json,'$.canvas'),'{}'))) AS document
                    FROM (SELECT seq,row_json FROM plan_task_milestones WHERE plan_id=?1 AND task_id=?2 ORDER BY seq DESC LIMIT 1) ORDER BY seq
                ))),
                'execution_attempts',json((SELECT json_group_array(json(document)) FROM (
                    SELECT json_object('id',attempt_id,'executor_tab_id',executor_tab_id,
                        'role',json_extract(row_json,'$.role'),'model',json_extract(row_json,'$.model'),
                        'session_model',json_extract(row_json,'$.session_model'),'assigned_at',json_extract(row_json,'$.assigned_at')) AS document
                    FROM plan_task_attempts WHERE plan_id=?1 AND task_id=?2
                    ORDER BY json_extract(row_json,'$.assigned_at') DESC,position DESC LIMIT 1
                )))
            ) FROM plan_tasks t WHERE plan_id=?1 AND task_id=?2",
            params![plan_id,id], |row| column(row,0),
        ).map_err(db_error)?;
        tasks.push(decode(plan_id, &document)?);
    }
    Ok(tasks)
}

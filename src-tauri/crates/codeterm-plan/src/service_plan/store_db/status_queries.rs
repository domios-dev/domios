use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::{Milestone, Plan, Section, Task, TaskExecutionAttempt, TaskStatus};
use crate::service_plan::state::mission_control::BounceSummary;
use crate::service_plan::status_projection::PlanStatusRead;
use crate::service_plan::PlanError;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

const HEADER: &str = "SELECT json_set(json_remove(header_json,
    '$.planning_log','$.owner_history','$.lifecycle_events','$.pause_operations',
    '$.planner_requests','$.planner_inbox'),
    '$.control_loop.rounds',json((SELECT json_group_array(json(value)) FROM (
        SELECT value FROM (SELECT key,value FROM json_each(p.header_json,'$.control_loop.rounds') WHERE json_extract(value,'$.ended_at')>=?2 ORDER BY key DESC LIMIT 20) ORDER BY key))))
    FROM plans p WHERE id=?1";

const TASKS: &str = "SELECT json_set(json_remove(row_json,
    '$.body','$.test_plan','$.qa_plan','$.model_rationale','$.intent','$.correctness',
    '$.files','$.attachments','$.review_audit','$.review_override','$.interruption','$.role_guidance'),
    '$.body',domios_skill_refs(json_extract(row_json,'$.body'),json_extract(row_json,'$.test_plan'),json_extract(row_json,'$.qa_plan'),?3),
    '$.acceptance_criteria',json((SELECT json_group_array('') FROM json_each(t.row_json,'$.acceptance_criteria'))),
    '$.review_evidence',json(CASE WHEN json_array_length(row_json,'$.review_evidence')>0
        THEN json_array(json_set(json_extract(row_json,'$.review_evidence[#-1]'),'$.note',
            CASE WHEN length(trim(coalesce(json_extract(row_json,'$.review_evidence[#-1].note'),''),?2))>0 THEN 'held' ELSE '' END))
        ELSE '[]' END)),
    coalesce(json_extract(row_json,'$.interruption.handoff_required'),0)
    FROM plan_tasks t WHERE plan_id=?1 ORDER BY position";

const ATTEMPTS: &str = "SELECT task_id,json_object(
    'id',attempt_id,'executor_tab_id',executor_tab_id,'executor_title',json_extract(row_json,'$.executor_title'),
    'session_id',json_extract(row_json,'$.session_id'),'session_binding_nonce',json_extract(row_json,'$.session_binding_nonce'),
    'role',json_extract(row_json,'$.role'),'model',json_extract(row_json,'$.model'),
    'assigned_at',json_extract(row_json,'$.assigned_at'),'acknowledged_at',json_extract(row_json,'$.acknowledged_at'),
    'reported_at',json_extract(row_json,'$.reported_at'),'report_target_written_at',json_extract(row_json,'$.report_target_written_at'),
    'closed_at',json_extract(row_json,'$.closed_at'),'retired_at',json_extract(row_json,'$.retired_at'),
    'parent_tab_id',json_extract(row_json,'$.parent_tab_id'),'orchestrator_tab_id',json_extract(row_json,'$.orchestrator_tab_id'),
    'competition_group_id',json_extract(row_json,'$.competition_group_id'),
    'report',CASE WHEN json_type(row_json,'$.report')='object' THEN json_object(
        'id',coalesce(json_extract(row_json,'$.report.id'),''),'status',json_extract(row_json,'$.report.status')) END)
    FROM plan_task_attempts WHERE plan_id=?1 AND json_extract(row_json,'$.closed_at') IS NULL
    ORDER BY task_id,position";

const RECENT: &str = "WITH history AS (
    SELECT m.task_id,t.position AS task_position,m.seq,0 AS phase,json_extract(m.row_json,'$.ts') AS ts,m.row_json
    FROM plan_task_milestones m JOIN plan_tasks t USING(plan_id,task_id) WHERE m.plan_id=?1
    UNION ALL SELECT NULL,0,seq,1,json_extract(row_json,'$.ts'),row_json FROM plan_strides WHERE plan_id=?1
    ) SELECT task_id,json_remove(row_json,'$.attachments','$.canvas') FROM history
    ORDER BY ts DESC,phase,task_position,seq LIMIT 10";

const REGRESSION: &str = "WITH records AS (
    SELECT row_json,seq,json_extract(row_json,'$.kind') AS kind,
           row_number() OVER (PARTITION BY json_extract(row_json,'$.kind'),link.value
              ORDER BY json_extract(row_json,'$.ts') DESC,seq DESC) AS rank
    FROM plan_strides,json_each(row_json,'$.links') link
    WHERE plan_id=?1 AND json_extract(row_json,'$.kind') IN ('regression','group_close')
      AND link.value LIKE 'group:%'
    ) SELECT DISTINCT json_remove(row_json,'$.text','$.attachments','$.canvas') FROM records WHERE rank=1";

pub(super) const WHITESPACE: &str = "\t\n\u{b}\u{c}\r \u{85}\u{a0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}";

impl PlanDatabase {
    pub(in crate::service_plan) fn heartbeat_owner_candidates(
        &self,
        predicate: &crate::service_plan::PlanMatch,
    ) -> Result<Vec<Plan>, PlanError> {
        let (clause, params) = crate::service_plan::plan_match::compile(predicate);
        let connection = self.connection.lock();
        record_query();
        let mut statement=connection.prepare_cached(&format!(
            "SELECT p.id,json_object('id',p.id,'title',p.title,'status',p.status,
                'project_path','','created_at',0,'updated_at',p.updated_at,
                'owner',json_extract(p.header_json,'$.owner'),'mode',coalesce(json_extract(p.header_json,'$.mode'),'planning'),
                'tasks',json(CASE WHEN EXISTS(SELECT 1 FROM plan_tasks t WHERE t.plan_id=p.id AND t.status IN ('pending','in_progress','in_review','blocked'))
                    THEN '[{{\"id\":\"\",\"name\":\"\",\"type\":\"\",\"status\":\"pending\"}}]' ELSE '[]' END))
             FROM plans p WHERE {clause} ORDER BY p.updated_at DESC,p.id"
        )).map_err(db_error)?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(params), |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?;
        let mut result = Vec::new();
        for row in rows {
            let (id, document) = row.map_err(db_error)?;
            result.push(decode(&id, &document)?);
        }
        Ok(result)
    }

    pub(in crate::service_plan) fn status_projection(
        &self,
        plan_id: &str,
        now_ms: u64,
        selected_learnings: &[String],
    ) -> Result<PlanStatusRead, PlanError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction().map_err(db_error)?;
        record_query();
        let header: String = transaction
            .query_row(
                HEADER,
                params![plan_id, now_ms.saturating_sub(7 * 24 * 60 * 60 * 1000)],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or(PlanError::NotFound)?;
        let mut plan: Plan = decode(plan_id, &header)?;
        load_tasks(&transaction, &mut plan)?;
        load_sections(&transaction, &mut plan)?;
        load_attempts(&transaction, &mut plan)?;
        load_history(&transaction, &mut plan)?;
        load_heartbeat(&transaction, &mut plan, selected_learnings)?;
        refresh_control_counts(&transaction, &mut plan, now_ms)?;
        let (bounces, held_reviews, lane_history_count) = history_facts(&transaction, &plan)?;
        let event_log = super::state_event_queries::event_log(&transaction, plan_id)?;
        record_query();
        let (undelivered_requests,delivery_uncertain) = transaction.query_row(
            "SELECT (SELECT count(*) FROM json_each(p.header_json,'$.planner_requests') WHERE json_extract(value,'$.state')<>'completed'),
                    EXISTS(SELECT 1 FROM json_each(p.header_json,'$.planner_inbox') WHERE json_extract(value,'$.phase')='delivery_uncertain')
             FROM plans p WHERE id=?1",[plan_id],|row| Ok((row.get(0)?,row.get(1)?))
        ).map_err(db_error)?;
        let revision = super::change_feed::revision(&transaction, plan_id)?.to_string();
        transaction.commit().map_err(db_error)?;
        Ok(PlanStatusRead {
            revision,
            plan,
            event_log,
            bounces,
            held_reviews,
            lane_history_count,
            undelivered_requests,
            delivery_uncertain,
        })
    }
}

fn load_tasks(connection: &Connection, plan: &mut Plan) -> Result<(), PlanError> {
    let slugs: Vec<_> = crate::service_plan::skill_registry::list_plan_skill_summaries(plan)
        .into_iter()
        .map(|skill| skill.slug)
        .collect();
    let slugs = serde_json::to_string(&slugs).map_err(|error| PlanError::Io(error.to_string()))?;
    record_query();
    let mut statement = connection.prepare_cached(TASKS).map_err(db_error)?;
    for row in statement
        .query_map(params![plan.id, WHITESPACE, slugs], |row| {
            Ok((column::<String>(row, 0)?, row.get::<_, bool>(1)?))
        })
        .map_err(db_error)?
    {
        let (document, handoff) = row.map_err(db_error)?;
        let mut task: Task = decode(&plan.id, &document)?;
        task.interruption.handoff_required = handoff;
        plan.tasks.push(task);
    }
    Ok(())
}

pub(super) fn register_functions(connection: &Connection) -> Result<(), PlanError> {
    connection
        .create_scalar_function(
            "domios_skill_refs",
            4,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let encoded: String = context.get(3)?;
                let slugs: Vec<String> = serde_json::from_str(&encoded)
                    .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
                let mut matches = Vec::new();
                for slug in slugs {
                    let mut matched = false;
                    for index in 0..3 {
                        if let rusqlite::types::ValueRef::Text(bytes) = context.get_raw(index) {
                            let source = std::str::from_utf8(bytes).map_err(|error| {
                                rusqlite::Error::UserFunctionError(Box::new(error))
                            })?;
                            matched |= source
                                .split(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
                                .any(|token| token == slug);
                        }
                    }
                    if matched {
                        matches.push(slug);
                    }
                }
                Ok(matches.join(" "))
            },
        )
        .map_err(db_error)?;
    connection
        .create_scalar_function(
            "domios_readiness_hint",
            1,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
            |context| {
                let source = match context.get_raw(0) {
                    rusqlite::types::ValueRef::Text(bytes) => std::str::from_utf8(bytes)
                        .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?,
                    _ => "",
                };
                let approval = ["approval", "self-review", "review gate"]
                    .iter()
                    .any(|needle| lowercase_contains(source, needle));
                let verification = [
                    "verification",
                    "test plan",
                    "build.sh",
                    "cargo test",
                    "playwright",
                ]
                .iter()
                .any(|needle| lowercase_contains(source, needle));
                Ok(i32::from(approval) | (i32::from(verification) << 1))
            },
        )
        .map_err(db_error)
}

fn lowercase_contains(source: &str, needle: &str) -> bool {
    let mut window = std::collections::VecDeque::with_capacity(needle.len());
    for ch in source.chars().flat_map(char::to_lowercase) {
        window.push_back(ch);
        if window.len() > needle.len() {
            window.pop_front();
        }
        if window.iter().copied().eq(needle.chars()) {
            return true;
        }
    }
    false
}

fn load_sections(connection: &Connection, plan: &mut Plan) -> Result<(), PlanError> {
    record_query();
    let mut statement = connection.prepare_cached(
        "SELECT json_object('id',json_extract(j.value,'$.id'),'title',json_extract(j.value,'$.title'),
            'body',CASE WHEN (domios_readiness_hint(json_extract(j.value,'$.body')) & 1)<>0 THEN 'approval ' ELSE '' END
                   || CASE WHEN (domios_readiness_hint(json_extract(j.value,'$.body')) & 2)<>0 THEN 'verification' ELSE '' END)
         FROM plan_sections s,json_tree(s.row_json) j WHERE s.plan_id=?1 AND j.type='object'
            AND (j.fullkey='$' OR j.path LIKE '%.subsections') AND json_type(j.value,'$.id')='text'
         ORDER BY s.position,j.id"
    ).map_err(db_error)?;
    for document in statement
        .query_map([&plan.id], |row| column::<String>(row, 0))
        .map_err(db_error)?
    {
        plan.sections
            .push(decode::<Section>(&plan.id, &document.map_err(db_error)?)?);
    }
    Ok(())
}

fn load_attempts(connection: &Connection, plan: &mut Plan) -> Result<(), PlanError> {
    let indexes: HashMap<_, _> = plan
        .tasks
        .iter()
        .enumerate()
        .map(|(index, task)| (task.id.clone(), index))
        .collect();
    record_query();
    let mut statement = connection.prepare_cached(ATTEMPTS).map_err(db_error)?;
    for row in statement
        .query_map([&plan.id], |row| {
            Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(db_error)?
    {
        let (id, document) = row.map_err(db_error)?;
        let attempt: TaskExecutionAttempt = decode(&plan.id, &document)?;
        let index = indexes.get(&id).ok_or(PlanError::TaskNotFound)?;
        plan.tasks[*index].execution_attempts.push(attempt);
    }
    Ok(())
}

fn load_history(connection: &Connection, plan: &mut Plan) -> Result<(), PlanError> {
    record_query();
    let mut statement = connection.prepare_cached(RECENT).map_err(db_error)?;
    for row in statement
        .query_map([&plan.id], |row| {
            Ok((column::<Option<String>>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(db_error)?
    {
        let (task_id, document) = row.map_err(db_error)?;
        let milestone: Milestone = decode(&plan.id, &document)?;
        match task_id {
            Some(id) => plan
                .tasks
                .iter_mut()
                .find(|task| task.id == id)
                .ok_or(PlanError::TaskNotFound)?
                .milestones
                .push(milestone),
            None => plan.strides.push(milestone),
        }
    }
    record_query();
    let mut statement = connection.prepare_cached(REGRESSION).map_err(db_error)?;
    for document in statement
        .query_map([&plan.id], |row| column::<String>(row, 0))
        .map_err(db_error)?
    {
        let milestone: Milestone = decode(&plan.id, &document.map_err(db_error)?)?;
        if !plan
            .strides
            .iter()
            .any(|existing| existing.id == milestone.id)
        {
            plan.strides.push(milestone);
        }
    }
    Ok(())
}

fn load_heartbeat(
    connection: &Connection,
    plan: &mut Plan,
    selected_learnings: &[String],
) -> Result<(), PlanError> {
    record_query();
    let document: Option<String> = connection.query_row(
        "SELECT json_set(row_json,'$.learnings',json((SELECT json_group_array(json(value)) FROM (
            SELECT key,value FROM json_each(h.row_json,'$.learnings') WHERE json_extract(value,'$.tag')='owner'
               OR json_extract(value,'$.id') IN (SELECT value FROM json_each(?2))
               OR key IN (SELECT key FROM json_each(h.row_json,'$.learnings') WHERE coalesce(json_extract(value,'$.tag'),'')<>'owner' ORDER BY key DESC LIMIT 50)
            ORDER BY key)))) FROM plan_heartbeat h WHERE plan_id=?1",
        params![plan.id,serde_json::to_string(selected_learnings).map_err(|error| PlanError::Io(error.to_string()))?],|row| column(row,0)
    ).optional().map_err(db_error)?;
    if let Some(document) = document {
        plan.heartbeat = decode(&plan.id, &document)?;
    }
    Ok(())
}

type HistoryFacts = (BounceSummary, HashMap<String, (usize, Option<u64>)>, usize);

fn history_facts(connection: &Connection, plan: &Plan) -> Result<HistoryFacts, PlanError> {
    record_query();
    let mut statement = connection.prepare_cached(
        "SELECT t.task_id,(SELECT count(*) FROM plan_task_attempts a WHERE a.plan_id=t.plan_id AND a.task_id=t.task_id),
           (SELECT count(*) FROM plan_task_milestones m WHERE m.plan_id=t.plan_id AND m.task_id=t.task_id AND json_extract(m.row_json,'$.kind')='review'),
           (SELECT max(json_extract(m.row_json,'$.ts')) FROM plan_task_milestones m WHERE m.plan_id=t.plan_id AND m.task_id=t.task_id AND json_extract(m.row_json,'$.kind')='review')
         FROM plan_tasks t WHERE t.plan_id=?1 ORDER BY t.position"
    ).map_err(db_error)?;
    let rows = statement
        .query_map([&plan.id], |row| {
            Ok((
                column::<String>(row, 0)?,
                row.get::<_, usize>(1)?,
                row.get::<_, usize>(2)?,
                row.get::<_, Option<u64>>(3)?,
            ))
        })
        .map_err(db_error)?;
    let mut bounces = BounceSummary::default();
    let mut held = HashMap::new();
    let mut lane_history = 0;
    for (row, task) in rows.zip(&plan.tasks) {
        let (id, attempts, reviews, last_review) = row.map_err(db_error)?;
        bounces.total_attempts += attempts;
        bounces.tasks_on_second_attempt += usize::from(attempts >= 2);
        bounces.first_try_done += usize::from(task.status == TaskStatus::Done && attempts == 1);
        bounces.labelled_bounced +=
            usize::from(task.status_label.as_deref() == Some("review bounced"));
        lane_history += usize::from(
            attempts > 0
                || task.assigned_tab_id.is_some()
                || task.lane.tab_id.is_some()
                || task.lane.branch.is_some()
                || task.lane.worktree_path.is_some(),
        );
        if task.status_label.as_deref() == Some("review held") {
            held.insert(id, (reviews, last_review));
        }
    }
    Ok((bounces, held, lane_history))
}

pub(super) fn refresh_control_counts(
    connection: &Connection,
    plan: &mut Plan,
    now_ms: u64,
) -> Result<(), PlanError> {
    record_query();
    plan.control_loop.counts_7d=connection.query_row(
        "SELECT coalesce(sum(json_extract(r.value,'$.outcome') NOT IN ('quiet','nudged','fallback')),0),
                coalesce(sum(json_extract(r.value,'$.outcome')='quiet'),0),
                coalesce(sum(json_extract(r.value,'$.outcome')='nudged'),0),
                coalesce(sum(json_extract(r.value,'$.learnings')),0),
                coalesce(sum(json_extract(r.value,'$.save_id') IS NOT NULL),0),
                coalesce(sum(json_extract(r.value,'$.outcome')='fallback'),0)
         FROM plans p,json_each(p.header_json,'$.control_loop.rounds') r
         WHERE p.id=?1 AND json_extract(r.value,'$.ended_at')>=?2",
        params![plan.id,now_ms.saturating_sub(7*24*60*60*1000)], |row| Ok(crate::plan::CuratorCounts {
            rounds:row.get(0)?,quiet:row.get(1)?,nudged:row.get(2)?,learnings:row.get(3)?,saves:row.get(4)?,fallbacks:row.get(5)?,
        })
    ).map_err(db_error)?;
    Ok(())
}

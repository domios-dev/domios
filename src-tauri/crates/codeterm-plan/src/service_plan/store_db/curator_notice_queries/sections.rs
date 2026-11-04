use super::*;
use crate::plan::{ControlLoopMode, HeartbeatLearning, LoopStatus};
use crate::service_plan::{
    heartbeat, milestone_gates,
    next_actions::{compute_next_actions_from, ActionLoop, NextActionInput},
};

#[derive(serde::Deserialize)]
struct LoopRead {
    id: String,
    status: LoopStatus,
    count: u32,
    budget: u32,
}

pub(super) fn loops(render: &mut Render<'_>) -> Result<bool, PlanError> {
    record_query();
    let connection = render.connection;
    let mut statement=connection.prepare_cached("SELECT json_object('id',json_extract(value,'$.id'),'status',json_extract(value,'$.status'),'count',CASE WHEN json_type(value,'$.count') IS NULL THEN 0 ELSE json_extract(value,'$.count') END,'budget',json_extract(value,'$.budget')) FROM plans p,json_each(CASE WHEN json_type(p.header_json,'$.loops') IS NULL THEN '[]' WHEN json_type(p.header_json,'$.loops')='array' THEN p.header_json->'$.loops' ELSE json('invalid notice loops') END) WHERE p.id=?1 AND domios_notice_loop_due(json_extract(value,'$.status'),CASE WHEN json_type(value,'$.count') IS NULL THEN 0 ELSE json_extract(value,'$.count') END,json_extract(value,'$.budget')) ORDER BY key LIMIT ?2").map_err(db_error)?;
    let mut rows = statement
        .query(params![render.id, MAX_REMINDER_BYTES + 1])
        .map_err(db_error)?;
    while let Some(row) = rows.next().map_err(db_error)? {
        let item: LoopRead = decode(render.id, &column::<String>(row, 0).map_err(db_error)?)?;
        let input = NextActionInput {
            plan_id: render.id,
            execution_tasks_unlocked: true,
            planning_approval_blockers: &[],
            planner_resolution: &crate::plan::peer::PeerResolution::Unbound,
            open_lanes: Vec::new(),
            blocked_lanes: Vec::new(),
            drift_total: 0,
            has_live_agents: false,
            loops: vec![ActionLoop {
                id: &item.id,
                status: item.status,
                count: item.count,
                budget: item.budget,
            }],
            pending_regression_gates: &[],
            pending_group_retrospectives: &[],
        };
        for action in compute_next_actions_from(&input) {
            if !render.append(&format!("- {}\n", action.message))? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(super) fn learnings(render: &mut Render<'_>) -> Result<bool, PlanError> {
    for owner in [true, false] {
        record_query();
        let sql = if owner {
            "SELECT value FROM plan_heartbeat h,json_each(h.row_json,'$.learnings') WHERE h.plan_id=?1 AND json_extract(value,'$.tag')='owner' ORDER BY key LIMIT ?2"
        } else {
            "SELECT value FROM plan_heartbeat h,json_each(h.row_json,'$.learnings') WHERE h.plan_id=?1 AND coalesce(json_extract(value,'$.tag'),'')<>'owner' AND length(trim(json_extract(value,'$.text'),?3))>0 ORDER BY key DESC LIMIT ?2"
        };
        let connection = render.connection;
        let mut statement = connection.prepare_cached(sql).map_err(db_error)?;
        let mut rows = if owner {
            statement.query(params![render.id, MAX_REMINDER_BYTES + 1])
        } else {
            statement.query(params![
                render.id,
                heartbeat::MAX_SITUATIONAL_LEARNINGS,
                super::super::status_queries::WHITESPACE
            ])
        }
        .map_err(db_error)?;
        while let Some(row) = rows.next().map_err(db_error)? {
            let learning: HeartbeatLearning =
                decode(render.id, &column::<String>(row, 0).map_err(db_error)?)?;
            let mut line = String::new();
            if owner {
                heartbeat::render_learning(&learning, &mut line);
            } else {
                heartbeat::render_situational_learning(&learning, &mut line);
            }
            if !render.append(&line)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(super) fn regression(render: &mut Render<'_>) -> Result<bool, PlanError> {
    let input = super::super::document_queries::regression::regression_gate_input(
        render.connection,
        render.id,
    )?
    .ok_or(PlanError::NotFound)?;
    for (retrospective, gates) in [
        (
            false,
            milestone_gates::pending_regression_gates(input.clone()),
        ),
        (true, milestone_gates::pending_group_retrospectives(input)),
    ] {
        if !render.append(&heartbeat::regression_line(&gates, retrospective))? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn competition(render: &mut Render<'_>) -> Result<bool, PlanError> {
    record_query();
    let name = facts::text_sql("t.row_json", "name", false);
    let sql=format!("{}, competing AS (
        SELECT f.task_id,f.position,(SELECT count(*) FROM plan_task_attempts a WHERE a.plan_id=?1 AND a.task_id=f.task_id AND json_extract(a.row_json,'$.closed_at') IS NULL) AS attempts
        FROM facts f WHERE f.status='in_review' AND json_extract(f.decision,'$.category')>=0
    ) SELECT c.task_id,{name},c.attempts FROM competing c JOIN plan_tasks t ON t.plan_id=?1 AND t.task_id=c.task_id
        WHERE c.attempts>1 ORDER BY c.position LIMIT ?3",facts::task_facts());
    let connection = render.connection;
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let mut rows = statement
        .query(params![
            render.id,
            super::super::status_queries::WHITESPACE,
            MAX_REMINDER_BYTES + 1
        ])
        .map_err(db_error)?;
    let mut first = true;
    while let Some(row) = rows.next().map_err(db_error)? {
        if first {
            first = false;
            if !render
                .append(crate::service_plan::prompts_plan::HEARTBEAT_COMPETITIVE_ATTEMPTS_TITLE)?
            {
                return Ok(false);
            }
        }
        let id: String = column(row, 0).map_err(db_error)?;
        let name: String = column(row, 1).map_err(db_error)?;
        let count = row.get(2).map_err(db_error)?;
        if !render.append(&heartbeat::competitive_attempt_summary_line(
            &id, &name, count,
        ))? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(serde::Deserialize)]
struct GateRead {
    id: String,
    question: String,
    asked_at: u64,
    routed: Option<String>,
    blocking: usize,
}

pub(super) fn gates(render: &mut Render<'_>, now: u64) -> Result<bool, PlanError> {
    record_query();
    let at = super::super::document_queries::regression::unsigned_timestamp(
        "g.value", "asked_at", false,
    );
    let question = facts::text_sql("g.value", "question", false);
    let sql=format!("SELECT json_object('id',json_extract(g.value,'$.id'),'question',domios_notice_excerpt({question},100),
        'asked_at',json({at}),'routed',json_extract(g.value,'$.routed_to_tab_id'),
        'blocking',CASE WHEN json_type(g.value,'$.blocking_task_ids') IS NULL THEN 0 WHEN json_type(g.value,'$.blocking_task_ids')='array' THEN json_array_length(g.value,'$.blocking_task_ids') ELSE json('invalid gate task list') END)
        FROM plans p,json_each(CASE WHEN json_type(p.header_json,'$.gates') IS NULL THEN '[]' WHEN json_type(p.header_json,'$.gates')='array' THEN p.header_json->'$.gates' ELSE json('invalid decision gates') END) g WHERE p.id=?1 AND CASE WHEN json_extract(g.value,'$.status') IN ('open','escalated','answered') THEN json_extract(g.value,'$.status') ELSE json('invalid decision gate status') END IN ('open','escalated') ORDER BY g.key LIMIT ?2");
    let connection = render.connection;
    let mut statement = connection.prepare_cached(&sql).map_err(db_error)?;
    let mut rows = statement
        .query(params![render.id, MAX_REMINDER_BYTES + 1])
        .map_err(db_error)?;
    let mut first = true;
    while let Some(row) = rows.next().map_err(db_error)? {
        let gate: GateRead = decode(render.id, &column::<String>(row, 0).map_err(db_error)?)?;
        if first {
            first = false;
            if !render.append("Decision gates:\n")? {
                return Ok(false);
            }
        }
        if !render.append(&heartbeat::gate_line(
            &gate.id,
            &gate.question,
            gate.asked_at,
            gate.blocking,
            gate.routed.as_deref(),
            now,
        ))? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(serde::Deserialize)]
struct ControlRead {
    mode: ControlLoopMode,
    next_at: Option<u64>,
    fallback: Option<String>,
}

pub(super) fn control(render: &mut Render<'_>) -> Result<bool, PlanError> {
    record_query();
    let encoded:String=render.connection.query_row("SELECT json_object('mode',CASE WHEN json_type(header_json,'$.control_loop.mode') IS NULL THEN 'curator' ELSE json_extract(header_json,'$.control_loop.mode') END,'next_at',json(header_json->'$.control_loop.next_at'),'fallback',json_extract(header_json,'$.control_loop.fallback_reason')) FROM plans WHERE id=?1",[render.id],|row|column(row,0)).map_err(db_error)?;
    let control: ControlRead = decode(render.id, &encoded)?;
    render.append(&format!(
        "- {}\n",
        heartbeat::view::control_line(&control.mode, control.next_at, control.fallback.as_deref())
    ))
}

pub(super) fn skills(render: &mut Render<'_>, header: &Header) -> Result<bool, PlanError> {
    if render.action_ids.is_empty() {
        return Ok(true);
    }
    record_query();
    let (project,canonical):(String,String)=render.connection.query_row("SELECT project_path,CASE WHEN json_type(header_json,'$.canonical_project_path') IS NULL THEN '' ELSE json_extract(header_json,'$.canonical_project_path') END FROM plans WHERE id=?1",[render.id],|row|Ok((column(row,0)?,column(row,1)?))).map_err(db_error)?;
    let root = crate::service_plan::paths::sidecar_root(&project, &canonical);
    let directory =
        crate::service_plan::paths::plan_state_dir_unchecked(root, &header.status, &header.id);
    let manifest = crate::service_plan::plan_skills::plan_skill_manifest_from_dir(&directory)?;
    if manifest.is_empty() {
        return Ok(true);
    }
    let slugs: Vec<_> = manifest.iter().map(|entry| entry.slug.as_str()).collect();
    let encoded =
        serde_json::to_string(&slugs).map_err(|error| PlanError::Io(error.to_string()))?;
    let ids = serde_json::to_string(&render.action_ids)
        .map_err(|error| PlanError::Io(error.to_string()))?;
    record_query();
    let references = {
        let mut statement=render.connection.prepare_cached("SELECT domios_skill_refs(json_extract(row_json,'$.body'),json_extract(row_json,'$.test_plan'),json_extract(row_json,'$.qa_plan'),?2) FROM plan_tasks WHERE plan_id=?1 AND task_id IN(SELECT value FROM json_each(?3)) ORDER BY position").map_err(db_error)?;
        let rows = statement
            .query_map(params![render.id, encoded, ids], |row| {
                column::<String>(row, 0)
            })
            .map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?
    };
    let matched: HashSet<_> = references
        .iter()
        .flat_map(|line| line.split_whitespace())
        .collect();
    for slug in slugs {
        if matched.contains(slug)
            && !render.append(&format!(
                "- skill {slug}: domios plan skill show --plan {} {slug}\n",
                render.id
            ))?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

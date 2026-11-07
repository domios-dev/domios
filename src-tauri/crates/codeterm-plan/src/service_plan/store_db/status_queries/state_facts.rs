use super::*;
use crate::service_plan::state::input::{StateAttemptInput, StateInput, StateTaskInput};

const HEADER: &str = "WITH selected AS (SELECT json_group_object(h.key,json(p.header_json -> ('$.' || h.key))) AS document FROM plans p,json_each(p.header_json) h WHERE p.id=?1 AND h.key IN ('id','title','status','mode','owner','team_id','project_path','canonical_project_path','planner','planning_state','goal','loops','task_groups','tag_facets','spec_refs','spec_links','primary_spec_id','control_loop','review_policy','gates','regression_gate_overrides','orphaned') GROUP BY p.id)
    SELECT json_remove(document,'$.control_loop.rounds') FROM selected";

const TASKS: &str = "SELECT json_set((SELECT json_group_object(h.key,json(t.row_json -> ('$.' || h.key))) FROM json_each(t.row_json) h WHERE h.key IN ('id','name','key','handle','status','review_stage','qa_state','status_label','status_changed_at','tags','section_ids','executor_role','competition','parent_task_id','depends_on','assigned_tab_id','lane','peer','group')),
    '$.review_evidence',json(CASE WHEN json_array_length(row_json,'$.review_evidence')>0
        THEN json_array(json_set(json_extract(row_json,'$.review_evidence[#-1]'),'$.note',
            CASE WHEN length(trim(coalesce(json_extract(row_json,'$.review_evidence[#-1].note'),''),?2))>0 THEN 'held' ELSE '' END))
        ELSE '[]' END)),
    coalesce(json_extract(row_json,'$.interruption.handoff_required'),0)
    FROM plan_tasks t WHERE plan_id=?1 ORDER BY position";

const ATTEMPTS: &str = "SELECT task_id,json_object(
    'id',attempt_id,'executor_tab_id',executor_tab_id,
    'reported_at',json_extract(row_json,'$.reported_at'),'report_target_written_at',json_extract(row_json,'$.report_target_written_at'),
    'closed_at',json_extract(row_json,'$.closed_at'),
    'parent_tab_id',json_extract(row_json,'$.parent_tab_id'),'orchestrator_tab_id',json_extract(row_json,'$.orchestrator_tab_id'),
    'competition_group_id',json_extract(row_json,'$.competition_group_id'),
    'report_status',json_extract(row_json,'$.report.status'))
    FROM plan_task_attempts WHERE plan_id=?1 AND json_extract(row_json,'$.closed_at') IS NULL ORDER BY task_id,position";

pub(super) fn load_status_projection(
    connection: &Connection,
    plan_id: &str,
    now_ms: u64,
) -> Result<PlanStatusRead, PlanError> {
    record_query();
    let header: String = connection
        .query_row(HEADER, params![plan_id], |row| column(row, 0))
        .optional()
        .map_err(db_error)?
        .ok_or(PlanError::NotFound)?;
    let mut input: StateInput = decode(plan_id, &header)?;
    input.execution_tasks_unlocked =
        codeterm_types::plan::execution_tasks_unlocked_for(&input.mode, input.planning_state);
    input.orphaned_active = codeterm_types::plan::orphan::orphaned_active_for(
        &input.status,
        input.team_id.as_deref(),
        input.orphaned.as_ref(),
    );
    record_query();
    let mut statement = connection.prepare_cached(TASKS).map_err(db_error)?;
    for row in statement
        .query_map(params![plan_id, WHITESPACE], |row| {
            Ok((column::<String>(row, 0)?, row.get::<_, bool>(1)?))
        })
        .map_err(db_error)?
    {
        let (document, handoff_required) = row.map_err(db_error)?;
        let mut task: StateTaskInput = decode(plan_id, &document)?;
        task.handoff_required = handoff_required;
        input.tasks.push(task);
    }
    record_query();
    let mut statement = connection.prepare_cached(
        "SELECT json_object('id',json_extract(j.value,'$.id'),'title',json_extract(j.value,'$.title'),
            'body',CASE WHEN (domios_readiness_hint(json_extract(j.value,'$.body')) & 1)<>0 THEN 'approval ' ELSE '' END
                   || CASE WHEN (domios_readiness_hint(json_extract(j.value,'$.body')) & 2)<>0 THEN 'verification' ELSE '' END)
         FROM plan_sections s,json_tree(s.row_json) j WHERE s.plan_id=?1 AND j.type='object'
            AND (j.fullkey='$' OR j.path LIKE '%.subsections') AND json_type(j.value,'$.id')='text' ORDER BY s.position,j.id"
    ).map_err(db_error)?;
    for document in statement
        .query_map([plan_id], |row| column::<String>(row, 0))
        .map_err(db_error)?
    {
        input
            .sections
            .push(decode(plan_id, &document.map_err(db_error)?)?);
    }
    let indexes: HashMap<_, _> = input
        .tasks
        .iter()
        .enumerate()
        .map(|(index, task)| (task.id.clone(), index))
        .collect();
    record_query();
    let mut statement = connection.prepare_cached(ATTEMPTS).map_err(db_error)?;
    for row in statement
        .query_map([plan_id], |row| {
            Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(db_error)?
    {
        let (id, document) = row.map_err(db_error)?;
        let index = indexes.get(&id).ok_or(PlanError::TaskNotFound)?;
        input.tasks[*index]
            .execution_attempts
            .push(decode::<StateAttemptInput>(plan_id, &document)?);
    }
    load_history(connection, &mut input)?;
    record_query();
    let document: Option<String> = connection.query_row("SELECT json_object('current_focus',json_extract(row_json,'$.current_focus'),'active_lanes',json(coalesce(json_extract(row_json,'$.active_lanes'),'[]'))) FROM plan_heartbeat WHERE plan_id=?1",[plan_id],|row| column(row,0)).optional().map_err(db_error)?;
    if let Some(document) = document {
        input.heartbeat = decode(plan_id, &document)?;
    }
    input.control_loop.counts_7d = control_counts(connection, plan_id, now_ms)?;
    let (bounces, held_reviews, lane_history_count) = history_facts(connection, &input)?;
    let event_log = super::super::state_event_queries::event_log(connection, plan_id)?;
    record_query();
    let (undelivered_requests,delivery_uncertain) = connection.query_row(
        "SELECT (SELECT count(*) FROM json_each(p.header_json,'$.planner_requests') WHERE json_extract(value,'$.state')<>'completed'),
            EXISTS(SELECT 1 FROM json_each(p.header_json,'$.planner_inbox') WHERE json_extract(value,'$.phase')='delivery_uncertain')
        FROM plans p WHERE id=?1",[plan_id],|row| Ok((row.get(0)?,row.get(1)?))).map_err(db_error)?;
    input.undelivered_request_count = undelivered_requests;
    input.delivery_uncertain = delivery_uncertain;
    let revision = super::super::change_feed::revision(connection, plan_id)?.to_string();
    Ok(PlanStatusRead {
        revision,
        input,
        event_log,
        bounces,
        held_reviews,
        lane_history_count,
        undelivered_requests,
        delivery_uncertain,
    })
}

fn load_history(connection: &Connection, input: &mut StateInput) -> Result<(), PlanError> {
    record_query();
    let mut statement = connection.prepare_cached(RECENT).map_err(db_error)?;
    for row in statement
        .query_map([&input.id], |row| {
            Ok((column::<Option<String>>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(db_error)?
    {
        let (task_id, document) = row.map_err(db_error)?;
        let milestone: Milestone = decode(&input.id, &document)?;
        match task_id {
            Some(id) => input
                .tasks
                .iter_mut()
                .find(|task| task.id == id)
                .ok_or(PlanError::TaskNotFound)?
                .milestones
                .push(milestone),
            None => input.strides.push(milestone),
        }
    }
    record_query();
    let mut statement = connection.prepare_cached(REGRESSION).map_err(db_error)?;
    for document in statement
        .query_map([&input.id], |row| column::<String>(row, 0))
        .map_err(db_error)?
    {
        let milestone: Milestone = decode(&input.id, &document.map_err(db_error)?)?;
        if !input
            .strides
            .iter()
            .any(|existing| existing.id == milestone.id)
        {
            input.strides.push(milestone);
        }
    }
    Ok(())
}

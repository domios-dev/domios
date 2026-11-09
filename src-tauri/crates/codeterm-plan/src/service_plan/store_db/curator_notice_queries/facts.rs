use super::super::{db_error, PlanError};
use crate::plan::{peer::PeerResolution, QaState, ReviewStage, TaskStatus};
use crate::service_plan::next_actions::{
    compute_next_actions_from, ActionLane, NextActionInput, NextActionKind,
};
use crate::service_plan::state::{project_task_facts, DependencyWait, TaskPresentation};
use rusqlite::{
    functions::{Context, FunctionFlags},
    Connection,
};
use serde::de::DeserializeOwned;

fn text<'a>(context: &'a Context<'_>, index: usize) -> rusqlite::Result<Option<&'a str>> {
    match context.get_raw(index) {
        rusqlite::types::ValueRef::Null => Ok(None),
        rusqlite::types::ValueRef::Text(bytes) => std::str::from_utf8(bytes)
            .map(Some)
            .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error))),
        _ => Err(rusqlite::Error::UserFunctionError(
            "invalid notice text field".into(),
        )),
    }
}

fn enumeration<T: DeserializeOwned>(value: &str) -> rusqlite::Result<T> {
    T::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(value))
        .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
}

pub(super) fn register(connection: &Connection) -> Result<(), PlanError> {
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_scalar_function("domios_notice_stalled", 3, flags, |context| {
            let changed = text(context, 0)?
                .unwrap_or("")
                .parse::<u64>()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            let now = text(context, 1)?
                .unwrap_or("")
                .parse::<u64>()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            let eligible: bool = context.get(2)?;
            Ok(eligible
                && changed > 0
                && crate::plan::reconcile::stale_threshold_ms(&TaskStatus::InProgress)
                    .is_some_and(|threshold| now.saturating_sub(changed) > threshold))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_notice_role", 3, flags, |context| {
            let id = text(context, 0)?
                .ok_or_else(|| rusqlite::Error::UserFunctionError("missing notice id".into()))?;
            let title = text(context, 1)?
                .ok_or_else(|| rusqlite::Error::UserFunctionError("missing notice title".into()))?;
            Ok(
                crate::service_plan::reminders::composition::role_header_fields(
                    id,
                    title,
                    text(context, 2)?,
                ),
            )
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_notice_excerpt", 2, flags, |context| {
            let source = text(context, 0)?
                .ok_or_else(|| rusqlite::Error::UserFunctionError("missing notice text".into()))?;
            let limit: usize = context.get(1)?;
            Ok(source.chars().take(limit).collect::<String>())
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_notice_learning", 3, flags, |context| {
            let id = text(context, 0)?.unwrap_or("");
            let value = text(context, 1)?.ok_or_else(|| {
                rusqlite::Error::UserFunctionError("missing learning text".into())
            })?;
            let owner: bool = context.get(2)?;
            let mut line = String::new();
            if owner {
                crate::service_plan::heartbeat::render_learning_text(value, &mut line);
            } else {
                crate::service_plan::heartbeat::render_situational_learning_fields(
                    id, value, &mut line,
                );
            }
            Ok(line)
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_notice_loop_due", 3, flags, |context| {
            let status = enumeration(text(context, 0)?.unwrap_or(""))?;
            let count = context.get(1)?;
            let budget = context.get(2)?;
            Ok(crate::service_plan::next_actions::loop_near_limit(
                &crate::service_plan::next_actions::ActionLoop {
                    id: "",
                    status,
                    count,
                    budget,
                },
            ))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_notice_label", 3, flags, |context| {
            let name = text(context, 0)?.ok_or_else(|| {
                rusqlite::Error::UserFunctionError("missing notice task name".into())
            })?;
            Ok(
                crate::service_plan::reminders::composition::task_label_fields(
                    name,
                    text(context, 1)?,
                    text(context, 2)?,
                ),
            )
        })
        .map_err(db_error)?;
    connection.create_scalar_function("domios_notice_decision",9,flags,|context| {
        let status:TaskStatus=enumeration(text(context,0)?.unwrap_or(""))?;
        let stage:ReviewStage=enumeration(text(context,1)?.unwrap_or("review"))?;
        let qa:Option<QaState>=text(context,2)?.map(enumeration).transpose()?;
        let waiting:bool=context.get(3)?;
        let missing:bool=context.get(4)?;
        let waits=if waiting {vec![DependencyWait {task_id:String::new(),name:String::new(),status:if missing {None} else {Some(TaskStatus::Pending)},missing}]} else {Vec::new()};
        let projection=project_task_facts(&status,waits,&stage,&qa,context.get(5)?,context.get(6)?,context.get(7)?);
        let held:bool=context.get(8)?;
        let orchestrator_actionable=!held && projection.needs_orchestrator_action();
        let reminder_worker=projection.presentation.worker_reminder_eligible(&status);
        let aged_eligible=projection.can_age() && !held;
        let category=if held || !projection.needs_orchestrator_action() {-1} else {
            match projection.presentation {
                TaskPresentation::InReview|TaskPresentation::QueuedForQa=>0,
                TaskPresentation::ActionablePending=>1,
                TaskPresentation::HandoffRequired|TaskPresentation::Blocked=>2,
                _=>-1,
            }
        };
        let parked=projection.presentation==TaskPresentation::ParkedForLaterGroup;
        let pending=projection.presentation==TaskPresentation::ActionablePending;
        let handoff=projection.presentation==TaskPresentation::HandoffRequired;
        let action=if held||!projection.needs_orchestrator_action() {0} else {
            let lane=ActionLane {task_id:"",name:"",status:status.clone(),review_stage:stage,qa_state:qa,
                presentation:projection.presentation.clone(),waiting_on_dependencies:waiting,
                dependency_waits:&projection.dependency_waits,competition:None};
            let blocked=status==TaskStatus::Blocked;
            let open=matches!(status,TaskStatus::Pending|TaskStatus::InProgress|TaskStatus::InReview);
            let input=NextActionInput {plan_id:"",execution_tasks_unlocked:true,planning_approval_blockers:&[],
                planner_resolution:&PeerResolution::Unbound,open_lanes:if open {vec![lane]} else {Vec::new()},
                blocked_lanes:if blocked {vec![lane_for_block(&projection,waiting)]} else {Vec::new()},
                drift_total:0,has_live_agents:false,loops:Vec::new(),pending_regression_gates:&[],pending_group_retrospectives:&[]};
            compute_next_actions_from(&input).iter().find_map(|action|match action.kind {
                NextActionKind::Gate=>Some(101),NextActionKind::ClaimQa=>Some(102),
                NextActionKind::Advance=>Some(201),NextActionKind::WaitingOnDependency=>Some(301),
                NextActionKind::Dispatch=>Some(501),_=>None,
            }).unwrap_or(if projection.presentation==TaskPresentation::HandoffRequired {601} else {0})
        };
        let blocked=crate::service_plan::state::outstanding_blocked_facts(&status,parked,!projection.dependency_waits.is_empty());
        Ok(format!("{{\"category\":{category},\"action\":{action},\"parked\":{},\"pending\":{},\"handoff\":{},\"held\":{},\"blocked\":{},\"aged_eligible\":{},\"reminder_worker\":{},\"orchestrator_actionable\":{}}}",i32::from(parked),i32::from(pending),i32::from(handoff),i32::from(held),i32::from(blocked),i32::from(aged_eligible),i32::from(reminder_worker),i32::from(orchestrator_actionable)))
    }).map_err(db_error)
}

fn lane_for_block(
    projection: &crate::service_plan::state::TaskProjection,
    waiting: bool,
) -> ActionLane<'_> {
    ActionLane {
        task_id: "",
        name: "",
        status: TaskStatus::Blocked,
        review_stage: ReviewStage::Review,
        qa_state: None,
        presentation: projection.presentation.clone(),
        waiting_on_dependencies: waiting,
        dependency_waits: &projection.dependency_waits,
        competition: None,
    }
}

pub(super) fn action_verb(code: i64) -> Result<&'static str, PlanError> {
    let kind = match code {
        101 => NextActionKind::Gate,
        102 => NextActionKind::ClaimQa,
        201 => NextActionKind::Advance,
        301 => NextActionKind::WaitingOnDependency,
        501 => NextActionKind::Dispatch,
        601 => return Ok(crate::service_plan::heartbeat::HANDOFF_ACTION_VERB),
        _ => return Err(PlanError::Io("invalid notice action code".into())),
    };
    crate::service_plan::heartbeat::action_verb(kind)
        .ok_or_else(|| PlanError::Io("notice action has no rendering".into()))
}

const TASK_FACTS:&str="WITH declared AS MATERIALIZED (
    SELECT CAST(key AS INTEGER) AS position,CASE WHEN json_type(value,'$.key')='text' THEN json_extract(value,'$.key') ELSE json('invalid declared group key') END AS group_key
    FROM plans p,json_each(CASE WHEN json_type(p.header_json,'$.task_groups') IS NULL THEN '[]' WHEN json_type(p.header_json,'$.task_groups')='array' THEN p.header_json->'$.task_groups' ELSE json('invalid task groups') END) WHERE p.id=?1
), group_facts AS MATERIALIZED (
    SELECT task_id,CASE WHEN json_type(row_json,'$.group')='text' THEN json_extract(row_json,'$.group') ELSE json('invalid task group') END AS group_key
    FROM plan_tasks WHERE plan_id=?1 AND status NOT IN ('done','cancelled')
        AND json_type(row_json,'$.group') IS NOT NULL AND json_type(row_json,'$.group')<>'null'
), open_groups AS MATERIALIZED (
    SELECT DISTINCT group_key FROM group_facts
), declared_positions AS MATERIALIZED (
    SELECT group_key,min(position) AS position FROM declared GROUP BY group_key
 ), active AS (
    SELECT min(d.position) AS position FROM declared d JOIN open_groups g USING(group_key)
), dependency_targets AS MATERIALIZED (
    SELECT task_id,status FROM plan_tasks WHERE plan_id=?1
), open_parents AS MATERIALIZED (
    SELECT DISTINCT CASE WHEN json_type(row_json,'$.parent_task_id')='text' THEN json_extract(row_json,'$.parent_task_id') ELSE json('invalid parent task id') END AS parent_id
    FROM plan_tasks WHERE plan_id=?1 AND status NOT IN ('done','cancelled')
        AND json_type(row_json,'$.parent_task_id') IS NOT NULL AND json_type(row_json,'$.parent_task_id')<>'null'
), facts AS MATERIALIZED (
    SELECT t.task_id,t.position,t.status,
        domios_notice_decision(t.status,CASE WHEN json_type(t.row_json,'$.review_stage') IS NULL THEN 'review' WHEN json_type(t.row_json,'$.review_stage')='text' THEN json_extract(t.row_json,'$.review_stage') ELSE json('invalid review stage') END,json_extract(t.row_json,'$.qa_state'),
            EXISTS(SELECT 1 FROM json_each(CASE WHEN json_type(t.row_json,'$.depends_on') IS NULL THEN '[]' WHEN json_type(t.row_json,'$.depends_on')='array' THEN t.row_json->'$.depends_on' ELSE json('invalid task dependencies') END) dep LEFT JOIN dependency_targets target
                ON target.task_id=CASE WHEN dep.type='text' THEN dep.value ELSE json('invalid task dependency') END WHERE target.task_id IS NULL OR target.status NOT IN ('done','cancelled')),
            EXISTS(SELECT 1 FROM json_each(CASE WHEN json_type(t.row_json,'$.depends_on') IS NULL THEN '[]' WHEN json_type(t.row_json,'$.depends_on')='array' THEN t.row_json->'$.depends_on' ELSE json('invalid task dependencies') END) dep WHERE NOT EXISTS(
                SELECT 1 FROM dependency_targets target WHERE target.task_id=CASE WHEN dep.type='text' THEN dep.value ELSE json('invalid task dependency') END)),
            CASE WHEN json_type(t.row_json,'$.interruption') IS NULL THEN 0
                WHEN json_type(t.row_json,'$.interruption')<>'object' THEN json('invalid task interruption')
                WHEN json_type(t.row_json,'$.interruption.handoff_required') IS NULL THEN 0
                WHEN json_type(t.row_json,'$.interruption.handoff_required') IN ('true','false') THEN json_extract(t.row_json,'$.interruption.handoff_required')
                ELSE json('invalid handoff-required flag') END,
            parent.parent_id IS NOT NULL,
            t.status='pending' AND coalesce(declared_position.position>(SELECT position FROM active),0),
            CASE WHEN t.status='in_review' THEN CASE
                WHEN json_type(t.row_json,'$.review_evidence') IS NULL THEN 0
                WHEN json_type(t.row_json,'$.review_evidence')<>'array' THEN json('invalid task review evidence')
                WHEN json_array_length(t.row_json,'$.review_evidence')=0 THEN 0
                WHEN coalesce(json_extract(t.row_json,'$.review_evidence[#-1].verdict') IN ('accept','bounce','hold'),0)=0 THEN json('invalid review verdict')
                WHEN json_extract(t.row_json,'$.review_evidence[#-1].verdict')='hold'
                THEN printf('%20s',NOTICE_REVIEW_AT)>=printf('%20s',NOTICE_CHANGED_AT)
                    AND length(trim(CASE WHEN json_type(t.row_json,'$.review_evidence[#-1].note') IS NULL THEN ''
                        WHEN json_type(t.row_json,'$.review_evidence[#-1].note')='text' THEN json_extract(t.row_json,'$.review_evidence[#-1].note')
                        ELSE json('invalid review hold note') END,?2))>0
                ELSE 0 END ELSE 0 END
        ) AS decision
    FROM plan_tasks t LEFT JOIN open_parents parent ON parent.parent_id=t.task_id
        LEFT JOIN group_facts group_fact ON group_fact.task_id=t.task_id
        LEFT JOIN declared_positions declared_position ON declared_position.group_key=group_fact.group_key
    WHERE t.plan_id=?1 NOTICE_TASK_SCOPE
)";

pub(super) fn task_facts() -> String {
    scoped_task_facts("")
}

pub(super) fn worker_task_facts() -> String {
    scoped_task_facts("AND t.assigned_tab_id IS NOT NULL AND t.status IN ('pending','in_progress','blocked') AND (?3 IS NULL OR trim(t.assigned_tab_id,?2)=?3)")
}

fn scoped_task_facts(scope: &str) -> String {
    use super::super::document_queries::regression::unsigned_timestamp;
    TASK_FACTS
        .replace("NOTICE_TASK_SCOPE", scope)
        .replace(
            "NOTICE_REVIEW_AT",
            &unsigned_timestamp("t.row_json", "review_evidence[#-1].at", false),
        )
        .replace(
            "NOTICE_CHANGED_AT",
            &unsigned_timestamp("t.row_json", "status_changed_at", true),
        )
}

pub(super) fn text_sql(document: &str, field: &str, optional: bool) -> String {
    let absent = if optional {
        "NULL"
    } else {
        "json('missing notice text')"
    };
    let null = if optional {
        "NULL"
    } else {
        "json('invalid notice text')"
    };
    format!(
        "CASE WHEN json_type({document},'$.{field}') IS NULL THEN {absent}
        WHEN json_type({document},'$.{field}')='null' THEN {null}
        WHEN json_type({document},'$.{field}')='text' THEN json_extract({document},'$.{field}')
        ELSE json('invalid notice text') END"
    )
}

pub(super) fn label_sql(document: &str) -> String {
    format!(
        "domios_notice_label({},{},{})",
        text_sql(document, "name", false),
        text_sql(document, "key", true),
        text_sql(document, "handle", true)
    )
}

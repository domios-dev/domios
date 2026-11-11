use std::cmp::Ordering;

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use base64::Engine;
pub use codeterm_plan::service_plan::team_mission_control::{
    TeamMissionControlEvent, TeamMissionControlQuery, TeamMissionControlState, TeamPlanGoal,
    TeamPlanSummary,
};
use codeterm_types::{plan::PlanLifecycleEvent, plan::PlanStatus, team::Team};

use crate::routes::AppState;
use codeterm_service_core::service::plan::{
    PlanError, PlanStore, TeamEventCursor, TeamPlanMetadataRead, TeamPlanRead,
};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 100;

trait TeamMissionControlQueryExt {
    fn bounded_limit(&self) -> usize;
}

impl TeamMissionControlQueryExt for TeamMissionControlQuery {
    fn bounded_limit(&self) -> usize {
        self.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
    }
}

trait TeamPlanSummaryRead {
    fn from_read(input: &TeamPlanRead) -> Self;
}

impl TeamPlanSummaryRead for TeamPlanSummary {
    fn from_read(input: &TeamPlanRead) -> Self {
        let plan = &input.metadata;
        Self {
            id: plan.id.clone(),
            title: plan.title.clone(),
            team_id: plan.team_id.clone(),
            status: plan.status.clone(),
            mode: plan.mode.clone(),
            planning_state: plan.planning_state,
            goal: plan.goal.as_ref().map(|goal| TeamPlanGoal {
                vector: goal.vector.clone(),
            }),
            updated_at: plan.updated_at,
            task_counts: input.task_counts.clone(),
        }
    }
}

type EventCursor = TeamEventCursor;

fn encode_cursor(event: &TeamMissionControlEvent) -> String {
    let payload = EventCursor {
        ts: event.ts,
        plan_id: event.plan_id.clone(),
        event_id: event.id.clone(),
    };
    let bytes = serde_json::to_vec(&payload).expect("event cursor is serializable");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn decode_cursor(raw: &str) -> Result<EventCursor, String> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw.as_bytes())
        .map_err(|_| "invalid event cursor".to_string())?;
    serde_json::from_slice(&bytes).map_err(|_| "invalid event cursor".to_string())
}

fn compare_events(left: &TeamMissionControlEvent, right: &TeamMissionControlEvent) -> Ordering {
    right
        .ts
        .cmp(&left.ts)
        .then_with(|| left.plan_id.cmp(&right.plan_id))
        .then_with(|| left.id.cmp(&right.id))
}

fn is_after_cursor(event: &TeamMissionControlEvent, cursor: &EventCursor) -> bool {
    event.ts < cursor.ts
        || (event.ts == cursor.ts
            && (event.plan_id > cursor.plan_id
                || (event.plan_id == cursor.plan_id && event.id > cursor.event_id)))
}

fn author_parts(author: &codeterm_types::plan::OrchestratorRef) -> (String, Option<String>) {
    let tab_id = author.tab_id.clone();
    let label = author
        .title
        .clone()
        .or_else(|| tab_id.clone())
        .unwrap_or_default();
    (label, tab_id)
}

fn lifecycle_event(
    plan: &TeamPlanMetadataRead,
    event: &PlanLifecycleEvent,
    ordinal: usize,
) -> TeamMissionControlEvent {
    let (author, author_tab_id) = author_parts(&event.author);
    TeamMissionControlEvent {
        id: if event.id.is_empty() {
            format!("lifecycle-{ordinal}-{}-{}", event.kind, event.at)
        } else {
            event.id.clone()
        },
        plan_id: plan.id.clone(),
        plan_title: plan.title.clone(),
        ts: event.at,
        last_ts: event.at,
        count: 1,
        kind: event.kind.clone(),
        author,
        author_tab_id,
        task_id: None,
        task_name: None,
        linked_tasks: vec![],
        text: format!("Plan {}", event.kind),
    }
}

fn stored_event(
    plan: &TeamPlanMetadataRead,
    event: &codeterm_plan::service_plan::state::mission_control::PlanEvent,
    ordinal: usize,
) -> TeamMissionControlEvent {
    TeamMissionControlEvent {
        id: format!(
            "stored-{ordinal}-{}-{}-{}",
            event.kind,
            event.ts,
            event.task_id.as_deref().unwrap_or_default()
        ),
        plan_id: plan.id.clone(),
        plan_title: plan.title.clone(),
        ts: event.ts,
        last_ts: event.last_ts,
        count: event.count,
        kind: event.kind.clone(),
        author: event.author.clone(),
        author_tab_id: event.author_tab_id.clone(),
        task_id: event.task_id.clone(),
        task_name: event.task_name.clone(),
        linked_tasks: event.linked_tasks.clone(),
        text: event.text.clone(),
    }
}

fn team_plans(
    store: &PlanStore,
    team_id: &str,
    limit: usize,
    cursor: Option<&EventCursor>,
) -> Result<Vec<TeamPlanRead>, PlanError> {
    store.team_mission_control_projection(team_id, limit, cursor)
}

pub(crate) fn project_team_state(
    team: Team,
    plans: Vec<TeamPlanRead>,
    limit: usize,
    cursor: Option<&str>,
) -> Result<TeamMissionControlState, String> {
    let plans: Vec<_> = plans
        .into_iter()
        .filter(|plan| plan.metadata.team_id.as_deref() == Some(team.id.as_str()))
        .collect();
    let active_plan = plans
        .iter()
        .find(|plan| plan.metadata.status == PlanStatus::Active)
        .map(TeamPlanSummary::from_read);
    let archived_plans = plans
        .iter()
        .filter(|plan| plan.metadata.status == PlanStatus::Archived)
        .map(TeamPlanSummary::from_read)
        .collect();

    let decoded_cursor = cursor.map(decode_cursor).transpose()?;
    let mut events = Vec::new();
    for plan in &plans {
        events.extend(
            plan.lifecycle_events
                .iter()
                .map(|(ordinal, event)| lifecycle_event(&plan.metadata, event, *ordinal)),
        );
        events.extend(
            plan.stored_events
                .iter()
                .map(|(ordinal, event)| stored_event(&plan.metadata, event, *ordinal)),
        );
    }
    events.sort_by(compare_events);
    if let Some(cursor) = decoded_cursor.as_ref() {
        events.retain(|event| is_after_cursor(event, cursor));
    }
    let has_more = events.len() > limit;
    events.truncate(limit);
    let next_cursor = has_more.then(|| events.last()).flatten().map(encode_cursor);

    Ok(TeamMissionControlState {
        team_id: team.id.clone(),
        team,
        active_plan,
        archived_plans,
        events,
        next_cursor,
    })
}

pub async fn state(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(team_id): Path<String>,
    Query(query): Query<TeamMissionControlQuery>,
) -> Result<Json<TeamMissionControlState>, (StatusCode, String)> {
    let owner = state.clone();
    let target = team_id.clone();
    let team = tokio::task::spawn_blocking(move || {
        let state = owner;
        let team_id = target;
        let mux = state.svc.mux.read();
        mux.get_team(&team_id)
            .cloned()
            .ok_or((StatusCode::NOT_FOUND, "team not found".to_string()))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })??;
    let store = state.svc.plan_store.clone();
    let limit = query.bounded_limit();
    let cursor = query.cursor;
    let projection = tokio::task::spawn_blocking(move || {
        let decoded_cursor = cursor
            .as_deref()
            .map(decode_cursor)
            .transpose()
            .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
        let plans = team_plans(&store, &team_id, limit, decoded_cursor.as_ref())
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        project_team_state(team, plans, limit, cursor.as_deref())
            .map_err(|error| (StatusCode::BAD_REQUEST, error))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    Ok(Json(projection))
}

#[cfg(test)]
mod tests;

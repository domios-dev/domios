use std::cmp::Ordering;

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use base64::Engine;
use codeterm_types::{plan::Plan, plan::PlanLifecycleEvent, plan::PlanStatus, team::Team};
use serde::{Deserialize, Serialize};

use crate::api::routes::{check_auth, AppState};
use crate::service::plan::PlanStore;

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 100;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TeamMissionControlQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    cursor: Option<String>,
}

impl TeamMissionControlQuery {
    fn bounded_limit(&self) -> usize {
        self.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct TeamMissionControlEvent {
    pub id: String,
    pub plan_id: String,
    pub plan_title: String,
    pub ts: u64,
    pub last_ts: u64,
    pub count: usize,
    pub kind: String,
    pub author: String,
    pub author_tab_id: Option<String>,
    pub task_id: Option<String>,
    pub task_name: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct TeamMissionControlState {
    pub team_id: String,
    pub team: Team,
    pub active_plan: Option<Plan>,
    pub archived_plans: Vec<Plan>,
    pub events: Vec<TeamMissionControlEvent>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct EventCursor {
    ts: u64,
    plan_id: String,
    event_id: String,
}

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
    plan: &Plan,
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
        text: format!("Plan {}", event.kind),
    }
}

fn stored_event(
    plan: &Plan,
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
        text: event.text.clone(),
    }
}

fn team_plans(store: &PlanStore, team_id: &str) -> Vec<Plan> {
    let mut ids: Vec<String> = store
        .list_index()
        .into_iter()
        .filter(|entry| entry.team_id.as_deref() == Some(team_id))
        .map(|entry| entry.plan_id)
        .collect();
    ids.sort();
    ids.dedup();
    let mut plans: Vec<Plan> = ids.into_iter().filter_map(|id| store.get(&id)).collect();
    plans.retain(|plan| plan.team_id.as_deref() == Some(team_id));
    plans.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    plans
}

pub(crate) fn project_team_state(
    team: Team,
    plans: Vec<Plan>,
    limit: usize,
    cursor: Option<&str>,
) -> Result<TeamMissionControlState, String> {
    let plans: Vec<Plan> = plans
        .into_iter()
        .filter(|plan| plan.team_id.as_deref() == Some(team.id.as_str()))
        .collect();
    let active_plan = plans
        .iter()
        .find(|plan| plan.status == PlanStatus::Active)
        .cloned();
    let archived_plans = plans
        .iter()
        .filter(|plan| plan.status == PlanStatus::Archived)
        .cloned()
        .collect();

    let decoded_cursor = cursor.map(decode_cursor).transpose()?;
    let mut events = Vec::new();
    for plan in &plans {
        events.extend(
            plan.lifecycle_events
                .iter()
                .enumerate()
                .map(|(ordinal, event)| lifecycle_event(plan, event, ordinal)),
        );
        let projection =
            codeterm_plan::service_plan::state::mission_control::build_plan_event_log(plan);
        events.extend(
            projection
                .entries
                .iter()
                .enumerate()
                .map(|(ordinal, event)| stored_event(plan, event, ordinal)),
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
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Query(query): Query<TeamMissionControlQuery>,
) -> Result<Json<TeamMissionControlState>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let team = state
        .svc
        .mux
        .read()
        .get_team(&team_id)
        .cloned()
        .ok_or((StatusCode::NOT_FOUND, "team not found".to_string()))?;
    let store = state.svc.plan_store.clone();
    let limit = query.bounded_limit();
    let cursor = query.cursor;
    let projection = tokio::task::spawn_blocking(move || {
        let plans = team_plans(&store, &team_id);
        project_team_state(team, plans, limit, cursor.as_deref())
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    Ok(Json(projection))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn team(id: &str) -> Team {
        Team {
            id: id.into(),
            name: Some("Board".into()),
            lifecycle: codeterm_types::team::TeamLifecycle::Active,
            created_at: 1,
            workspace_id: "workspace".into(),
        }
    }

    fn plan(
        id: &str,
        team_id: &str,
        status: PlanStatus,
        updated_at: u64,
        lifecycle_events: Vec<PlanLifecycleEvent>,
        stored_kind: Option<&str>,
    ) -> Plan {
        let mut plan = Plan::new(
            format!("Plan {id}"),
            "/repo".into(),
            codeterm_types::plan::OrchestratorRef::from_tab("author-tab"),
        );
        plan.id = id.into();
        plan.team_id = Some(team_id.into());
        plan.status = status;
        plan.created_at = updated_at.saturating_sub(10);
        plan.updated_at = updated_at;
        plan.lifecycle_events = lifecycle_events;
        if let Some(kind) = stored_kind {
            plan.strides.push(codeterm_types::plan::Milestone {
                id: format!("milestone-{id}"),
                from: codeterm_types::plan::OrchestratorRef::from_tab("worker-tab"),
                kind: kind.into(),
                text: format!("stored {id}"),
                ts: updated_at,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
        }
        plan
    }

    fn lifecycle(kind: &str, at: u64, id: &str) -> PlanLifecycleEvent {
        PlanLifecycleEvent {
            id: id.into(),
            kind: kind.into(),
            at,
            author: codeterm_types::plan::OrchestratorRef::from_tab("author-tab"),
        }
    }

    #[test]
    fn team_board_state_lifecycle_events_are_recorded() {
        let root = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(root.path());
        let mut plan = Plan::new(
            "Team board".into(),
            root.path().display().to_string(),
            codeterm_types::plan::OrchestratorRef::from_tab("owner-tab"),
        );
        plan.team_id = Some("team-a".into());
        store.save(&mut plan, 10).unwrap();
        store.archive(&plan.id, 20).unwrap();
        store.unarchive(&plan.id, 30).unwrap();

        let events = store.get(&plan.id).unwrap().lifecycle_events;
        assert_eq!(
            events.iter().map(|event| event.kind.as_str()).collect::<Vec<_>>(),
            vec!["opened", "activated", "archived", "activated"]
        );
    }

    #[test]
    fn team_board_state_active_and_lifecycle_projection() {
        let active = plan(
            "active",
            "team-a",
            PlanStatus::Active,
            30,
            vec![
                lifecycle("opened", 10, "open"),
                lifecycle("activated", 20, "activate"),
            ],
            Some("stored"),
        );
        let archived = plan(
            "archived",
            "team-a",
            PlanStatus::Archived,
            40,
            vec![lifecycle("archived", 40, "archive")],
            None,
        );
        let state = project_team_state(team("team-a"), vec![active, archived], 20, None).unwrap();
        assert_eq!(state.team_id, "team-a");
        assert_eq!(
            state.active_plan.as_ref().map(|plan| plan.id.as_str()),
            Some("active")
        );
        assert_eq!(state.archived_plans.len(), 1);
        assert!(state.events.iter().any(|event| event.kind == "stored"));
        assert!(state.events.iter().any(|event| event.kind == "opened"));
        assert!(state.events.iter().any(|event| event.kind == "activated"));
        assert!(state.events.iter().any(|event| event.kind == "archived"));
    }

    #[test]
    fn team_board_state_no_active_plan_keeps_archived_plans() {
        let archived = plan(
            "archived",
            "team-a",
            PlanStatus::Archived,
            10,
            vec![lifecycle("archived", 10, "archive")],
            None,
        );
        let state = project_team_state(team("team-a"), vec![archived], 20, None).unwrap();
        assert!(state.active_plan.is_none());
        assert_eq!(state.archived_plans[0].id, "archived");
    }

    #[test]
    fn team_board_state_excludes_foreign_plans() {
        let local = plan("local", "team-a", PlanStatus::Active, 20, vec![], None);
        let foreign = plan(
            "foreign",
            "team-b",
            PlanStatus::Active,
            30,
            vec![],
            Some("foreign"),
        );
        let foreign_id = foreign.id.clone();
        let state = project_team_state(team("team-a"), vec![local, foreign], 20, None).unwrap();
        assert_eq!(
            state.active_plan.as_ref().map(|plan| plan.id.as_str()),
            Some("local")
        );
        assert!(state.events.iter().all(|event| event.plan_id != foreign_id));
    }

    #[test]
    fn team_board_state_pages_newest_first_and_bounded() {
        let plans = vec![
            plan(
                "a",
                "team-a",
                PlanStatus::Active,
                10,
                vec![],
                Some("stored-a"),
            ),
            plan(
                "b",
                "team-a",
                PlanStatus::Archived,
                20,
                vec![lifecycle("archived", 19, "archive")],
                Some("stored-b"),
            ),
        ];
        let state = project_team_state(team("team-a"), plans, 2, None).unwrap();
        assert_eq!(state.events.len(), 2);
        assert_eq!(state.events[0].ts, 20);
        assert_eq!(state.events[1].ts, 19);
        assert!(state
            .events
            .windows(2)
            .all(|window| compare_events(&window[0], &window[1]) != Ordering::Greater));
        assert!(state.next_cursor.is_some());
    }

    #[test]
    fn team_board_state_cursor_continues_without_duplicates() {
        let plans = vec![
            plan(
                "a",
                "team-a",
                PlanStatus::Active,
                10,
                vec![],
                Some("stored-a"),
            ),
            plan(
                "b",
                "team-a",
                PlanStatus::Archived,
                20,
                vec![lifecycle("archived", 19, "archive")],
                Some("stored-b"),
            ),
        ];
        let first = project_team_state(team("team-a"), plans.clone(), 1, None).unwrap();
        let second = project_team_state(
            team("team-a"),
            plans.clone(),
            1,
            first.next_cursor.as_deref(),
        )
        .unwrap();
        assert_eq!(second.events.len(), 1);
        let first_ids: std::collections::BTreeSet<_> =
            first.events.iter().map(|event| event.id.as_str()).collect();
        assert!(second
            .events
            .iter()
            .all(|event| !first_ids.contains(event.id.as_str())));
    }

    #[test]
    fn team_board_state_cursor_is_deterministic() {
        let plans = vec![plan(
            "a",
            "team-a",
            PlanStatus::Active,
            10,
            vec![lifecycle("opened", 2, "open")],
            Some("stored"),
        )];
        let first = project_team_state(team("team-a"), plans.clone(), 1, None).unwrap();
        let second = project_team_state(team("team-a"), plans.clone(), 1, None).unwrap();
        assert_eq!(first, second);
        let cursor = first.next_cursor.as_deref().unwrap();
        let page_a = project_team_state(team("team-a"), plans.clone(), 1, Some(cursor));
        let page_b = project_team_state(team("team-a"), plans, 1, Some(cursor));
        assert_eq!(page_a, page_b);
    }
}

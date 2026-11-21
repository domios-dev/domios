use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, Method, StatusCode},
    Json,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{self, Write};

use crate::routes::AppState;
use codeterm_service_core::api_support::OptionalJson;
use codeterm_types::workspace_close_preview::{
    WorkspaceCloseLaneState, WorkspaceClosePreview, WorkspaceClosePreviewAttempt,
    WorkspaceClosePreviewMember, WorkspaceClosePreviewRemoteMember, WorkspaceClosePreviewTab,
    WorkspaceClosePreviewTeam, WorkspaceClosePreviewTask, WorkspaceClosePreviewWarning,
    WorkspaceCloseWarningKind,
};

const WORKSPACE_CLOSE_PREVIEW_MAX_ROWS: usize = 4096;
const WORKSPACE_CLOSE_PREVIEW_MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug)]
enum WorkspaceClosePreviewError {
    NotFound,
    Protected,
    TooLarge,
    Unavailable(String),
}

impl WorkspaceClosePreviewError {
    fn into_http(self) -> (StatusCode, String) {
        match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "Workspace not found".into()),
            Self::Protected => (
                StatusCode::CONFLICT,
                "The selected workspace is protected and cannot be closed".into(),
            ),
            Self::TooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "The complete workspace close preview exceeds its supported bound".into(),
            ),
            Self::Unavailable(detail) => (StatusCode::SERVICE_UNAVAILABLE, detail),
        }
    }
}

struct BoundedSizeWriter {
    written: usize,
    exceeded: bool,
}

impl Write for BoundedSizeWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self.written.saturating_add(bytes.len());
        if next > WORKSPACE_CLOSE_PREVIEW_MAX_BYTES {
            self.exceeded = true;
            return Err(io::Error::new(io::ErrorKind::WriteZero, "preview byte bound exceeded"));
        }
        self.written = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub async fn list_workspaces(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let result = codeterm_service_core::dispatch::dispatch_blocking(
        codeterm_service_core::dispatch::Command::WorkspaceList,
        state.svc.clone(),
    )
    .await
    .map_err(|error| {
        if error == codeterm_service_core::startup::STORAGE_UNAVAILABLE_MESSAGE {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                codeterm_service_core::startup::STORAGE_UNAVAILABLE_MESSAGE.to_string(),
            )
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to list workspaces".to_string(),
            )
        }
    })?;
    let value = tokio::task::spawn_blocking(move || result.into_json())
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(Json(value))
}

#[derive(Deserialize)]
pub struct CreateWorkspaceBody {
    name: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    activate: Option<bool>,
    #[serde(default)]
    provision: bool,
}

pub(crate) type CreateTabBody = crate::tab_routes::CreateShellTabBody;

pub async fn create_workspace(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    OptionalJson(body): OptionalJson<CreateWorkspaceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let body = body.ok_or((StatusCode::BAD_REQUEST, "name is required".to_string()))?;
    let cmd = codeterm_service_core::dispatch::Command::WorkspaceCreate {
        name: body.name,
        cwd: body.cwd,
        activate: body.activate,
        provision: body.provision,
    };
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        codeterm_service_core::dispatch::dispatch(&cmd, svc.as_ref())
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| {
        if error.starts_with(codeterm_service_core::service::billing::FREE_LIMIT_REACHED) {
            return (StatusCode::FORBIDDEN, error);
        }
        let status = if error == codeterm_service_core::dispatch::WORKSPACE_PROVISION_CWD_CONFLICT
            || error.starts_with("workspace name")
        {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, format!("Failed to create workspace: {error}"))
    })?;
    Ok(Json(result.into_json()))
}

pub async fn focus_workspace(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let svc = state.svc.clone();
    tokio::task::spawn_blocking(move || svc.request_workspace_focus(&id))
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| (StatusCode::NOT_FOUND, error))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

#[derive(Deserialize)]
pub struct SetWorkspaceCwdBody {
    cwd: String,
}

pub async fn set_workspace_cwd(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SetWorkspaceCwdBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let svc = state.svc.clone();
    let cmd = codeterm_service_core::dispatch::Command::WorkspaceSetCwd {
        workspace_id: id,
        cwd: body.cwd,
    };
    let result = tokio::task::spawn_blocking(move || {
        codeterm_service_core::dispatch::dispatch(&cmd, svc.as_ref())
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::UNPROCESSABLE_ENTITY, error))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub struct RenameWorkspaceBody {
    name: String,
}

fn map_rename_error(e: String) -> (StatusCode, String) {
    if e.contains("not found") {
        (StatusCode::NOT_FOUND, e)
    } else if e.starts_with("workspace name") {
        (StatusCode::BAD_REQUEST, e)
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, e)
    }
}

#[derive(Deserialize, Default)]
pub struct CloseWorkspaceBody {
    #[serde(default)]
    force: bool,
    #[serde(default)]
    confirm: Option<String>,
}

fn has_human_workspace_confirmation(body: &CloseWorkspaceBody, workspace_id: &str) -> bool {
    body.force && body.confirm.as_deref() == Some(workspace_id)
}

#[derive(Debug, PartialEq, Eq)]
enum WorkspaceCloseCaller {
    AgentTab(String),
    HumanConfirmedHeaderless,
    Unattributed,
}

fn workspace_close_caller(
    headers: &crate::auth::ActorHeaders,
    body: &CloseWorkspaceBody,
    workspace_id: &str,
) -> WorkspaceCloseCaller {
    if let Some(tab_id) =
        crate::tab_routes::caller_tab_header(headers).filter(|tab_id| !tab_id.trim().is_empty())
    {
        WorkspaceCloseCaller::AgentTab(tab_id)
    } else if has_human_workspace_confirmation(body, workspace_id) {
        WorkspaceCloseCaller::HumanConfirmedHeaderless
    } else {
        WorkspaceCloseCaller::Unattributed
    }
}

pub async fn rename_workspace(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<RenameWorkspaceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let cmd = codeterm_service_core::dispatch::Command::WorkspaceRename {
        workspace_id: id,
        name: body.name,
    };
    let result = codeterm_service_core::dispatch::dispatch_blocking(cmd, state.svc.clone())
        .await
        .map_err(map_rename_error)?;
    Ok(Json(result.into_json()))
}

pub async fn close_workspace(
    AxumState(state): AxumState<AppState>,
    headers: crate::auth::ActorHeaders,
    Path(id): Path<String>,
    OptionalJson(body): OptionalJson<CloseWorkspaceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let body = body.unwrap_or_default();
    let result = match workspace_close_caller(&headers, &body, &id) {
        WorkspaceCloseCaller::AgentTab(caller_tab_id) => {
            state
                .svc
                .close_workspace_guarded_async(Some(&caller_tab_id), &id, body.force)
                .await
        }
        WorkspaceCloseCaller::HumanConfirmedHeaderless => {
            state
                .svc
                .close_workspace_as_confirmed_headerless_human(&id, body.force)
                .await
        }
        WorkspaceCloseCaller::Unattributed => {
            state
                .svc
                .close_workspace_guarded_async(None, &id, body.force)
                .await
        }
    };
    result.map_err(crate::tab_routes::map_close_error)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn close_preview(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<WorkspaceClosePreview>, (StatusCode, String)> {
    let (identity, origin) = crate::auth::authenticated_request(&state, &headers)?;
    if !origin.owner_trusted() {
        return Err((
            StatusCode::FORBIDDEN,
            "Only owner-origin requests may inspect workspace close previews".into(),
        ));
    }
    if headers
        .get("x-codeterm-tab-id")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|tab_id| !tab_id.trim().is_empty())
    {
        return Err((
            StatusCode::FORBIDDEN,
            "An agent tab cannot inspect workspace close previews".into(),
        ));
    }
    match identity {
        crate::auth::CallerIdentity::Local => {}
        crate::auth::CallerIdentity::Peer { access_level, .. } => {
            let path = format!("/api/workspaces/{id}/close-preview");
            if !codeterm_service_core::api_support::access_policy::check_access(
                &access_level,
                &Method::GET,
                &path,
            ) {
                return Err((
                    StatusCode::FORBIDDEN,
                    "Peer lacks workspace close authority".into(),
                ));
            }
        }
        crate::auth::CallerIdentity::Tab(_)
        | crate::auth::CallerIdentity::ServicePush { .. }
        | crate::auth::CallerIdentity::Relayed => {
            return Err((
                StatusCode::FORBIDDEN,
                "Caller lacks authority to inspect workspace close previews".into(),
            ));
        }
    }
    close_preview_for_mesh_peer(&state, &id).await.map(Json)
}

pub(crate) async fn close_preview_for_mesh_peer(
    state: &AppState,
    workspace_id: &str,
) -> Result<WorkspaceClosePreview, (StatusCode, String)> {
    let svc = state.svc.clone();
    let cron = state.cron.clone();
    let installation_id = state.mesh.lock().ident.identity.instance_id.clone();
    let workspace_id = workspace_id.to_string();
    tokio::task::spawn_blocking(move || {
        build_workspace_close_preview(&svc, &cron, &installation_id, &workspace_id)
    })
    .await
    .map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Workspace preview worker failed: {error}"),
        )
    })?
    .map_err(WorkspaceClosePreviewError::into_http)
}

struct PreviewTab {
    id: String,
    title: String,
    agent: bool,
    team_id: Option<String>,
    role: Option<String>,
    role_profile: Option<String>,
    remote: Option<WorkspaceClosePreviewRemoteMember>,
}

struct PreviewBudget {
    rows: usize,
    warnings: Vec<WorkspaceClosePreviewWarning>,
}

impl PreviewBudget {
    fn new() -> Self {
        Self { rows: 0, warnings: Vec::new() }
    }

    fn remaining(&self) -> usize {
        WORKSPACE_CLOSE_PREVIEW_MAX_ROWS.saturating_sub(self.rows)
    }

    fn add(&mut self, count: usize) -> Result<(), WorkspaceClosePreviewError> {
        self.rows = self.rows.saturating_add(count);
        if self.rows > WORKSPACE_CLOSE_PREVIEW_MAX_ROWS {
            return Err(WorkspaceClosePreviewError::TooLarge);
        }
        Ok(())
    }

    fn warn(
        &mut self,
        kind: WorkspaceCloseWarningKind,
        team_id: Option<String>,
        plan_id: Option<String>,
        task_id: Option<String>,
        detail: String,
    ) -> Result<(), WorkspaceClosePreviewError> {
        self.add(1)?;
        self.warnings.push(WorkspaceClosePreviewWarning { kind, team_id, plan_id, task_id, detail });
        Ok(())
    }
}

fn empty_preview_team(id: String, name: String, remote: bool) -> WorkspaceClosePreviewTeam {
    WorkspaceClosePreviewTeam {
        id,
        name,
        remote,
        members: Vec::new(),
        crons: Vec::new(),
        blueprints: Vec::new(),
        tasks: Vec::new(),
        open_attempts: Vec::new(),
        lanes: Vec::new(),
    }
}

fn build_workspace_close_preview(
    svc: &codeterm_service_core::service::ServiceContext,
    cron: &codeterm_service_core::CronState,
    installation_id: &str,
    workspace_id: &str,
) -> Result<WorkspaceClosePreview, WorkspaceClosePreviewError> {
    use codeterm_sentinel_host::AutomationGraphExt;

    svc.ensure_storage_available().map_err(WorkspaceClosePreviewError::Unavailable)?;
    let mut budget = PreviewBudget::new();
    let (workspace_name, tabs, mut teams, missing_team_metadata) = {
        let mux = svc.mux.read();
        let workspace = mux.get_workspace(workspace_id).ok_or(WorkspaceClosePreviewError::NotFound)?;
        if workspace.protected {
            return Err(WorkspaceClosePreviewError::Protected);
        }
        if workspace.tabs.len() > WORKSPACE_CLOSE_PREVIEW_MAX_ROWS {
            return Err(WorkspaceClosePreviewError::TooLarge);
        }
        let mut pending = workspace.tabs.clone();
        let mut visited = HashSet::new();
        let mut tabs = Vec::new();
        while let Some(tab_id) = pending.pop() {
            if !visited.insert(tab_id.clone()) {
                continue;
            }
            if visited.len() > WORKSPACE_CLOSE_PREVIEW_MAX_ROWS {
                return Err(WorkspaceClosePreviewError::TooLarge);
            }
            let tab = mux.get_tab(&tab_id).ok_or_else(|| {
                WorkspaceClosePreviewError::Unavailable(format!(
                    "Workspace {workspace_id} references missing tab {tab_id}"
                ))
            })?;
            let remote = tab.ownership.as_ref().map(|ownership| WorkspaceClosePreviewRemoteMember {
                tab_id: tab.id.clone(),
                title: tab.title.clone(),
                peer_id: ownership.peer_id.clone(),
                peer_name: ownership.peer_name.clone(),
                team_id: ownership.team.as_ref().map(|team| team.id.clone()),
            });
            tabs.push(PreviewTab {
                id: tab.id.clone(),
                title: tab.title.clone(),
                agent: tab.agent_type.is_some(),
                team_id: tab.team_id.clone(),
                role: tab.role.as_ref().map(|role| format!("{role:?}")),
                role_profile: tab.role_profile.clone(),
                remote,
            });
            let child_limit = WORKSPACE_CLOSE_PREVIEW_MAX_ROWS.saturating_sub(visited.len());
            let children = mux.get_children_ids_bounded(&tab_id, child_limit);
            if children.len() > child_limit {
                return Err(WorkspaceClosePreviewError::TooLarge);
            }
            pending.extend(children);
        }

        let mut team_ids = BTreeSet::new();
        for tab in &tabs {
            if let Some(team_id) = tab.team_id.as_ref() {
                team_ids.insert(team_id.clone());
            }
        }
        for team in mux.teams_for_workspace(workspace_id).take(WORKSPACE_CLOSE_PREVIEW_MAX_ROWS + 1) {
            team_ids.insert(team.id.clone());
        }
        if team_ids.len() > WORKSPACE_CLOSE_PREVIEW_MAX_ROWS {
            return Err(WorkspaceClosePreviewError::TooLarge);
        }
        let (mut local_anchors, mut remote_owners) = (HashSet::new(), HashSet::new());
        if !team_ids.is_empty() {
            for tab in mux.all_tabs() {
                if tab.anchors_team() {
                    if let Some(id) = tab.team_id.as_ref().filter(|id| team_ids.contains(*id)) {
                        local_anchors.insert(id.clone());
                    }
                }
                if let Some((ownership, remote_team)) =
                    tab.ownership.as_ref().and_then(|o| o.team.as_ref().map(|t| (o, t)))
                {
                    let qualified = format!("mesh:{}/{}", ownership.peer_id, remote_team.id);
                    if team_ids.contains(&remote_team.id) {
                        remote_owners.insert(remote_team.id.clone());
                    } else if team_ids.contains(&qualified) {
                        remote_owners.insert(qualified);
                    }
                }
            }
        }
        let mut teams = BTreeMap::new();
        let mut missing_team_metadata = HashSet::new();
        for team_id in team_ids {
            let record = mux.get_team(&team_id);
            let remote = team_id.starts_with("mesh:")
                || (!local_anchors.contains(&team_id) && remote_owners.contains(&team_id));
            if record.is_none() {
                missing_team_metadata.insert(team_id.clone());
            }
            let name = record
                .and_then(|team| team.name.as_deref())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(if record.is_some() { "Team" } else { "Missing team metadata" })
                .to_string();
            teams.insert(team_id.clone(), empty_preview_team(team_id, name, remote));
        }
        for tab in &tabs {
            if let Some(team) = tab.team_id.as_ref().and_then(|id| teams.get_mut(id)) {
                team.members.push(WorkspaceClosePreviewMember {
                    tab_id: tab.id.clone(),
                    title: tab.title.clone(),
                    role: tab.role.clone(),
                    role_profile: tab.role_profile.clone(),
                    remote: tab.remote.is_some(),
                });
            }
        }
        (workspace.name.clone(), tabs, teams, missing_team_metadata)
    };
    budget.add(teams.len())?;
    budget.add(teams.values().map(|team| team.members.len()).sum::<usize>())?;

    let remote_members = tabs.iter().filter_map(|tab| tab.remote.clone()).collect::<Vec<_>>();
    budget.add(remote_members.len())?;
    for remote in &remote_members {
        budget.warn(
            WorkspaceCloseWarningKind::RemoteMember,
            remote.team_id.clone(),
            None,
            None,
            format!("{} is remotely owned by {} ({})", remote.title, remote.peer_name, remote.peer_id),
        )?;
    }
    let agent_tabs = tabs
        .iter()
        .filter(|tab| tab.agent && tab.team_id.is_none())
        .map(|tab| WorkspaceClosePreviewTab { tab_id: tab.id.clone(), title: tab.title.clone() })
        .collect::<Vec<_>>();
    budget.add(agent_tabs.len())?;

    let mut workspace_crons = Vec::new();
    {
        let store = cron.lock();
        if let Some(reason) = store.unavailable_reason() {
            return Err(WorkspaceClosePreviewError::Unavailable(format!(
                "Workspace cron inventory is unavailable: {reason}"
            )));
        }
        let remaining = budget.remaining();
        let crons = store
            .list()
            .iter()
            .filter(|job| job.workspace_id == workspace_id)
            .take(remaining.saturating_add(1))
            .map(|job| (job.team_id.clone(), job.name.clone()))
            .collect::<Vec<_>>();
        if crons.len() > remaining {
            return Err(WorkspaceClosePreviewError::TooLarge);
        }
        budget.add(crons.len())?;
        for (team_id, name) in crons {
            if let Some(team_id) = team_id {
                if let Some(team) = teams.get_mut(&team_id) {
                    team.crons.push(name);
                } else {
                    workspace_crons.push(format!("{name} (Team {team_id})"));
                    budget.warn(
                        WorkspaceCloseWarningKind::OrphanTask,
                        Some(team_id),
                        None,
                        None,
                        format!("Cron {name} refers to a Team absent from the selected workspace."),
                    )?;
                }
            } else {
                workspace_crons.push(name);
            }
        }
    }

    let graph = svc
        .automation_graph_workspace_snapshot(workspace_id)
        .map_err(WorkspaceClosePreviewError::Unavailable)?;
    if graph.truncated {
        return Err(WorkspaceClosePreviewError::Unavailable(
            "Workspace blueprint ownership inventory is truncated".into(),
        ));
    }
    let tab_teams = tabs.iter().map(|t| (t.id.as_str(), t.team_id.as_deref())).collect::<BTreeMap<_, _>>();
    let mut blueprints = Vec::new();
    for blueprint in graph.blueprints.iter().filter(|bp| format!("{:?}", bp.state) == "Running") {
        let name = blueprint.name.as_deref().unwrap_or(&blueprint.id).to_string();
        match blueprint.owner.kind.as_str() {
            "team" => {
                let team_id = blueprint.owner.team_id.as_deref().filter(|id| !id.is_empty()).ok_or_else(|| {
                    WorkspaceClosePreviewError::Unavailable(format!(
                        "Running Blueprint {} has no Team owner identity", blueprint.id
                    ))
                })?;
                budget.add(1)?;
                if let Some(team) = teams.get_mut(team_id) {
                    team.blueprints.push(name);
                } else {
                    blueprints.push(format!("{name} (missing Team {team_id})"));
                    budget.warn(
                        WorkspaceCloseWarningKind::OrphanTask,
                        Some(team_id.to_string()),
                        None,
                        None,
                        format!("Running Blueprint {} has no Team in the selected workspace.", blueprint.id),
                    )?;
                }
            }
            "tab" => {
                let tab_id = blueprint.owner.tab_id.as_deref().filter(|id| !id.is_empty()).ok_or_else(|| {
                    WorkspaceClosePreviewError::Unavailable(format!(
                        "Running Blueprint {} has no tab owner identity", blueprint.id
                    ))
                })?;
                budget.add(1)?;
                match tab_teams.get(tab_id).copied() {
                    Some(Some(team_id)) => {
                        if let Some(team) = teams.get_mut(team_id) {
                            team.blueprints.push(name);
                        } else {
                            blueprints.push(name);
                        }
                    }
                    Some(None) => blueprints.push(name),
                    None => {
                        budget.warn(
                            WorkspaceCloseWarningKind::OrphanTask,
                            None,
                            None,
                            Some(tab_id.to_string()),
                            format!("Running Blueprint {name} refers to tab {tab_id} outside the selected workspace."),
                        )?;
                        blueprints.push(name);
                    }
                }
            }
            other => {
                return Err(WorkspaceClosePreviewError::Unavailable(format!(
                    "Running Blueprint {} has unsupported owner kind {other}", blueprint.id
                )))
            }
        }
    }

    for (team_id, team) in &mut teams {
        if team.remote {
            budget.warn(
                WorkspaceCloseWarningKind::RemoteTeam,
                Some(team_id.clone()),
                None,
                None,
                format!("Team {} has remote membership; its owning peer remains authoritative.", team.name),
            )?;
            continue;
        }
        if missing_team_metadata.contains(team_id) {
            budget.warn(
                WorkspaceCloseWarningKind::OrphanTask,
                Some(team_id.clone()),
                None,
                None,
                "Workspace tab references a Team with missing metadata".into(),
            )?;
        }
        let members = team.members.iter().map(|m| m.tab_id.clone()).collect::<Vec<_>>();
        let plans = svc
            .plan_store
            .team_close_projection_bounded(team_id, &members, budget.remaining())
            .map_err(|error| match error {
                codeterm_service_core::service::plan::TeamCloseProjectionError::TooLarge => {
                    WorkspaceClosePreviewError::TooLarge
                }
                codeterm_service_core::service::plan::TeamCloseProjectionError::Store(error) => {
                    WorkspaceClosePreviewError::Unavailable(format!(
                        "Team plan close projection is unavailable: {error}"
                    ))
                }
            })?;
        let plan_rows = plans
            .iter()
            .map(|p| {
                p.tasks.len().saturating_add(
                    p.tasks
                        .iter()
                        .map(|t| t.open_attempts.len())
                        .sum::<usize>(),
                )
            })
            .sum::<usize>();
        budget.add(plan_rows)?;
        let lanes = codeterm_orchestration::team::close::workspace_close_lane_preview(
            &plans,
            team_id,
            &members,
            budget.remaining(),
        )
        .map_err(|error| match error {
            codeterm_orchestration::team::close::WorkspaceCloseLanePreviewError::TooLarge => {
                WorkspaceClosePreviewError::TooLarge
            }
            codeterm_orchestration::team::close::WorkspaceCloseLanePreviewError::Unavailable(d) => {
                WorkspaceClosePreviewError::Unavailable(format!("Team lane safety inventory is unavailable: {d}"))
            }
        })?;
        budget.add(lanes.len())?;
        for lane in &lanes {
            let kind = match lane.state {
                WorkspaceCloseLaneState::Safe => None,
                WorkspaceCloseLaneState::Dirty => Some(WorkspaceCloseWarningKind::DirtyLane),
                WorkspaceCloseLaneState::Unmerged => Some(WorkspaceCloseWarningKind::UnmergedLane),
                WorkspaceCloseLaneState::Orphan => Some(WorkspaceCloseWarningKind::OrphanBranch),
                WorkspaceCloseLaneState::Held => Some(WorkspaceCloseWarningKind::HeldLane),
                WorkspaceCloseLaneState::Remote => Some(WorkspaceCloseWarningKind::RemoteAttempt),
                WorkspaceCloseLaneState::Unavailable => Some(WorkspaceCloseWarningKind::LaneUnavailable),
            };
            if let Some(kind) = kind {
                budget.warn(
                    kind,
                    Some(team_id.clone()),
                    Some(lane.plan_id.clone()),
                    Some(lane.task_id.clone()),
                    lane.detail.clone().unwrap_or_else(|| {
                        format!("Lane {} is {:?}", lane.branch.as_deref().or(lane.path.as_deref()).unwrap_or("unknown"), lane.state)
                    }),
                )?;
            }
        }
        for plan in &plans {
            let plan_title = if plan.title.trim().is_empty() { plan.id.clone() } else { plan.title.clone() };
            for task in &plan.tasks {
                let status = serde_json::to_value(&task.status)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .ok_or_else(|| WorkspaceClosePreviewError::Unavailable(format!("Task {}/{} has an unknown status", plan.id, task.id)))?;
                let orphan = plan.team_id.as_deref() == Some(team_id.as_str())
                    && status == "in_progress"
                    && task.member_executor.is_none();
                if orphan {
                    budget.warn(
                        WorkspaceCloseWarningKind::OrphanTask,
                        Some(team_id.clone()),
                        Some(plan.id.clone()),
                        Some(task.id.clone()),
                        format!("In-progress task {} has no live Team member owner", task.name),
                    )?;
                }
                if task.has_remote_attempt {
                    budget.warn(
                        WorkspaceCloseWarningKind::RemoteAttempt,
                        Some(team_id.clone()),
                        Some(plan.id.clone()),
                        Some(task.id.clone()),
                        format!("Task {} has remote attempt metadata", task.name),
                    )?;
                }
                team.tasks.push(WorkspaceClosePreviewTask {
                    plan_id: plan.id.clone(),
                    plan_title: plan_title.clone(),
                    task_id: task.id.clone(),
                    task_name: task.name.clone(),
                    status,
                    orphan,
                    remote_attempt: task.has_remote_attempt,
                });
                for attempt in &task.open_attempts {
                    if attempt.remote {
                        budget.warn(
                            WorkspaceCloseWarningKind::RemoteAttempt,
                            Some(team_id.clone()),
                            Some(plan.id.clone()),
                            Some(task.id.clone()),
                            format!("Open attempt {} is owned by a remote executor", attempt.id),
                        )?;
                    }
                    team.open_attempts.push(WorkspaceClosePreviewAttempt {
                        plan_id: plan.id.clone(),
                        task_id: task.id.clone(),
                        attempt_id: attempt.id.clone(),
                        executor_tab_id: attempt.executor_tab_id.clone(),
                        remote: attempt.remote,
                    });
                }
            }
        }
        team.lanes = lanes;
    }

    let preview = WorkspaceClosePreview {
        installation_id: installation_id.to_string(),
        workspace_id: workspace_id.to_string(),
        workspace_name,
        protected: false,
        agent_tabs,
        remote_members,
        crons: workspace_crons,
        blueprints,
        teams: teams.into_values().collect(),
        warnings: budget.warnings,
    };
    let mut writer = BoundedSizeWriter { written: 0, exceeded: false };
    if let Err(error) = serde_json::to_writer(&mut writer, &preview) {
        if writer.exceeded {
            return Err(WorkspaceClosePreviewError::TooLarge);
        }
        return Err(WorkspaceClosePreviewError::Unavailable(format!(
            "Workspace close preview could not be serialized: {error}"
        )));
    }
    Ok(preview)
}

pub async fn list_recently_closed(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let cmd =
        codeterm_service_core::dispatch::Command::WorkspaceRecentlyClosed { workspace_id: id };
    let result = codeterm_service_core::dispatch::dispatch_blocking(cmd, state.svc.clone())
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to list recently closed",
            )
        })?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize, Default)]
pub struct ReopenClosedBody {
    #[serde(default)]
    index: Option<usize>,
}

pub async fn reopen_closed_tab(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(id): Path<String>,
    OptionalJson(body): OptionalJson<ReopenClosedBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let index = body.and_then(|b| b.index);
    let cmd = codeterm_service_core::dispatch::Command::WorkspaceReopenClosedTab {
        workspace_id: id,
        index,
    };
    let result = codeterm_service_core::dispatch::dispatch_blocking(cmd, state.svc.clone())
        .await
        .map_err(|error| crate::plan_quota_status::route_error(StatusCode::NOT_FOUND, error))?;
    Ok(Json(result.into_json()))
}

pub async fn reopen_closed_group(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(group_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let cmd = codeterm_service_core::dispatch::Command::WorkspaceReopenClosedGroup { group_id };
    let service = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        codeterm_service_core::dispatch::dispatch(&cmd, service.as_ref())
    })
    .await
    .map_err(|error| {
        crate::plan_quota_status::route_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
    })?
    .map_err(|error| crate::plan_quota_status::route_error(StatusCode::BAD_REQUEST, error))?;
    Ok(Json(result.into_json()))
}

#[derive(Deserialize)]
pub struct MoveTabBody {
    to_workspace_id: String,
}

pub async fn move_tab(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((workspace_id, tab_id)): Path<(String, String)>,
    Json(body): Json<MoveTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .move_tab(&tab_id, &workspace_id, &body.to_workspace_id)
            .map_err(|message| {
                let status = if message.starts_with("Workspace not found:") {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::BAD_REQUEST
                };
                (status, message)
            })?;
        Ok(Json(serde_json::json!({"ok": true})))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?
}

#[derive(Deserialize)]
pub struct ReorderTabsBody {
    ordered_tab_ids: Vec<String>,
}

fn reorder_error(
    status: StatusCode,
    code: &str,
    message: String,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({"ok": false, "error": {"code": code, "message": message}})),
    )
}

pub async fn reorder_tabs(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(body): Json<ReorderTabsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .reorder_tab(&workspace_id, &body.ordered_tab_ids)
            .map_err(|message| reorder_error(StatusCode::BAD_REQUEST, "reorder_failed", message))?;
        Ok(Json(serde_json::json!({"ok": true})))
    })
    .await
    .map_err(|error| {
        reorder_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "reorder_failed",
            error.to_string(),
        )
    })?
}

pub async fn reorder_children(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(parent_tab_id): Path<String>,
    Json(body): Json<ReorderTabsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .reorder_child(&parent_tab_id, &body.ordered_tab_ids)
            .map_err(|message| reorder_error(StatusCode::BAD_REQUEST, "reorder_failed", message))?;
        Ok(Json(serde_json::json!({"ok": true})))
    })
    .await
    .map_err(|error| {
        reorder_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "reorder_failed",
            error.to_string(),
        )
    })?
}

pub async fn reorder_display(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(body): Json<ReorderTabsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .reorder_display(&workspace_id, &body.ordered_tab_ids)
            .map_err(|message| reorder_error(StatusCode::BAD_REQUEST, "reorder_failed", message))?;
        Ok(Json(serde_json::json!({"ok": true})))
    })
    .await
    .map_err(|error| {
        reorder_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "reorder_failed",
            error.to_string(),
        )
    })?
}

#[derive(Deserialize)]
pub struct ReorderWorkspacesBody {
    ordered_workspace_ids: Vec<String>,
}

pub async fn reorder_workspaces(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Json(body): Json<ReorderWorkspacesBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    tokio::task::spawn_blocking(move || {
        state
            .svc
            .reorder_workspace(&body.ordered_workspace_ids)
            .map_err(|_| (StatusCode::BAD_REQUEST, "Failed to reorder workspaces"))?;
        Ok(Json(serde_json::json!({"ok": true})))
    })
    .await
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "blocking task failed"))?
}

#[derive(Deserialize)]
pub struct RenameTabBody {
    title: String,
}

pub async fn rename_tab(
    AxumState(state): AxumState<AppState>,
    _headers: HeaderMap,
    Path((_workspace_id, tab_id)): Path<(String, String)>,
    Json(body): Json<RenameTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        codeterm_service_core::service::tab::validate_tab_title(&body.title)
            .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
        state
            .svc
            .rename_tab(&tab_id, &body.title)
            .map_err(map_rename_error)?;
        Ok(Json(serde_json::json!({"ok": true})))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "blocking task failed".to_string(),
        )
    })?
}

pub async fn create_tab(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    OptionalJson(body): OptionalJson<CreateTabBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut body = body.unwrap_or_default();
    body.workspace_id = Some(workspace_id);
    let response =
        crate::tab_routes::create_shell_tab(AxumState(state.clone()), headers, Json(body)).await?;
    let mut value = response.0;
    let tab_id = value["tab_id"].as_str().map(str::to_owned);
    if let Some(tab_id) = tab_id {
        let svc = state.svc.clone();
        value["title"] = serde_json::Value::String(
            tokio::task::spawn_blocking(move || svc.mux.read().display_label_for_tab(&tab_id))
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "blocking task failed".to_string(),
                    )
                })?,
        );
    }
    Ok(Json(value))
}

#[cfg(test)]
mod close_tests;

#[cfg(test)]
mod tests;

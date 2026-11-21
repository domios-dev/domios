use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceCloseLaneState {
    Safe,
    Dirty,
    Unmerged,
    Orphan,
    Remote,
    Unavailable,
    Held,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceCloseWarningKind {
    RemoteMember,
    RemoteTeam,
    RemoteAttempt,
    DirtyLane,
    UnmergedLane,
    HeldLane,
    OrphanTask,
    OrphanBranch,
    LaneUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceClosePreview {
    pub installation_id: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub protected: bool,
    pub agent_tabs: Vec<WorkspaceClosePreviewTab>,
    pub remote_members: Vec<WorkspaceClosePreviewRemoteMember>,
    pub crons: Vec<String>,
    pub blueprints: Vec<String>,
    pub teams: Vec<WorkspaceClosePreviewTeam>,
    pub warnings: Vec<WorkspaceClosePreviewWarning>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceClosePreviewTab {
    pub tab_id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceClosePreviewRemoteMember {
    pub tab_id: String,
    pub title: String,
    pub peer_id: String,
    pub peer_name: String,
    pub team_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceClosePreviewTeam {
    pub id: String,
    pub name: String,
    pub remote: bool,
    pub members: Vec<WorkspaceClosePreviewMember>,
    pub crons: Vec<String>,
    pub blueprints: Vec<String>,
    pub tasks: Vec<WorkspaceClosePreviewTask>,
    pub open_attempts: Vec<WorkspaceClosePreviewAttempt>,
    pub lanes: Vec<WorkspaceClosePreviewLane>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceClosePreviewMember {
    pub tab_id: String,
    pub title: String,
    pub role: Option<String>,
    pub role_profile: Option<String>,
    pub remote: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceClosePreviewTask {
    pub plan_id: String,
    pub plan_title: String,
    pub task_id: String,
    pub task_name: String,
    pub status: String,
    pub orphan: bool,
    pub remote_attempt: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceClosePreviewAttempt {
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub executor_tab_id: Option<String>,
    pub remote: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceClosePreviewLane {
    pub plan_id: String,
    pub task_id: String,
    pub path: Option<String>,
    pub branch: Option<String>,
    pub state: WorkspaceCloseLaneState,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceClosePreviewWarning {
    pub kind: WorkspaceCloseWarningKind,
    pub team_id: Option<String>,
    pub plan_id: Option<String>,
    pub task_id: Option<String>,
    pub detail: String,
}

use axum::http::{Method, Uri};

use crate::mesh::types::{AccessLevel, Permission};

const SUPPORTER_TOKEN_PATH: &str = "/api/supporter/token";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerOperation {
    Announce,
    Sync,
    Inventory,
    Rail,
    ReadTabs,
    ReadMux,
    ReadOutput,
    ReadPreview,
    ReadCron,
    ReadCronRun,
    ReadService,
    ReadPlan,
    ReadHost,
    SendInput,
    WriteTab,
    WriteOwnedTab,
    SpawnAgent,
    ManageWorkspace,
    ReadWorkspaceClosePreview,
    WorkspacePath,
    ManageCron,
    FullAccess,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerResource<'a> {
    Host,
    TabCollection,
    Tab(&'a str),
    OutputCollection,
    Cron(&'a str),
    CronRun { cron_id: &'a str, run_id: &'a str },
    Service(&'a str),
    Workspace(&'a str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerScope<'a> {
    pub operation: PeerOperation,
    pub resource: PeerResource<'a>,
}

impl<'a> PeerScope<'a> {
    pub const fn new(operation: PeerOperation, resource: PeerResource<'a>) -> Self {
        Self {
            operation,
            resource,
        }
    }

    pub fn allowed(self, access: &AccessLevel) -> bool {
        use PeerOperation::*;
        match access {
            AccessLevel::Full => true,
            AccessLevel::ReadOnly => matches!(
                self.operation,
                Sync | Inventory
                    | Rail
                    | ReadTabs
                    | ReadMux
                    | ReadOutput
                    | ReadPreview
                    | ReadCron
                    | ReadCronRun
                    | ReadService
                    | ReadPlan
                    | ReadHost
            ),
            AccessLevel::AgentIsolated => match self.operation {
                Sync | Rail | SpawnAgent => true,
                ReadTabs => matches!(
                    self.resource,
                    PeerResource::TabCollection | PeerResource::Tab(_)
                ),
                ReadOutput | ReadPreview | SendInput | WriteOwnedTab => {
                    matches!(self.resource, PeerResource::Tab(_))
                }
                _ => false,
            },
            AccessLevel::Custom(perms) => {
                perms.contains(&Permission::Admin)
                    || match self.operation {
                        Rail => {
                            perms.contains(&Permission::ReadTabs)
                                || perms.contains(&Permission::ReadOutput)
                        }
                        ReadTabs | ReadMux => perms.contains(&Permission::ReadTabs),
                        ReadOutput | ReadPreview => perms.contains(&Permission::ReadOutput),
                        Inventory => perms.contains(&Permission::ManageCrons),
                        ReadCron | ReadCronRun | ManageCron => {
                            perms.contains(&Permission::ManageCrons)
                        }
                        SendInput => {
                            perms.contains(&Permission::SendInput)
                                || perms.contains(&Permission::WriteTabs)
                        }
                        WriteTab | WriteOwnedTab => perms.contains(&Permission::WriteTabs),
                        SpawnAgent => perms.contains(&Permission::SpawnAgents),
                        ManageWorkspace => perms.contains(&Permission::ManageWorkspaces),
                        ReadWorkspaceClosePreview => perms.contains(&Permission::ManageWorkspaces),
                        WorkspacePath => {
                            perms.contains(&Permission::ManageWorkspaces)
                                || perms.contains(&Permission::SpawnAgents)
                        }
                        Announce | Sync | ReadService | ReadPlan | ReadHost | FullAccess => false,
                    }
            }
        }
    }

    pub fn allowed_for_peer(
        self,
        access: &AccessLevel,
        peer_id: &str,
        tab_owner: impl FnOnce(&str) -> Option<String>,
    ) -> bool {
        self.allowed_with_owner(access, |id| tab_owner(id).as_deref() == Some(peer_id))
    }

    pub fn allowed_with_owner(
        self,
        access: &AccessLevel,
        owns_tab: impl FnOnce(&str) -> bool,
    ) -> bool {
        self.allowed(access)
            && (*access != AccessLevel::AgentIsolated
                || match self.resource {
                    PeerResource::Tab(id) => owns_tab(id),
                    _ => true,
                })
    }
}

pub fn request_path(path: &str) -> &str {
    path.split(['?', '#']).next().unwrap_or(path)
}

pub fn is_preview_read(method: &Method, path: &str) -> bool {
    *method == Method::POST
        && scoped_path(request_path(path), "/api/tabs/").is_some_and(|(_, action)| {
            matches!(action, "preview/read" | "preview/check" | "preview/prepare")
        })
}

fn scoped_path<'a>(path: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let rest = path.strip_prefix(prefix)?;
    let (id, action) = rest.split_once('/').unwrap_or((rest, ""));
    (!id.is_empty()).then_some((id, action))
}

#[derive(serde::Deserialize)]
struct OutputTarget {
    tab_id: Option<String>,
}

pub fn request_scope<'a>(method: &Method, uri: &'a str) -> Option<PeerScope<'a>> {
    use PeerOperation::*;
    use PeerResource::*;
    let path = request_path(uri);
    if !path.starts_with("/api/") || is_local_only_path(path) {
        return None;
    }
    let scope = match (method, path) {
        (&Method::GET | &Method::POST, path) if path.starts_with("/api/appearance/") => {
            PeerScope::new(FullAccess, Host)
        }
        (&Method::PATCH, "/api/config") => PeerScope::new(FullAccess, Host),
        (&Method::POST, "/api/mesh/rpc") => PeerScope::new(FullAccess, Host),
        (&Method::POST, "/api/mesh/announce") => PeerScope::new(Announce, Host),
        (&Method::POST, "/api/mesh/sync") => PeerScope::new(Sync, Host),
        (&Method::GET, "/api/mesh/inventory") => PeerScope::new(Inventory, Host),
        (&Method::GET, "/api/events/stream") => PeerScope::new(Rail, Host),
        (&Method::GET, "/api/mux/state") => PeerScope::new(ReadMux, Host),
        (&Method::GET, "/api/tabs") => PeerScope::new(ReadTabs, TabCollection),
        (&Method::GET, "/api/tabs/inspect" | "/api/workspaces" | "/api/shells") => {
            PeerScope::new(ReadTabs, Host)
        }
        (&Method::GET, "/api/output/stream" | "/api/sessions/transcript") => {
            PeerScope::new(ReadOutput, OutputCollection)
        }
        (&Method::POST, "/api/tabs") => PeerScope::new(WriteTab, TabCollection),
        (&Method::POST, "/api/agents/spawn") => PeerScope::new(SpawnAgent, Host),
        (&Method::GET, "/api/crons") => PeerScope::new(ReadCron, Host),
        (&Method::POST, "/api/crons") => PeerScope::new(ManageCron, Host),
        (&Method::GET, "/api/services") => PeerScope::new(ReadService, Host),
        (&Method::GET, "/api/plans") => PeerScope::new(ReadPlan, Host),
        (&Method::POST, "/api/workspaces" | "/api/workspaces/reorder") => {
            PeerScope::new(ManageWorkspace, Host)
        }
        (&Method::GET, path)
            if scoped_path(path, "/api/workspaces/")
                .is_some_and(|(_, action)| action == "close-preview") =>
        {
            let (id, _) = scoped_path(path, "/api/workspaces/")?;
            PeerScope::new(ReadWorkspaceClosePreview, Workspace(id))
        }
        (&Method::GET, "/api/fs/list-dir") | (&Method::POST, "/api/fs/mkdir") => {
            PeerScope::new(WorkspacePath, Host)
        }
        (&Method::POST, "/api/automation/blueprints/solutions/list") => {
            PeerScope::new(ReadHost, Host)
        }
        (
            &Method::GET,
            "/api/identify"
            | "/api/health"
            | "/api/system/default-cwd"
            | "/api/config"
            | "/api/network"
            | "/api/notifications"
            | "/api/notifications/channels"
            | "/api/agents/done"
            | "/api/activity"
            | "/api/usage/latest"
            | "/api/usage/history"
            | "/api/usage/projection"
            | "/api/update/status"
            | "/api/update/wait"
            | "/api/bridge/state"
            | "/api/mesh/status"
            | "/api/mesh/peers"
            | "/api/automation/graph"
            | "/api/automation/graph/workspace"
            | "/api/surfaces",
        ) => PeerScope::new(ReadHost, Host),
        _ => {
            if let Some((id, action)) = scoped_path(path, "/api/tabs/") {
                let operation = match (method, action) {
                    (
                        &Method::GET,
                        "output"
                        | "output/stream"
                        | "text"
                        | "chat"
                        | "shell-commands"
                        | "chat-send"
                        | "chat-send/queue"
                        | "chat-send/dismissals"
                        | "agent/background-work",
                    ) => ReadOutput,
                    (
                        &Method::GET,
                        "status"
                        | "usage"
                        | "turn-controls"
                        | "reports"
                        | "closed-children"
                        | "agent/service-tiers"
                        | "agent/provider-history",
                    ) => ReadTabs,
                    (
                        &Method::POST,
                        "chat-raw/start"
                        | "chat-raw/stop"
                        | "chat-raw/history"
                        | "chat-raw/backfill"
                        | "chat-raw/resume"
                        | "chat-send/admissions"
                        | "agent/runtime/thread-history",
                    ) => ReadOutput,
                    _ if is_preview_read(method, path) => ReadPreview,
                    (
                        &Method::POST,
                        "send"
                        | "attachments"
                        | "chat-send"
                        | "chat-send/queue"
                        | "chat-send/dismissals"
                        | "agent/background-work",
                    ) => SendInput,
                    (
                        &Method::POST,
                        "exec" | "respond" | "report" | "clear" | "close" | "prompt/respond"
                        | "prompt/clear",
                    ) => WriteOwnedTab,
                    (
                        &Method::POST,
                        "chat-view"
                        | "focus"
                        | "stop"
                        | "restart"
                        | "rename"
                        | "describe"
                        | "resize"
                        | "timeout"
                        | "children/reorder"
                        | "agent/runtime/retry"
                        | "agent/runtime/goal",
                    ) => WriteTab,
                    _ => return None,
                };
                PeerScope::new(operation, Tab(id))
            } else if let Some((id, action)) = scoped_path(path, "/api/crons/") {
                if let (&Method::GET, Some(run_id)) = (method, action.strip_prefix("history/")) {
                    if !run_id.is_empty() && !run_id.contains('/') {
                        return Some(PeerScope::new(
                            ReadCronRun,
                            CronRun {
                                cron_id: id,
                                run_id,
                            },
                        ));
                    }
                }
                let operation = match (method, action) {
                    (&Method::GET, "" | "history") => ReadCron,
                    (&Method::PUT | &Method::PATCH | &Method::DELETE, "") => ManageCron,
                    (&Method::POST, "enable" | "disable" | "trigger" | "cancel") => ManageCron,
                    _ => return None,
                };
                PeerScope::new(operation, Cron(id))
            } else if *method == Method::GET && is_plan_view_read(path) {
                PeerScope::new(ReadPlan, Host)
            } else if let Some((id, "")) = scoped_path(path, "/api/services/") {
                let operation = match *method {
                    Method::GET => ReadService,
                    Method::DELETE => FullAccess,
                    _ => return None,
                };
                PeerScope::new(operation, Service(id))
            } else if let Some((id, "stream")) = scoped_path(path, "/api/surfaces/") {
                if *method != Method::GET {
                    return None;
                }
                PeerScope::new(ReadOutput, Service(id))
            } else if *method == Method::POST
                && scoped_path(path, "/api/workspaces/").is_some_and(|(_, action)| {
                    matches!(
                        action,
                        "focus"
                            | "cwd"
                            | "rename"
                            | "close"
                            | "reopen-closed"
                            | "tabs"
                            | "tabs/reorder"
                            | "tabs/display-order"
                    )
                })
            {
                PeerScope::new(ManageWorkspace, Host)
            } else {
                return None;
            }
        }
    };
    Some(scope)
}

fn is_plan_view_read(path: &str) -> bool {
    if let Some((_, action)) = scoped_path(path, "/api/plans/") {
        if matches!(action, "projection" | "board-state" | "tasks") {
            return true;
        }
        return scoped_path(action, "tasks/")
            .is_some_and(|(_, detail)| matches!(detail, "" | "history"));
    }
    scoped_path(path, "/api/teams/").is_some_and(|(_, action)| action == "mission-control/state")
        || scoped_path(path, "/api/specs/").is_some_and(|(_, action)| action.is_empty())
}

pub fn peer_access_allowed(
    access: &AccessLevel,
    peer_id: &str,
    method: &Method,
    uri: &str,
    tab_owner: impl FnOnce(&str) -> Option<String>,
) -> bool {
    // The query extractor decodes the exact scope consumed by the output handler.
    if *method == Method::GET && request_path(uri) == "/api/output/stream" {
        let Some(target) = uri
            .parse::<Uri>()
            .ok()
            .and_then(|uri| axum::extract::Query::<OutputTarget>::try_from_uri(&uri).ok())
        else {
            return false;
        };
        if let Some(id) = target.tab_id.as_deref() {
            return !id.is_empty()
                && PeerScope::new(PeerOperation::ReadOutput, PeerResource::Tab(id))
                    .allowed_for_peer(access, peer_id, tab_owner);
        }
    }
    request_scope(method, uri).is_some_and(|scope| {
        if *access == AccessLevel::AgentIsolated
            && matches!(scope.resource, PeerResource::Tab(id) if id.contains('%'))
        {
            return false;
        }
        scope.allowed_for_peer(access, peer_id, tab_owner)
    })
}

pub fn allows_full_dispatch(access: &AccessLevel) -> bool {
    PeerScope::new(PeerOperation::FullAccess, PeerResource::Host).allowed(access)
}

pub fn is_observation(method: &str, path: &str) -> bool {
    use PeerOperation::*;
    method.eq_ignore_ascii_case("GET")
        || (method.eq_ignore_ascii_case("POST")
            && request_scope(&Method::POST, path).is_some_and(|scope| {
                matches!(
                    scope.operation,
                    ReadOutput | ReadPreview | ReadCronRun | ReadHost
                )
            }))
}

pub fn check_access(access: &AccessLevel, method: &Method, path: &str) -> bool {
    request_scope(method, path).is_some_and(|scope| scope.allowed(access))
}

pub fn is_local_only_path(path: &str) -> bool {
    let path = request_path(path);
    path.starts_with("/api/analytics/")
        || path.starts_with("/api/dictation/")
        || path.starts_with("/api/secrets/")
        || path.starts_with("/api/account/")
        || path == "/api/mesh/debug/sign"
        || path == SUPPORTER_TOKEN_PATH
        || path == "/api/agents/reparent"
        || matches!(
            path,
            "/api/cli/rpc"
                | "/api/model-providers"
                | "/api/mesh/link"
                | "/api/mesh/relay-to"
                | "/api/mesh/relay-stream"
                | "/api/mesh/disconnect"
                | "/api/mesh/unrevoke"
        )
        || path.starts_with("/api/mesh/peers/disconnect/")
        || path.starts_with("/api/mesh/peers/") && path.ends_with("/access")
        // Canvas write-back belongs to the owner on this machine; peers only read previews.
        || (path.starts_with("/api/tabs/") && path.contains("/preview/canvas/"))
        || (path.starts_with("/api/blueprints/") && path.ends_with("/webhook/configure"))
        || (path.starts_with("/api/teams/") && path.ends_with("/adopt-remote"))
        || scoped_path(path, "/api/workspaces/").is_some_and(|(_, action)| {
            scoped_path(action, "tabs/").is_some_and(|(_, action)| action == "close")
        })
}

#[cfg(test)]
mod tests;

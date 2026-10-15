use crate::error::StorageError;
use crate::repositories::workspaces::WorkspaceRecord;
use crate::repositories::Revision;
use serde_json::Value;
use std::collections::HashMap;

pub const MAX_DESK_WORKSPACES: usize = 4096;
pub const MAX_DESK_TABS: usize = 16_384;
pub const MAX_DESK_TEAMS: usize = 4096;
pub const MAX_DESK_LAYOUT_NODES: usize = 65_536;
pub const MAX_DESK_LAYOUT_DEPTH: usize = 128;
pub const MAX_DESK_LEAF_BYTES: usize = 64 * 1024;
pub const MAX_DESK_CATALOGUE: usize = 65_536;

const LEAF_FIELDS: &[&str] = &[
    "last_active_at_ms",
    "purpose",
    "icon",
    "utility_created_at",
    "shell_profile",
    "close_restore",
    "title",
    "description",
    "title_level",
    "agent_type",
    "agent_type_locked",
    "claude_session_id",
    "agent_session_id",
    "agent_identity",
    "role",
    "role_profile",
    "team_id",
    "parent_tab_id",
    "spawned_by_tab_id",
    "report_target_tab_id",
    "report_target_mesh_peer",
    "ownership",
    "can_spawn",
    "depth",
    "chat_view",
    "auto_approve",
    "permission_auto_confirm",
    "audience",
    "agent_session_model",
    "agent_planned_model",
    "agent_requested_model",
    "agent_provider_resolved_model",
    "model_fallback",
    "model_identity_mismatch",
    "chatbackend_plugin_id",
    "chatbackend_session_id",
    "surface",
    "watcher",
    "pending_delivery",
    "cwd",
];

#[derive(Debug, Clone, PartialEq)]
pub struct DeskLeafFields {
    pub values: Value,
}

impl Default for DeskLeafFields {
    fn default() -> Self {
        DeskLeafFields {
            values: Value::Object(Default::default()),
        }
    }
}

impl DeskLeafFields {
    pub fn validate(&self) -> Result<(), StorageError> {
        let object = self
            .values
            .as_object()
            .ok_or_else(|| StorageError::invalid("desk leaf fields", "must be a JSON object"))?;
        if let Some(field) = object
            .keys()
            .find(|field| !LEAF_FIELDS.contains(&field.as_str()))
        {
            return Err(StorageError::invalid(
                "desk leaf fields",
                format!("unknown field {field}"),
            ));
        }
        for (field, value) in object {
            if !valid_leaf_value(field, value) {
                return Err(StorageError::invalid(
                    "desk leaf fields",
                    format!("field {field} has the wrong JSON value type"),
                ));
            }
        }
        let bytes = serde_json::to_vec(&self.values)
            .map_err(|error| StorageError::invalid("desk leaf fields", error.to_string()))?
            .len();
        if bytes > MAX_DESK_LEAF_BYTES {
            return Err(StorageError::invalid(
                "desk leaf fields",
                format!("exceeds {MAX_DESK_LEAF_BYTES} bytes"),
            ));
        }
        Ok(())
    }
}

fn valid_leaf_value(field: &str, value: &Value) -> bool {
    match field {
        "purpose" => value.as_str().is_some_and(|purpose| {
            matches!(
                purpose,
                "build" | "dev" | "qa" | "deploy" | "logs" | "db" | "shell"
            )
        }),
        "agent_type_locked" | "can_spawn" | "auto_approve" | "permission_auto_confirm" => {
            value.is_boolean()
        }
        "utility_created_at" => value.as_u64().is_some(),
        "depth" => value.as_u64().is_some_and(|depth| depth <= u8::MAX as u64),
        "chat_view" => value.is_boolean() || value.is_null(),
        "icon"
        | "title"
        | "description"
        | "title_level"
        | "agent_type"
        | "claude_session_id"
        | "agent_session_id"
        | "role"
        | "role_profile"
        | "team_id"
        | "parent_tab_id"
        | "spawned_by_tab_id"
        | "report_target_tab_id"
        | "report_target_mesh_peer"
        | "audience"
        | "agent_session_model"
        | "agent_planned_model"
        | "agent_requested_model"
        | "agent_provider_resolved_model"
        | "chatbackend_plugin_id"
        | "chatbackend_session_id"
        | "cwd" => value.is_string() || value.is_null(),
        "agent_identity"
        | "shell_profile"
        | "close_restore"
        | "ownership"
        | "model_fallback"
        | "model_identity_mismatch"
        | "watcher"
        | "surface"
        | "pending_delivery" => value.is_object() || value.is_null(),
        "last_active_at_ms" => value.is_u64() || value.is_null(),
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeskLayoutNode {
    Leaf {
        tab_id: String,
        cwd: Option<String>,
        fields: DeskLeafFields,
    },
    Split {
        orientation: String,
        ratio: f64,
        children: Vec<DeskLayoutNode>,
        fields: serde_json::Map<String, Value>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeskTab {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub sort_order: i32,
    pub layout: DeskLayoutNode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeskWorkspace {
    pub record: WorkspaceRecord,
    pub sort_order: i32,
    pub active_tab_id: Option<String>,
    pub tabs: Vec<DeskTab>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeskTeamLifecycle {
    Forming,
    Active,
    Closing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskTeam {
    pub id: String,
    pub name: Option<String>,
    pub lifecycle: DeskTeamLifecycle,
    pub created_at: u64,
    pub workspace_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskAgentSessionIdentity {
    pub provider: String,
    pub session_id: String,
    pub record_provider: String,
    pub role: String,
    pub role_profile: Option<String>,
    pub parent_tab_id: Option<String>,
    pub can_spawn: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeskSnapshot {
    pub version: String,
    pub timestamp: u64,
    pub active_workspace_id: Option<String>,
    pub workspaces: Vec<DeskWorkspace>,
    pub teams: Vec<DeskTeam>,
    pub agent_session_identities: Vec<DeskAgentSessionIdentity>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeskState {
    pub snapshot: DeskSnapshot,
    pub revision: Revision,
    pub workspace_order_revision: Revision,
    pub workspace_revisions: HashMap<String, Revision>,
    pub workspace_order_revisions: HashMap<String, Revision>,
    pub tab_revisions: HashMap<String, Revision>,
    pub team_revisions: HashMap<String, Revision>,
}

impl DeskSnapshot {
    pub fn validate(&self) -> Result<usize, StorageError> {
        self.validate_inner(true)
    }

    pub(crate) fn validate_preserving_orphan_teams(&self) -> Result<usize, StorageError> {
        self.validate_inner(false)
    }

    fn validate_inner(&self, require_team_workspace: bool) -> Result<usize, StorageError> {
        if self.workspaces.len() > MAX_DESK_WORKSPACES {
            return Err(StorageError::invalid(
                "desk workspaces",
                format!("must contain at most {MAX_DESK_WORKSPACES} records"),
            ));
        }
        if self.teams.len() > MAX_DESK_TEAMS
            || self.agent_session_identities.len() > MAX_DESK_CATALOGUE
        {
            return Err(StorageError::invalid(
                "desk records",
                "exceeds inventory bound",
            ));
        }
        i64::try_from(self.timestamp)
            .map_err(|_| StorageError::invalid("desk timestamp", "exceeds SQLite integer range"))?;
        let mut workspaces = std::collections::HashSet::new();
        let mut tabs = std::collections::HashSet::new();
        let mut teams = std::collections::HashSet::new();
        let mut identities = std::collections::HashSet::new();
        let mut nodes = 0;
        let mut cost = self.version.len();
        for workspace in &self.workspaces {
            if !workspaces.insert(workspace.record.id.as_str().to_owned()) {
                return Err(StorageError::invalid(
                    "desk workspace id",
                    "duplicate identity",
                ));
            }
            workspace.record.validate()?;
            cost = cost.saturating_add(workspace.record.cost());
            let mut local_tabs = std::collections::HashSet::new();
            for tab in &workspace.tabs {
                if !tabs.insert(tab.id.clone()) || !local_tabs.insert(tab.id.clone()) {
                    return Err(StorageError::invalid("desk tab id", "duplicate identity"));
                }
                if tab.id.is_empty() || tab.title.len() > 4096 {
                    return Err(StorageError::invalid(
                        "desk tab",
                        "invalid identity or title",
                    ));
                }
                cost = cost
                    .saturating_add(tab.id.len())
                    .saturating_add(tab.title.len())
                    .saturating_add(tab.description.as_ref().map_or(0, String::len));
                count_layout(&tab.layout, 0, &mut nodes, &mut cost)?;
            }
        }
        for team in &self.teams {
            if team.id.is_empty() || !teams.insert(team.id.clone()) {
                return Err(StorageError::invalid(
                    "desk team id",
                    "empty or duplicate identity",
                ));
            }
            if require_team_workspace && !workspaces.contains(team.workspace_id.as_str()) {
                return Err(StorageError::invalid(
                    "Team workspace",
                    "must name a desk workspace",
                ));
            }
            cost = cost
                .saturating_add(team.id.len())
                .saturating_add(team.workspace_id.len())
                .saturating_add(team.name.as_ref().map_or(0, String::len));
        }
        for identity in &self.agent_session_identities {
            if identity.provider.is_empty()
                || identity.session_id.is_empty()
                || identity.record_provider.is_empty()
                || !identities.insert((identity.provider.as_str(), identity.session_id.as_str()))
            {
                return Err(StorageError::invalid(
                    "agent session identity",
                    "provider/session identity is empty or duplicated",
                ));
            }
            cost = cost
                .saturating_add(identity.provider.len())
                .saturating_add(identity.session_id.len())
                .saturating_add(identity.record_provider.len())
                .saturating_add(identity.role.len())
                .saturating_add(identity.role_profile.as_ref().map_or(0, String::len))
                .saturating_add(identity.parent_tab_id.as_ref().map_or(0, String::len));
        }
        if tabs.len() > MAX_DESK_TABS || nodes > MAX_DESK_LAYOUT_NODES {
            return Err(StorageError::invalid(
                "desk layout",
                "exceeds inventory bound",
            ));
        }
        if cost > crate::DEFAULT_MAILBOX_BYTES {
            return Err(StorageError::invalid(
                "desk inventory",
                "exceeds Storage mailbox bound",
            ));
        }
        Ok(cost)
    }
}

fn count_layout(
    node: &DeskLayoutNode,
    depth: usize,
    count: &mut usize,
    cost: &mut usize,
) -> Result<(), StorageError> {
    if depth > MAX_DESK_LAYOUT_DEPTH {
        return Err(StorageError::invalid("desk layout", "exceeds depth bound"));
    }
    *count += 1;
    match node {
        DeskLayoutNode::Leaf {
            tab_id,
            cwd,
            fields,
        } => {
            if tab_id.is_empty() {
                return Err(StorageError::invalid(
                    "desk leaf tab id",
                    "must not be empty",
                ));
            }
            fields.validate()?;
            *cost = cost
                .saturating_add(tab_id.len())
                .saturating_add(cwd.as_ref().map_or(0, String::len));
            *cost =
                cost.saturating_add(serde_json::to_vec(&fields.values).unwrap_or_default().len());
        }
        DeskLayoutNode::Split {
            orientation,
            ratio,
            children,
            fields,
        } => {
            if orientation.is_empty() || orientation.len() > 32 || !ratio.is_finite() {
                return Err(StorageError::invalid(
                    "desk split",
                    "invalid orientation or ratio",
                ));
            }
            *cost = cost.saturating_add(orientation.len());
            let fields_cost = serde_json::to_vec(fields)
                .map_err(|error| StorageError::invalid("desk split fields", error.to_string()))?
                .len();
            if fields_cost > MAX_DESK_LEAF_BYTES {
                return Err(StorageError::invalid(
                    "desk split fields",
                    format!("exceeds {MAX_DESK_LEAF_BYTES} bytes"),
                ));
            }
            *cost = cost.saturating_add(fields_cost);
            for child in children {
                count_layout(child, depth + 1, count, cost)?;
            }
        }
    }
    if *count > MAX_DESK_LAYOUT_NODES {
        return Err(StorageError::invalid("desk layout", "exceeds node bound"));
    }
    Ok(())
}

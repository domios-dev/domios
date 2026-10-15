use super::legacy_decode;
use super::{SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot};
#[cfg(test)]
use codeterm_storage::StorageConfig;
use codeterm_storage::{
    Committed, Completion, DeskAgentSessionIdentity, DeskLayoutNode, DeskLeafFields, DeskSnapshot,
    DeskState, DeskTab, DeskTeam, DeskTeamLifecycle, DeskWorkspace, Revision, Storage,
};
use codeterm_types::plan::task_key::WorkspaceTaskKeys;
use std::collections::HashSet;
#[cfg(test)]
use std::path::Path;
use std::time::Duration;

const COMPLETION_DEADLINE: Duration = Duration::from_secs(30);

fn snapshot_workspace_ids(snapshot: &SessionSnapshot) -> HashSet<String> {
    snapshot
        .workspaces
        .iter()
        .map(|workspace| workspace.id.clone())
        .collect()
}

/// Blocking, dormant bridge from legacy snapshots to typed desk rows; use off the async runtime.
pub struct StorageDeskAdapter {
    storage: Storage,
}

impl StorageDeskAdapter {
    /// Storage open is blocking and must run off the async runtime and outside mux locks.
    #[cfg(test)]
    pub fn open(root: impl AsRef<Path>) -> Result<Self, String> {
        let storage =
            Storage::open(root, StorageConfig::default()).map_err(|error| error.to_string())?;
        Ok(StorageDeskAdapter { storage })
    }

    pub fn from_storage(storage: Storage) -> Self {
        StorageDeskAdapter { storage }
    }

    pub fn import_legacy(
        &self,
        snapshot_json: &str,
        identity_catalog_json: Option<&str>,
    ) -> Result<(), String> {
        let decoded = legacy_decode::decode_snapshot(snapshot_json)?;
        let identities = legacy_decode::decode_identity_catalog(identity_catalog_json)?;
        let mut snapshot = to_desk_snapshot(&decoded.snapshot, identities)?;
        if snapshot.workspaces.len() != decoded.layouts.len() {
            return Err("legacy snapshot layout count does not match workspaces".to_string());
        }
        for (workspace, layouts) in snapshot.workspaces.iter_mut().zip(decoded.layouts) {
            if workspace.tabs.len() != layouts.len() {
                return Err("legacy snapshot layout count does not match tabs".to_string());
            }
            for (tab, layout) in workspace.tabs.iter_mut().zip(layouts) {
                tab.layout = layout;
            }
        }
        self.import_normalized(snapshot)
    }

    pub fn import_snapshot(
        &self,
        snapshot: &SessionSnapshot,
        identities: Vec<DeskAgentSessionIdentity>,
    ) -> Result<(), String> {
        let normalized = to_desk_snapshot(snapshot, identities)?;
        self.import_normalized(normalized)
    }

    fn import_normalized(&self, normalized: DeskSnapshot) -> Result<(), String> {
        self.storage
            .desk()
            .import(normalized)
            .wait(COMPLETION_DEADLINE)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn additive_restore(
        &self,
        snapshot: &SessionSnapshot,
        identities: Vec<DeskAgentSessionIdentity>,
    ) -> Result<(), String> {
        let normalized = to_desk_snapshot(snapshot, identities)?;
        self.storage
            .desk()
            .additive_restore(normalized)
            .wait(COMPLETION_DEADLINE)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn snapshot(&self) -> Result<SessionSnapshot, String> {
        from_desk_snapshot(self.normalized_snapshot()?)
    }

    pub fn normalized_snapshot(&self) -> Result<DeskSnapshot, String> {
        Ok(self.state()?.snapshot)
    }

    pub fn state(&self) -> Result<DeskState, String> {
        self.storage
            .desk()
            .snapshot()
            .wait(COMPLETION_DEADLINE)
            .map(|state| state.value)
            .map_err(|error| error.to_string())
    }

    pub fn identity_catalog(&self) -> Result<Vec<DeskAgentSessionIdentity>, String> {
        self.storage
            .desk()
            .snapshot()
            .wait(COMPLETION_DEADLINE)
            .map(|read| read.value.snapshot.agent_session_identities)
            .map_err(|error| error.to_string())
    }

    pub fn rename_workspace(
        &self,
        id: String,
        expected: Revision,
        name: String,
    ) -> Result<Revision, String> {
        self.storage
            .desk()
            .rename_workspace(id, expected, name)
            .wait(COMPLETION_DEADLINE)
            .map(|committed| committed.receipt.revision)
            .map_err(|error| error.to_string())
    }

    pub fn update_workspace(
        &self,
        record: codeterm_storage::WorkspaceRecord,
        expected: Revision,
    ) -> Result<Revision, String> {
        self.storage
            .desk()
            .update_workspace(record, expected)
            .wait(COMPLETION_DEADLINE)
            .map(|committed| committed.receipt.revision)
            .map_err(|error| error.to_string())
    }

    pub fn rename_tab(
        &self,
        id: String,
        expected: Revision,
        title: String,
    ) -> Result<Revision, String> {
        self.storage
            .desk()
            .rename_tab(id, expected, title)
            .wait(COMPLETION_DEADLINE)
            .map(|committed| committed.receipt.revision)
            .map_err(|error| error.to_string())
    }

    pub fn rename_team(
        &self,
        id: String,
        expected: Revision,
        name: String,
    ) -> Result<Revision, String> {
        self.storage
            .desk()
            .rename_team(id, expected, name)
            .wait(COMPLETION_DEADLINE)
            .map(|committed| committed.receipt.revision)
            .map_err(|error| error.to_string())
    }

    pub fn set_team_lifecycle(
        &self,
        id: String,
        expected: Revision,
        lifecycle: DeskTeamLifecycle,
    ) -> Result<Revision, String> {
        self.storage
            .desk()
            .set_team_lifecycle(id, expected, lifecycle)
            .wait(COMPLETION_DEADLINE)
            .map(|committed| committed.receipt.revision)
            .map_err(|error| error.to_string())
    }

    pub fn remove_team(&self, id: String, expected: Revision) -> Result<Revision, String> {
        self.storage
            .desk()
            .remove_team(id, expected)
            .wait(COMPLETION_DEADLINE)
            .map(|committed| committed.receipt.revision)
            .map_err(|error| error.to_string())
    }

    pub fn select_workspace(
        &self,
        id: Option<String>,
        expected: Revision,
    ) -> Result<Revision, String> {
        self.storage
            .desk()
            .select_workspace(id, expected)
            .wait(COMPLETION_DEADLINE)
            .map(|committed| committed.receipt.revision)
            .map_err(|error| error.to_string())
    }

    pub fn select_tab(
        &self,
        workspace_id: String,
        expected_workspace: Revision,
        tab_id: Option<String>,
    ) -> Result<Revision, String> {
        self.storage
            .desk()
            .select_tab(workspace_id, expected_workspace, tab_id)
            .wait(COMPLETION_DEADLINE)
            .map(|committed| committed.receipt.revision)
            .map_err(|error| error.to_string())
    }

    pub fn reorder_workspaces(
        &self,
        expected_order: Revision,
        order: Vec<String>,
    ) -> Result<Revision, String> {
        self.storage
            .desk()
            .reorder_workspaces(expected_order, order)
            .wait(COMPLETION_DEADLINE)
            .map(|committed| committed.receipt.revision)
            .map_err(|error| error.to_string())
    }

    pub fn reorder_tabs(
        &self,
        workspace_id: String,
        expected_order: Revision,
        order: Vec<String>,
    ) -> Result<Revision, String> {
        self.storage
            .desk()
            .reorder_tabs(workspace_id, expected_order, order)
            .wait(COMPLETION_DEADLINE)
            .map(|committed| committed.receipt.revision)
            .map_err(|error| error.to_string())
    }

    pub fn close_tab(&self, id: String, expected: Revision) -> Result<Revision, String> {
        self.storage
            .desk()
            .close_tab(id, expected)
            .wait(COMPLETION_DEADLINE)
            .map(|committed| committed.receipt.revision)
            .map_err(|error| error.to_string())
    }

    pub fn close_workspace(&self, id: String, expected: Revision) -> Result<Revision, String> {
        self.storage
            .desk()
            .close_workspace(id, expected)
            .wait(COMPLETION_DEADLINE)
            .map(|committed| committed.receipt.revision)
            .map_err(|error| error.to_string())
    }

    pub fn delta_snapshot(
        &self,
        snapshot: &SessionSnapshot,
        identities: Vec<DeskAgentSessionIdentity>,
        expected: Revision,
        intent: super::SaveIntent,
    ) -> Result<Completion<Committed<()>>, String> {
        let normalized = to_desk_snapshot(snapshot, identities)?;
        let desk = self.storage.desk();
        Ok(match intent {
            super::SaveIntent::PreserveDesk => desk.save_preserving(normalized, expected),
            super::SaveIntent::AuthorizedReduction => {
                desk.save_authorized_reduction(normalized, expected)
            }
        })
    }

    pub fn flush(&self) -> Result<(), String> {
        self.storage
            .desk()
            .flush()
            .wait(COMPLETION_DEADLINE)
            .map_err(|error| error.to_string())
    }
}

fn to_desk_snapshot(
    snapshot: &SessionSnapshot,
    agent_session_identities: Vec<DeskAgentSessionIdentity>,
) -> Result<DeskSnapshot, String> {
    let workspace_ids = snapshot_workspace_ids(snapshot);
    let workspaces = snapshot
        .workspaces
        .iter()
        .map(|workspace| {
            let mut record = codeterm_storage::WorkspaceRecord::new(
                codeterm_storage::WorkspaceId::new(workspace.id.clone())
                    .map_err(|error| error.to_string())?,
                workspace.name.clone(),
            );
            record.cwd = workspace.cwd.clone();
            record.provisioned_cwd = workspace.provisioned_cwd.clone();
            record.protected = workspace.protected;
            record.task_key_prefix = workspace.task_keys.prefix.clone();
            record.task_key_next = workspace.task_keys.next;
            let tabs = workspace
                .tabs
                .iter()
                .map(|tab| {
                    Ok(DeskTab {
                        id: tab.id.clone(),
                        title: tab.title.clone(),
                        description: tab.description.clone(),
                        sort_order: tab.sort_order,
                        layout: to_tab_layout(tab)?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(DeskWorkspace {
                record,
                sort_order: workspace.sort_order,
                active_tab_id: workspace.active_tab_id.clone(),
                tabs,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(DeskSnapshot {
        version: snapshot.version.clone(),
        timestamp: snapshot.timestamp,
        active_workspace_id: snapshot.active_workspace_id.clone(),
        workspaces,
        teams: snapshot
            .teams
            .iter()
            .filter(|team| workspace_ids.contains(team.workspace_id.as_str()))
            .map(|team| DeskTeam {
                id: team.id.clone(),
                name: team.name.clone(),
                lifecycle: match team.lifecycle {
                    codeterm_types::team::TeamLifecycle::Forming => DeskTeamLifecycle::Forming,
                    codeterm_types::team::TeamLifecycle::Active => DeskTeamLifecycle::Active,
                    codeterm_types::team::TeamLifecycle::Closing => DeskTeamLifecycle::Closing,
                },
                created_at: team.created_at,
                workspace_id: team.workspace_id.clone(),
            })
            .collect(),
        agent_session_identities,
    })
}

pub fn project_snapshot(
    snapshot: &SessionSnapshot,
    agent_session_identities: Vec<DeskAgentSessionIdentity>,
) -> Result<DeskSnapshot, String> {
    to_desk_snapshot(snapshot, agent_session_identities)
}

pub fn prune_orphan_teams(snapshot: &mut SessionSnapshot) -> Vec<codeterm_types::team::Team> {
    let workspace_ids = snapshot_workspace_ids(snapshot);
    let mut orphans: Vec<_> = snapshot
        .teams
        .iter()
        .filter(|team| !workspace_ids.contains(team.workspace_id.as_str()))
        .cloned()
        .collect();
    orphans.sort_by(|left, right| left.id.cmp(&right.id));
    let orphan_ids: HashSet<&str> = orphans.iter().map(|team| team.id.as_str()).collect();
    snapshot
        .teams
        .retain(|team| !orphan_ids.contains(team.id.as_str()));
    orphans
}

pub fn orphan_teams(snapshot: &SessionSnapshot) -> Vec<(String, String)> {
    let workspace_ids = snapshot_workspace_ids(snapshot);
    let mut orphans: Vec<_> = snapshot
        .teams
        .iter()
        .filter(|team| !workspace_ids.contains(team.workspace_id.as_str()))
        .map(|team| (team.id.clone(), team.workspace_id.clone()))
        .collect();
    orphans.sort_unstable();
    orphans
}

fn to_layout(node: &SplitNodeSnapshot) -> Result<DeskLayoutNode, String> {
    match node {
        SplitNodeSnapshot::Leaf { tab_id, cwd, .. } => {
            let mut value = serde_json::to_value(node).map_err(|error| error.to_string())?;
            let object = value
                .as_object_mut()
                .ok_or_else(|| "serialized leaf is not an object".to_string())?;
            object.remove("type");
            object.remove("tab_id");
            Ok(DeskLayoutNode::Leaf {
                tab_id: tab_id.clone(),
                cwd: cwd.clone(),
                fields: DeskLeafFields {
                    values: super::leaf_bound::bound_leaf(tab_id, value),
                },
            })
        }
        SplitNodeSnapshot::Split {
            orientation,
            ratio,
            children,
            extra_fields,
        } => Ok(DeskLayoutNode::Split {
            orientation: orientation.clone(),
            ratio: *ratio,
            fields: extra_fields.clone(),
            children: children
                .iter()
                .map(to_layout)
                .collect::<Result<Vec<_>, _>>()?,
        }),
    }
}

fn from_desk_snapshot(snapshot: DeskSnapshot) -> Result<SessionSnapshot, String> {
    let workspaces = snapshot
        .workspaces
        .into_iter()
        .map(|workspace| {
            Ok(WorkspaceSnapshot {
                id: workspace.record.id.as_str().to_owned(),
                name: workspace.record.name,
                tabs: workspace
                    .tabs
                    .into_iter()
                    .map(|tab| {
                        Ok(TabSnapshot {
                            last_active_at_ms: layout_activity(&tab.layout),
                            id: tab.id,
                            title: tab.title,
                            description: tab.description,
                            split_tree: from_layout(tab.layout)?,
                            sort_order: tab.sort_order,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
                active_tab_id: workspace.active_tab_id,
                cwd: workspace.record.cwd,
                provisioned_cwd: workspace.record.provisioned_cwd,
                protected: workspace.record.protected,
                sort_order: workspace.sort_order,
                task_keys: WorkspaceTaskKeys {
                    prefix: workspace.record.task_key_prefix,
                    next: workspace.record.task_key_next,
                },
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(SessionSnapshot {
        version: snapshot.version,
        timestamp: snapshot.timestamp,
        workspaces,
        active_workspace_id: snapshot.active_workspace_id,
        teams: snapshot
            .teams
            .into_iter()
            .map(|team| codeterm_types::team::Team {
                id: team.id,
                name: team.name,
                lifecycle: match team.lifecycle {
                    DeskTeamLifecycle::Forming => codeterm_types::team::TeamLifecycle::Forming,
                    DeskTeamLifecycle::Active => codeterm_types::team::TeamLifecycle::Active,
                    DeskTeamLifecycle::Closing => codeterm_types::team::TeamLifecycle::Closing,
                },
                created_at: team.created_at,
                workspace_id: team.workspace_id,
            })
            .collect(),
    })
}

fn from_layout(node: DeskLayoutNode) -> Result<SplitNodeSnapshot, String> {
    match node {
        DeskLayoutNode::Leaf {
            tab_id,
            cwd,
            fields,
        } => {
            let mut value = fields.values;
            let object = value
                .as_object_mut()
                .ok_or_else(|| "stored leaf fields are not an object".to_string())?;
            object.insert("type".into(), serde_json::Value::String("Leaf".into()));
            object.insert("tab_id".into(), serde_json::Value::String(tab_id));
            if !object.contains_key("cwd") {
                if let Some(cwd) = cwd {
                    object.insert("cwd".into(), serde_json::Value::String(cwd));
                }
            }
            serde_json::from_value(value).map_err(|error| error.to_string())
        }
        DeskLayoutNode::Split {
            orientation,
            ratio,
            children,
            fields,
        } => Ok(SplitNodeSnapshot::Split {
            orientation,
            ratio,
            children: children
                .into_iter()
                .map(from_layout)
                .collect::<Result<Vec<_>, _>>()?,
            extra_fields: fields,
        }),
    }
}

fn to_tab_layout(tab: &TabSnapshot) -> Result<DeskLayoutNode, String> {
    let mut layout = to_layout(&tab.split_tree)?;
    fn set(node: &mut DeskLayoutNode, activity: Option<u64>) {
        match node {
            DeskLayoutNode::Leaf { fields, .. } => {
                if let Some(object) = fields.values.as_object_mut() {
                    object.insert("last_active_at_ms".into(), serde_json::json!(activity));
                }
            }
            DeskLayoutNode::Split { children, .. } => {
                for child in children {
                    set(child, activity);
                }
            }
        }
    }
    set(&mut layout, tab.last_active_at_ms);
    Ok(layout)
}

fn layout_activity(node: &DeskLayoutNode) -> Option<u64> {
    match node {
        DeskLayoutNode::Leaf { fields, .. } => fields
            .values
            .get("last_active_at_ms")
            .and_then(serde_json::Value::as_u64),
        DeskLayoutNode::Split { children, .. } => children.iter().find_map(layout_activity),
    }
}

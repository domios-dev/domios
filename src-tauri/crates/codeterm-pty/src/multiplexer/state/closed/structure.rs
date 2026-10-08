use std::collections::HashSet;

use codeterm_types::team::Team;
use serde::{Deserialize, Serialize};

use crate::multiplexer::types::{AgentRole, RestorableLaunchParameters, Tab, Workspace};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedTab {
    pub tab: Tab,
    pub launch: RestorableLaunchParameters,
    pub paired_planner_tab_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructureState {
    Captured,
    Closed,
    Restoring,
    Restored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedStructure {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub closed_at: u64,
    pub undo_until: u64,
    pub state: StructureState,
    pub workspace: Workspace,
    pub workspaces: Vec<Workspace>,
    pub teams: Vec<Team>,
    pub tabs: Vec<ArchivedTab>,
    pub resources: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedStructureView {
    pub state: StructureState,
    pub id: String,
    pub kind: String,
    pub label: String,
    pub closed_at: u64,
    pub undo_until: u64,
    pub member_count: usize,
    pub team_count: usize,
    pub shell_count: usize,
    pub agent_count: usize,
    pub cron_count: usize,
    pub blueprint_count: usize,
}

impl ClosedStructure {
    pub fn view(&self) -> ClosedStructureView {
        let count = |key: &str| {
            self.resources
                .get(key)
                .and_then(|value| value.as_array())
                .map_or(0, Vec::len)
        };
        ClosedStructureView {
            state: self.state,
            id: self.id.clone(),
            kind: self.kind.clone(),
            label: self.label.clone(),
            closed_at: self.closed_at,
            undo_until: self.undo_until,
            member_count: self.tabs.len(),
            team_count: self.teams.len(),
            shell_count: self
                .tabs
                .iter()
                .filter(|item| item.tab.is_utility_shell())
                .count(),
            agent_count: self
                .tabs
                .iter()
                .filter(|item| item.tab.agent_type.is_some())
                .count(),
            cron_count: count("crons"),
            blueprint_count: count("blueprints"),
        }
    }

    pub fn restore_tabs(&self) -> Result<Vec<Tab>, String> {
        let mut ids = HashSet::new();
        for item in &self.tabs {
            if item.tab.id.is_empty() || !ids.insert(item.tab.id.as_str()) {
                return Err("Close inventory contains duplicate or empty tab identities".into());
            }
            if item.tab.agent_type.is_some()
                && item
                    .tab
                    .agent_session_id
                    .as_deref()
                    .is_none_or(str::is_empty)
                && !item.tab.is_chat_backend()
            {
                return Err(format!(
                    "Tab {} has no exact provider session to restore",
                    item.tab.id
                ));
            }
        }
        Ok(self
            .tabs
            .iter()
            .map(|item| {
                let mut tab = item.tab.clone();
                if tab.cwd.is_none() {
                    tab.cwd = self
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.tabs.contains(&tab.id))
                        .and_then(|workspace| workspace.cwd.clone());
                }
                tab.pty_id = None;
                tab.pending_delivery = None;
                tab.pending_prompt = None;
                tab.pending_prompt_kind = None;
                tab.agent_state = None;
                tab.agent_booting = false;
                tab.session_binding_nonce = None;
                tab.reported_session_binding_nonce = None;
                tab.shell_integration_nonce = None;
                tab.lifecycle_epoch = None;
                tab.lifecycle_operation_id = None;
                tab.boot_stage = None;
                tab.boot_failure_reason = None;
                tab.provisional_launch = None;
                tab.utility_command_pending = false;
                tab.shell_command_state = Default::default();
                tab.restorable_launch = item.launch.clone();
                tab.pending_paired_planner_tab_id = item.paired_planner_tab_id.clone();
                tab.restored_from_snapshot = tab.agent_type.is_some() || tab.is_chat_backend();
                let shell = tab.is_utility_shell();
                let restore = tab.close_restore.get_or_insert_with(Default::default);
                restore.record_id = Some(self.id.clone());
                restore.stopped = shell;
                restore.exact_session = tab.agent_type.is_some();
                if restore.session_cwd.is_none() {
                    restore.session_cwd = tab.cwd.clone();
                }
                restore.paired_planner_tab_id = item.paired_planner_tab_id.clone();
                if tab.role == Some(AgentRole::Worker)
                    && (tab.task_binding.is_some() || tab.worker_id.is_some())
                {
                    tab.role = None;
                    tab.role_profile = None;
                    tab.worker_id = None;
                    tab.task_binding = None;
                    tab.parent_tab_id = None;
                    tab.report_target_tab_id = None;
                    tab.depth = 0;
                    tab.restorable_launch.worker_restart = None;
                    tab.restorable_launch.context_files = None;
                    tab.restorable_launch.context_skills = None;
                    tab.restorable_launch.identity_instructions = None;
                }
                tab
            })
            .collect())
    }
}

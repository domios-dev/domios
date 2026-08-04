use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, Tab};

/// The one kind carrying machine-coordination authority. It anchors a Team
/// exactly like `orchestrator` does.
pub const GENERAL_AGENT_KIND: &str = codeterm_types::agent::role_kind::builtins::GENERAL_AGENT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineCoordinationCapabilities {
    pub workspaces: bool,
    pub teams: bool,
    pub agent_control: bool,
    pub mesh: bool,
    pub bridges: bool,
    pub cron: bool,
    pub memory: bool,
}

impl MachineCoordinationCapabilities {
    pub const GENERAL_AGENT: Self = Self {
        workspaces: true,
        teams: true,
        agent_control: true,
        mesh: true,
        bridges: true,
        cron: true,
        memory: true,
    };
}

pub fn is_general_agent(tab: &Tab) -> bool {
    tab.role_kind_id() == Some(GENERAL_AGENT_KIND)
}

pub fn capabilities(tab: &Tab) -> Option<MachineCoordinationCapabilities> {
    is_general_agent(tab).then_some(MachineCoordinationCapabilities::GENERAL_AGENT)
}

/// Stamp the General Agent kind onto the machine's General tab.
pub fn apply_general_agent_kind(tab: &mut Tab) {
    tab.role = Some(AgentRole::Manager);
    tab.role_profile = Some(GENERAL_AGENT_KIND.to_string());
    tab.parent_tab_id = None;
    tab.can_spawn = true;
    tab.depth = 1;
}

/// The kind is machine-scoped: carrying it outside the General workspace would
/// mint a second machine coordinator, so strays are demoted and returned.
pub fn confine_general_agent_kind(mux: &mut MultiplexerState) -> Vec<String> {
    let sanctioned = mux.resolve_tab_id(super::GENERAL_ALIAS);
    let general_workspace = sanctioned
        .as_deref()
        .and_then(|tab_id| mux.find_workspace_for_tab(tab_id));
    let strays: Vec<String> = mux
        .snapshot()
        .tabs
        .values()
        .filter(|tab| tab.role_profile.as_deref() == Some(GENERAL_AGENT_KIND))
        .filter(|tab| {
            Some(tab.id.as_str()) != sanctioned.as_deref()
                || general_workspace.is_none()
                || mux.find_workspace_for_tab(&tab.id) != general_workspace
        })
        .map(|tab| tab.id.clone())
        .collect();
    for tab_id in &strays {
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            tab.role = None;
            tab.role_profile = None;
            tab.team_id = None;
            tab.parent_tab_id = None;
            tab.can_spawn = false;
            tab.depth = 0;
        }
        log::warn!("[general-agent] stripped stray general_agent kind from Tab {tab_id}");
    }
    strays
}

#[cfg(test)]
mod tests;

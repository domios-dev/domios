use crate::multiplexer::types::{MachineAgentIdentity, Tab};

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

pub fn capabilities(tab: &Tab) -> Option<MachineCoordinationCapabilities> {
    (tab.machine_agent_identity == Some(MachineAgentIdentity::GeneralAgent))
        .then_some(MachineCoordinationCapabilities::GENERAL_AGENT)
}

#[cfg(test)]
mod tests;

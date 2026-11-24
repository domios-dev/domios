use crate::mesh::MeshManager;
use crate::multiplexer::state::{MultiplexerState, TabCloseReservation};
use crate::service::remote_team::RemoteTeamMember;
use crate::MeshStateHandle;
use std::collections::BTreeMap;

pub struct MeshCloseGrant {
    mesh: MeshStateHandle,
    current: Box<dyn Fn(&MeshManager) -> bool + Send + Sync>,
}

impl MeshCloseGrant {
    pub fn new(
        mesh: MeshStateHandle,
        current: impl Fn(&MeshManager) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            mesh,
            current: Box::new(current),
        }
    }

    pub(crate) fn admit<T>(
        &self,
        reserve: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let mesh = self.mesh.lock();
        if !(self.current)(&mesh) {
            return Err(format!(
                "{}: Peer authorization changed",
                super::tab::REMOTE_TAB_CLOSE_REFUSAL_PREFIX
            ));
        }
        // The caller holds the mux guard before taking the mesh lock.
        reserve()
    }
}

pub enum OwnerTabCloseScope {
    Local,
    Workspace(String),
    Mesh(MeshCloseGrant),
}

#[cfg(test)]
mod tests;

impl OwnerTabCloseScope {
    pub(crate) fn check(&self, mux: &MultiplexerState, tab_id: &str) -> Result<(), String> {
        match self {
            Self::Workspace(workspace)
                if mux.find_workspace_for_tab(tab_id).as_deref() != Some(workspace.as_str()) =>
            {
                Err("Tab not found in this workspace".into())
            }
            Self::Mesh(grant) => grant.admit(|| Ok(())),
            _ => Ok(()),
        }
    }

    pub(crate) fn reserve(
        &self,
        mux: &mut MultiplexerState,
        tab_id: &str,
    ) -> Result<TabCloseReservation, String> {
        if let Self::Mesh(grant) = self {
            return grant.admit(|| mux.reserve_tab_close(tab_id));
        }
        self.check(mux, tab_id)?;
        mux.reserve_tab_close(tab_id)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct CloseTabTarget {
    workspace: Option<String>,
    team: Option<String>,
    parent: Option<String>,
    agent: Option<String>,
    closes_with_owner: bool,
}

impl CloseTabTarget {
    pub(super) fn capture(mux: &MultiplexerState, id: &str) -> Option<Self> {
        let tab = mux.get_tab(id)?;
        Some(Self {
            workspace: mux.find_workspace_for_tab(id),
            team: tab.team_id.clone(),
            parent: tab.parent_tab_id.clone(),
            agent: tab.agent_type.clone(),
            closes_with_owner: tab.closes_with_owner(),
        })
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct TeamCloseSnapshot {
    tabs: BTreeMap<String, CloseTabTarget>,
    pub(super) remote: BTreeMap<String, RemoteTeamMember>,
}

impl TeamCloseSnapshot {
    pub(super) fn capture(
        mux: &MultiplexerState,
        team_id: &str,
        remote: BTreeMap<String, RemoteTeamMember>,
    ) -> Self {
        Self {
            tabs: Self::tabs(mux, team_id),
            remote,
        }
    }

    fn tabs(mux: &MultiplexerState, team_id: &str) -> BTreeMap<String, CloseTabTarget> {
        let mut pending = mux.team_member_tab_ids(team_id);
        let mut tabs = BTreeMap::new();
        while let Some(id) = pending.pop() {
            if tabs.contains_key(&id) {
                continue;
            }
            if let Some(target) = CloseTabTarget::capture(mux, &id) {
                pending.extend(mux.get_children_ids(&id).into_iter().filter(|child| {
                    mux.get_tab(child)
                        .is_some_and(|tab| tab.closes_with_owner())
                }));
                tabs.insert(id, target);
            }
        }
        tabs
    }

    pub fn validate(&self, mux: &MultiplexerState, team_id: &str) -> Result<(), String> {
        if self.tabs != Self::tabs(mux, team_id) {
            return Err(format!(
                "Team {team_id} close targets changed; retry the workspace close"
            ));
        }
        Ok(())
    }
}

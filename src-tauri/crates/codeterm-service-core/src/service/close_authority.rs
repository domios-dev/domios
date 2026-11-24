use crate::mesh::MeshManager;
use crate::multiplexer::state::{MultiplexerState, TabCloseReservation};
use crate::MeshStateHandle;

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

use crate::multiplexer::{state::MultiplexerState, types::TabCloseReason};
use crate::service::{worker_loss::busy_worker_close_warnings, ServiceContext};

pub const REMOTE_TAB_CLOSE_REFUSAL_PREFIX: &str = "remote tab close refused";

fn authorize(
    mux: &MultiplexerState,
    tab_id: &str,
    peer_id: &str,
    controller_tab_id: Option<&str>,
) -> Result<(), String> {
    let tab = mux
        .get_tab(tab_id)
        .ok_or_else(|| format!("Tab not found: {tab_id}"))?;
    let owned = tab.ownership.as_ref().is_some_and(|ownership| {
        ownership.peer_id == peer_id
            && controller_tab_id
                .is_none_or(|caller| ownership.controller_tab_id.as_deref() == Some(caller))
    });
    if owned && !peer_id.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{REMOTE_TAB_CLOSE_REFUSAL_PREFIX}: target must belong to the authenticated remote creator"
        ))
    }
}

impl ServiceContext {
    pub fn close_tab_from_authenticated_mesh_caller(
        &self,
        peer_id: &str,
        controller_tab_id: Option<&str>,
        tab_id: &str,
        force: bool,
    ) -> Result<Vec<String>, String> {
        let tab_id = {
            let mux = self.mux.read();
            let target = mux
                .resolve_tab_id(tab_id)
                .ok_or_else(|| format!("Tab not found: {tab_id}"))?;
            authorize(&mux, &target, peer_id, controller_tab_id)?;
            target
        };
        if self.is_self_close_protected(&tab_id)? {
            return Err(format!(
                "{REMOTE_TAB_CLOSE_REFUSAL_PREFIX}: coordinator tabs require a local caller"
            ));
        }
        let assignments = self
            .in_progress_assignments_for_tab(&tab_id)
            .map_err(|error| error.to_string())?;
        if !force && !assignments.is_empty() {
            return Err(format!(
                "{}: remote Worker has unfinished work; rerun with --force",
                super::BUSY_WORKER_CLOSE_REFUSAL_PREFIX
            ));
        }
        self.close_tab_with_teardown(
            &tab_id,
            TabCloseReason::User,
            |mux, target| authorize(mux, target, peer_id, controller_tab_id),
            || Ok(()),
            |mux, target| authorize(mux, target, peer_id, controller_tab_id),
        )?;
        Ok(busy_worker_close_warnings(&tab_id, &assignments))
    }
}

#[cfg(test)]
mod tests;

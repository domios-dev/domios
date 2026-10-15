use tauri::Manager;

use super::{caller_is_live, SentinelCaller};
use crate::delete::{stage_delete, DeleteCaller, DeleteTarget};
use crate::scheduler::Handle;
use codeterm_service_core::service::ServiceContext;

pub trait SentinelDeleteExt {
    fn blueprint_delete(
        &self,
        caller: &SentinelCaller,
        target: DeleteTarget,
        stop: bool,
    ) -> Result<Vec<String>, String>;
}

impl SentinelDeleteExt for ServiceContext {
    /// Deletes whole Blueprints in the caller's Team; returns the scenario roots removed.
    fn blueprint_delete(
        &self,
        caller: &SentinelCaller,
        target: DeleteTarget,
        stop: bool,
    ) -> Result<Vec<String>, String> {
        let handle = self
            .app_handle
            .try_state::<Handle>()
            .ok_or("blueprints not initialized")?;
        let staged = {
            let mux = self.mux.read();
            if !caller_is_live(&mux, caller) {
                return Err("blueprint caller scope changed".into());
            }
            crate::graph::validate_scope(&mux, &caller.workspace, caller.team.as_deref())?;
            let team_owner = mux
                .get_tab(&caller.tab)
                .is_some_and(|tab| tab.has_team_orchestrator_authority());
            let request = DeleteCaller {
                tab: &caller.tab,
                workspace: &caller.workspace,
                team: caller.team.as_deref(),
                team_owner,
            };
            let mut store = handle.store.lock();
            stage_delete(&mut store, &request, target, stop)?
        };
        let mut deleted = Vec::with_capacity(staged.len());
        let mut failure = None;
        for (root, staged) in staged {
            match staged.settle(&handle.store) {
                Ok(()) => deleted.push(root),
                Err(error) => failure = Some(format!("{root}: {error}")),
            }
        }
        handle.notify();
        match failure {
            Some(error) if deleted.is_empty() => Err(error),
            Some(error) => Err(format!("deleted {} but failed {error}", deleted.join(", "))),
            None => Ok(deleted),
        }
    }
}

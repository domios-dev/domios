use crate::plan::IdentityProvenance;
use crate::service::plan::PlanError;
use crate::service::session_persist::{self, SessionPersistUrgency};
use crate::service::{MuxEmitCause, ServiceContext};

impl ServiceContext {
    pub fn update_task_personality(
        &self,
        plan_id: &str,
        task_id: &str,
        personality: Option<Option<&str>>,
        provenance: Option<Option<IdentityProvenance>>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let title = personality
            .flatten()
            .map(codeterm_types::plan::validate_executor_name)
            .transpose()
            .map_err(PlanError::InvalidExecutorName)?;
        if let Some(title) = &title {
            super::validate_tab_title(title).map_err(PlanError::InvalidExecutorName)?;
        }
        let mut mux = self.mux.write();
        self.plan_store.update_task_personality(
            plan_id,
            task_id,
            personality,
            provenance,
            now_ms,
        )?;
        let Some(title) = title else {
            return Ok(());
        };
        let tabs: Vec<_> = mux
            .list_workspaces()
            .into_iter()
            .flat_map(|workspace| workspace.tab_ids)
            .filter(|id| {
                mux.get_tab(id)
                    .and_then(|tab| tab.task_binding.as_ref())
                    .is_some_and(|binding| {
                        binding.plan_id.as_deref() == Some(plan_id)
                            && binding.task_id.as_deref() == Some(task_id)
                    })
            })
            .collect();
        for id in &tabs {
            mux.rename_tab(id, &title).map_err(PlanError::Io)?;
        }
        drop(mux);
        if !tabs.is_empty() {
            for id in &tabs {
                self.emit_tab_title_pinned(id, &title);
            }
            self.emit_state_changed(MuxEmitCause::Title);
            session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Prompt)
                .map_err(PlanError::Io)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

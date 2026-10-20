//! Any agent tab declares its own checklist; the sidebar learns which tabs hold one.

use super::{now_secs, ServiceContext};
use crate::service::MuxEmitCause;
use codeterm_types::agent::{plan_tab_todo_write, TabTodoChecklist, TabTodoWriteEntry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTodoWriteOutcome {
    pub checklist: TabTodoChecklist,
    pub omitted_locked_items: usize,
}

impl ServiceContext {
    /// Creates the caller's checklist on first write, then reconciles it: matching titles
    /// keep their item, new titles append unlocked items, omitted unlocked items go and
    /// omitted locked items stay.
    pub async fn tab_todo_write(
        &self,
        target_tab_id: &str,
        actor_tab_id: &str,
        entries: Vec<TabTodoWriteEntry>,
    ) -> Result<TabTodoWriteOutcome, String> {
        let target = self.todo_tab(target_tab_id)?;
        if actor_tab_id != target.id {
            return Err("only the owning Tab may write its own checklist".into());
        }
        let store = self.todo_store()?;
        let current = store.get(target.id.clone()).await?;
        let plan = plan_tab_todo_write(current.as_ref(), &entries)?;
        if let (true, Some(checklist)) = (plan.is_noop(), current.clone()) {
            return Ok(TabTodoWriteOutcome {
                checklist,
                omitted_locked_items: plan.omitted_locked_items,
            });
        }
        let mut appended = Vec::with_capacity(plan.appends.len());
        for entry in &plan.appends {
            appended.push(
                self.resolve_draft(uuid::Uuid::new_v4().to_string(), entry.draft())
                    .await?,
            );
        }
        let generation = current.as_ref().map_or(0, |checklist| checklist.generation);
        let checklist = store
            .write_own(
                target.id.clone(),
                target.workspace_id,
                target.team_id,
                now_secs(),
                generation,
                &plan,
                appended.clone(),
            )
            .await?;
        let checklist = self.todo_changed(checklist);
        self.record_task_links(&target.id, &appended)?;
        Ok(TabTodoWriteOutcome {
            checklist,
            omitted_locked_items: plan.omitted_locked_items,
        })
    }

    pub(super) fn sync_checklist_presence(&self, tab_id: &str, present: bool) {
        if self.mux.write().set_tab_has_checklist(tab_id, present) {
            self.emit_state_changed(MuxEmitCause::TabMetadata);
        }
    }
}

/// Marks restored tabs whose stored checklist holds items. Blocks on Storage, so it
/// runs on a startup worker thread, never the async runtime.
pub fn restore_checklist_presence(service: &ServiceContext) {
    let Ok(store) = service.todo_store() else {
        return;
    };
    let tab_ids = match crate::async_runtime::block_on(store.tab_ids_with_items()) {
        Ok(tab_ids) => tab_ids,
        Err(error) => {
            log::warn!("[tab-todo] checklist presence unavailable after restore: {error}");
            return;
        }
    };
    let changed = {
        let mut mux = service.mux.write();
        tab_ids.iter().fold(false, |changed, tab_id| {
            mux.set_tab_has_checklist(tab_id, true) || changed
        })
    };
    if changed {
        service.emit_state_changed(MuxEmitCause::TabMetadata);
    }
}

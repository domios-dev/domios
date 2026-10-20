//! Reviewer reopen, respawn carry-over and the heartbeat step line.

use super::recitation::{bounded_utf8_prefix, step_line};
use super::{now_secs, ServiceContext};
use codeterm_types::agent::{
    validate_tab_todo_reason, TabTodoChecklist, MAX_TAB_TODO_REASON_BYTES,
};

impl ServiceContext {
    /// Orchestrator/QA reopen of one named item; the review-bounce path uses it too.
    /// A user caller (`None`) is allowed as for every other checklist operation.
    pub async fn tab_todo_reopen(
        &self,
        target_tab_id: &str,
        actor_tab_id: Option<&str>,
        item_id: String,
        reason: String,
    ) -> Result<TabTodoChecklist, String> {
        let target = self.todo_tab(target_tab_id)?;
        if let Some(actor) = actor_tab_id {
            self.require_parent_orchestrator(&target, actor)
                .or_else(|_| self.require_qa_sibling(&target, actor))
                .map_err(|_| {
                    "only the target's same-Team Orchestrator or QA Worker may reopen a checklist item"
                        .to_string()
                })?;
        }
        validate_tab_todo_reason(&reason)?;
        let store = self.todo_store()?;
        let checklist = store
            .get(target.id.clone())
            .await?
            .ok_or_else(|| format!("Tab {} has no current checklist", target.id))?;
        store
            .reopen(
                target.id,
                checklist.generation,
                item_id,
                reason,
                actor_tab_id.map(str::to_owned),
                now_secs(),
            )
            .await
            .map(|checklist| self.todo_changed(checklist))
    }

    /// Review bounce: the bounce note becomes the bounded single-line reason.
    pub async fn tab_todo_reopen_for_review(
        &self,
        target_tab_id: &str,
        actor_tab_id: Option<&str>,
        item_id: String,
        note: &str,
    ) -> Result<TabTodoChecklist, String> {
        let single_line: String = note
            .trim()
            .chars()
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .collect();
        let reason = bounded_utf8_prefix(&single_line, MAX_TAB_TODO_REASON_BYTES).to_owned();
        self.tab_todo_reopen(target_tab_id, actor_tab_id, item_id, reason)
            .await
    }

    /// A replacement Worker on the same task inherits the newest predecessor's
    /// checklist with its attempts and history; only without one is it seeded.
    pub async fn tab_todo_inherit_or_seed(
        &self,
        target_tab_id: &str,
        orchestrator_tab_id: &str,
        predecessor_tab_ids: &[String],
        criteria: &[String],
    ) -> Result<Option<TabTodoChecklist>, String> {
        let target = self.todo_tab(target_tab_id)?;
        self.require_parent_orchestrator(&target, orchestrator_tab_id)?;
        let store = self.todo_store()?;
        if let Some(existing) = store.get(target.id.clone()).await? {
            return Ok(Some(existing));
        }
        for source in predecessor_tab_ids.iter().filter(|id| **id != target.id) {
            let carried = store
                .transfer(
                    source.clone(),
                    target.id.clone(),
                    target.workspace_id.clone(),
                    target.team_id.clone(),
                    Some(orchestrator_tab_id.into()),
                    now_secs(),
                )
                .await?;
            if let Some(carried) = carried {
                self.ui.emit(
                    super::TAB_TODO_CHANGED_EVENT,
                    serde_json::json!({
                        "tab_id": source,
                        "generation": carried.generation,
                        "revision": carried.revision,
                    }),
                );
                return Ok(Some(self.todo_changed(carried)));
            }
        }
        if criteria.is_empty() {
            return Ok(None);
        }
        self.tab_todo_seed_from_acceptance(target_tab_id, orchestrator_tab_id, criteria)
            .await
            .map(Some)
    }

    /// Blocks on Storage; callers must already run off the async runtime.
    pub(crate) fn tab_todo_step_line_blocking(&self, tab_id: &str) -> Option<String> {
        let recitation = crate::async_runtime::block_on(self.tab_todo_recite(tab_id, Some(tab_id)))
            .ok()
            .flatten()?;
        step_line(&recitation)
    }
}

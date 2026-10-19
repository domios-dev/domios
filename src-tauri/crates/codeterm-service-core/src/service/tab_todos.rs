//! Authorization and bounded operations for tab-owned checklist records.

use crate::multiplexer::types::AgentRole;
use crate::service::ServiceContext;
use crate::startup::StorageAuthority;
use codeterm_agent_model::tab_todo::TabTodoStore;
use codeterm_types::agent::{
    TabTodoChecklist, TabTodoDraft, TabTodoLink, TabTodoLinkDraft, TabTodoNewItem,
    TabTodoRecitation, TabTodoRecitedItem, TabTodoState, TabTodoSummary, MAX_TAB_TODO_ITEMS,
    MAX_TAB_TODO_SUMMARY_BYTES, MAX_TAB_TODO_SUMMARY_TITLES,
};
use std::time::{SystemTime, UNIX_EPOCH};

/// Permanently remove todo rows for tabs evicted from closed history.
///
/// Ordinary close deliberately does not call this: the records remain available
/// while the tab can still be restored from Recently Closed.
pub fn forget_permanently_closed_todos(storage: crate::storage::Storage, tab_ids: Vec<String>) {
    if tab_ids.is_empty() {
        return;
    }
    let mut tab_ids = tab_ids;
    tab_ids.sort_unstable();
    tab_ids.dedup();
    crate::async_runtime::spawn(async move {
        let store = TabTodoStore::new(storage.tab_todos());
        for tab_id in tab_ids {
            if let Err(error) = store.forget_tab(tab_id.clone()).await {
                log::warn!("[tab-todo] failed to remove permanently closed Tab {tab_id}: {error}");
            }
        }
    });
}

#[derive(Clone)]
struct TabIdentity {
    id: String,
    team_id: Option<String>,
    parent_tab_id: Option<String>,
    role: Option<AgentRole>,
    role_profile: Option<String>,
    workspace_id: String,
    restored_from_snapshot: bool,
}

impl ServiceContext {
    fn todo_store(&self) -> Result<TabTodoStore, String> {
        let storage = self
            .try_ext::<StorageAuthority>()
            .and_then(StorageAuthority::storage)
            .ok_or_else(|| "shared Storage is unavailable".to_string())?;
        Ok(TabTodoStore::new(storage.tab_todos()))
    }

    pub(crate) fn forget_evicted_tab_todos(&self, tab_ids: Vec<String>) {
        let Some(storage) = self
            .try_ext::<StorageAuthority>()
            .and_then(StorageAuthority::storage)
            .cloned()
        else {
            return;
        };
        forget_permanently_closed_todos(storage, tab_ids);
    }

    fn todo_tab(&self, tab_id: &str) -> Result<TabIdentity, String> {
        let mux = self.mux.read();
        let id = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| format!("Tab not found: {tab_id}"))?;
        let tab = mux
            .get_tab(&id)
            .ok_or_else(|| format!("Tab not found: {tab_id}"))?;
        let workspace_id = mux
            .find_workspace_for_tab(&id)
            .ok_or_else(|| format!("Tab {id} has no live workspace"))?;
        Ok(TabIdentity {
            id,
            team_id: tab.team_id.clone(),
            parent_tab_id: tab.parent_tab_id.clone(),
            role: tab.role,
            role_profile: tab.role_profile.clone(),
            workspace_id,
            restored_from_snapshot: tab.restored_from_snapshot,
        })
    }

    fn require_owner_or_user(
        &self,
        target: &TabIdentity,
        actor: Option<&str>,
    ) -> Result<(), String> {
        match actor {
            None => Ok(()),
            Some(actor) if actor == target.id => Ok(()),
            Some(_) => Err("only the owning Tab or user may read this checklist".into()),
        }
    }

    fn require_parent_orchestrator(&self, target: &TabIdentity, actor: &str) -> Result<(), String> {
        let parent = self.todo_tab(actor)?;
        if target.parent_tab_id.as_deref() != Some(actor)
            || parent.role != Some(AgentRole::Manager)
            || parent.role_profile.as_deref() != Some("orchestrator")
            || target.team_id != parent.team_id
        {
            return Err(
                "only the target's same-Team Orchestrator may perform this operation".into(),
            );
        }
        Ok(())
    }

    fn require_qa_sibling(&self, target: &TabIdentity, actor: &str) -> Result<(), String> {
        let qa = self.todo_tab(actor)?;
        if qa.role != Some(AgentRole::Worker)
            || qa.role_profile.as_deref() != Some("qa")
            || qa.team_id != target.team_id
            || qa.parent_tab_id != target.parent_tab_id
        {
            return Err("only a QA Worker in the target's Team may append ordered checks".into());
        }
        Ok(())
    }

    /// Seed the locked checklist from task acceptance criteria at dispatch.
    /// Items are ordered as declared; title truncations are recorded in audit.
    pub async fn tab_todo_seed_from_acceptance(
        &self,
        target_tab_id: &str,
        orchestrator_tab_id: &str,
        criteria: &[String],
    ) -> Result<TabTodoChecklist, String> {
        let target = self.todo_tab(target_tab_id)?;
        self.require_parent_orchestrator(&target, orchestrator_tab_id)?;
        if criteria.is_empty() || criteria.len() > MAX_TAB_TODO_ITEMS {
            return Err(format!(
                "acceptance criteria must contain 1..={MAX_TAB_TODO_ITEMS} items"
            ));
        }
        let mut items = Vec::with_capacity(criteria.len());
        let mut truncated_ids = Vec::new();
        for criterion in criteria {
            let title = criterion.trim();
            if title.is_empty() {
                return Err("acceptance criteria cannot contain an empty item".into());
            }
            let (title, truncated) = bounded_title(title);
            let id = uuid::Uuid::new_v4().to_string();
            if truncated {
                truncated_ids.push(id.clone());
            }
            items.push(TabTodoNewItem {
                id,
                title,
                description: None,
                link: None,
            });
        }
        let store = self.todo_store()?;
        store
            .create(
                target.id,
                target.workspace_id,
                target.team_id,
                now_secs(),
                items,
                truncated_ids,
            )
            .await
    }

    pub async fn tab_todo_append_qa(
        &self,
        target_tab_id: &str,
        actor_tab_id: &str,
        generation: u64,
        drafts: Vec<TabTodoDraft>,
    ) -> Result<TabTodoChecklist, String> {
        let target = self.todo_tab(target_tab_id)?;
        self.require_qa_sibling(&target, actor_tab_id)?;
        let (items, _) = self.resolve_drafts(drafts).await?;
        self.todo_store()?
            .append(
                target.id,
                generation,
                Some(actor_tab_id.into()),
                now_secs(),
                items,
            )
            .await
    }

    pub async fn tab_todo_set_state(
        &self,
        target_tab_id: &str,
        actor_tab_id: Option<&str>,
        generation: u64,
        item_id: String,
        expected_revision: u64,
        state: TabTodoState,
    ) -> Result<TabTodoChecklist, String> {
        let target = self.todo_tab(target_tab_id)?;
        self.require_owner_or_user(&target, actor_tab_id)?;
        self.todo_store()?
            .set_state(
                target.id,
                generation,
                item_id,
                expected_revision,
                state,
                actor_tab_id.map(str::to_owned),
                now_secs(),
            )
            .await
    }

    pub async fn tab_todo_amend(
        &self,
        target_tab_id: &str,
        actor_tab_id: &str,
        generation: u64,
        item_id: String,
        expected_revision: u64,
        draft: TabTodoDraft,
    ) -> Result<TabTodoChecklist, String> {
        let target = self.todo_tab(target_tab_id)?;
        self.require_parent_orchestrator(&target, actor_tab_id)?;
        draft.validate()?;
        let item = self.resolve_draft(item_id, draft).await?;
        let checklist = self
            .todo_store()?
            .amend(
                target.id,
                generation,
                item.id.clone(),
                expected_revision,
                item.clone(),
                Some(actor_tab_id.into()),
                now_secs(),
            )
            .await?;
        self.record_task_links(actor_tab_id, std::slice::from_ref(&item))?;
        Ok(checklist)
    }

    pub async fn tab_todo_clear_one(
        &self,
        target_tab_id: &str,
        actor_tab_id: Option<&str>,
        generation: u64,
        item_id: String,
        expected_revision: u64,
    ) -> Result<TabTodoChecklist, String> {
        require_human(actor_tab_id)?;
        let target = self.todo_tab(target_tab_id)?;
        self.todo_store()?
            .clear_one(
                target.id,
                generation,
                item_id,
                expected_revision,
                None,
                now_secs(),
            )
            .await
    }

    pub async fn tab_todo_clear_all(
        &self,
        target_tab_id: &str,
        actor_tab_id: Option<&str>,
        generation: u64,
    ) -> Result<TabTodoChecklist, String> {
        require_human(actor_tab_id)?;
        let target = self.todo_tab(target_tab_id)?;
        self.todo_store()?
            .clear_all(target.id, generation, None, now_secs())
            .await
    }

    /// User reset/rewrite creates a new generation; prior item rows are removed
    /// by the repository while its audit entries are retained.
    pub async fn tab_todo_reset(
        &self,
        target_tab_id: &str,
        actor_tab_id: Option<&str>,
        generation: u64,
        drafts: Vec<TabTodoDraft>,
    ) -> Result<TabTodoChecklist, String> {
        require_human(actor_tab_id)?;
        let target = self.todo_tab(target_tab_id)?;
        let event_tab_id = target.id.clone();
        let (items, _) = self.resolve_drafts(drafts).await?;
        let checklist = self
            .todo_store()?
            .reset(
                target.id,
                generation,
                target.workspace_id,
                target.team_id,
                now_secs(),
                None,
                items.clone(),
            )
            .await?;
        self.record_task_links(&event_tab_id, &items)?;
        Ok(checklist)
    }

    pub async fn tab_todo_list(
        &self,
        target_tab_id: &str,
        actor_tab_id: Option<&str>,
    ) -> Result<Option<TabTodoChecklist>, String> {
        let target = self.todo_tab(target_tab_id)?;
        self.require_owner_or_user(&target, actor_tab_id)?;
        self.todo_store()?.get(target.id).await
    }

    pub async fn tab_todo_parent_summary(
        &self,
        target_tab_id: &str,
        parent_tab_id: &str,
    ) -> Result<Option<TabTodoSummary>, String> {
        let target = self.todo_tab(target_tab_id)?;
        self.require_parent_orchestrator(&target, parent_tab_id)?;
        let Some(checklist) = self.todo_store()?.get(target.id.clone()).await? else {
            return Ok(None);
        };
        Ok(Some(summary(&checklist)))
    }

    /// Full bounded checklist inspection for the target's same-Team Orchestrator.
    /// The store caps current-generation checklists at `MAX_TAB_TODO_ITEMS`.
    pub async fn tab_todo_parent_list(
        &self,
        target_tab_id: &str,
        parent_tab_id: &str,
    ) -> Result<Option<TabTodoChecklist>, String> {
        let target = self.todo_tab(target_tab_id)?;
        self.require_parent_orchestrator(&target, parent_tab_id)?;
        self.todo_store()?.get(target.id).await
    }

    /// Bounded event recitation for the owning agent. Open/in-progress items are
    /// ordered before completed items; at most the 16 persisted items are emitted.
    pub async fn tab_todo_recite(
        &self,
        target_tab_id: &str,
        actor_tab_id: Option<&str>,
    ) -> Result<Option<TabTodoRecitation>, String> {
        let target = self.todo_tab(target_tab_id)?;
        self.require_owner_or_user(&target, actor_tab_id)?;
        if !self.team_is_live(target.team_id.as_deref()) {
            return Ok(None);
        }
        let Some(checklist) = self.todo_store()?.get(target.id.clone()).await? else {
            return Ok(None);
        };
        let mut items: Vec<_> = checklist.items.iter().collect();
        items.sort_by_key(|item| {
            (
                match item.state {
                    TabTodoState::Open => 0,
                    TabTodoState::InProgress => 1,
                    TabTodoState::Done => 2,
                },
                item.position,
            )
        });
        Ok(Some(TabTodoRecitation {
            tab_id: checklist.tab_id,
            generation: checklist.generation,
            revision: checklist.revision,
            items: items
                .into_iter()
                .take(MAX_TAB_TODO_ITEMS)
                .map(|item| TabTodoRecitedItem {
                    id: item.id.clone(),
                    title: item.title.clone(),
                    state: item.state,
                })
                .collect(),
        }))
    }

    /// Event owners call this on compaction, session rebind/resume, respawn, or a
    /// completion report with unfinished locked steps. It uses the existing
    /// automated reminder envelope and never schedules follow-up work.
    pub async fn deliver_tab_todo_recitation(&self, target_tab_id: &str) -> Result<bool, String> {
        let initial_target = match self.todo_tab(target_tab_id) {
            Ok(target) => target,
            Err(_) => return Ok(false),
        };
        let Some(recitation) = self
            .tab_todo_recite(target_tab_id, Some(target_tab_id))
            .await?
        else {
            return Ok(false);
        };
        if recitation
            .items
            .iter()
            .all(|item| item.state == TabTodoState::Done)
        {
            return Ok(false);
        }
        let live_target = match self.todo_tab(&recitation.tab_id) {
            Ok(target) => target,
            Err(_) => return Ok(false),
        };
        if !self.team_is_live(live_target.team_id.as_deref()) {
            return Ok(false);
        }
        if initial_target.restored_from_snapshot != live_target.restored_from_snapshot {
            return Ok(false);
        }
        let Some(current) = self.todo_store()?.get(recitation.tab_id.clone()).await? else {
            return Ok(false);
        };
        if current.generation != recitation.generation
            || current.revision != recitation.revision
            || current
                .items
                .iter()
                .all(|item| item.state == TabTodoState::Done)
        {
            return Ok(false);
        }
        let text = recitation_text(&recitation);
        crate::service::hierarchy::deliver_tab_todo_recitation(self, target_tab_id, &text)
    }

    async fn resolve_drafts(
        &self,
        drafts: Vec<TabTodoDraft>,
    ) -> Result<(Vec<TabTodoNewItem>, Vec<String>), String> {
        if drafts.is_empty() || drafts.len() > MAX_TAB_TODO_ITEMS {
            return Err(format!(
                "todo items must contain 1..={MAX_TAB_TODO_ITEMS} items"
            ));
        }
        let mut items = Vec::with_capacity(drafts.len());
        for draft in drafts {
            draft.validate()?;
            let id = uuid::Uuid::new_v4().to_string();
            items.push(self.resolve_draft(id, draft).await?);
        }
        Ok((items, Vec::new()))
    }

    async fn resolve_draft(
        &self,
        id: String,
        draft: TabTodoDraft,
    ) -> Result<TabTodoNewItem, String> {
        draft.validate()?;
        let link = match draft.link {
            None => None,
            Some(TabTodoLinkDraft::Url { url, label }) => {
                validate_external_url(&url)?;
                Some(TabTodoLink::Url { url, label })
            }
            Some(TabTodoLinkDraft::DomiosTask {
                task_ref,
                plan_ref,
                label,
            }) => {
                let resolved = self
                    .plan_store
                    .resolve_task_ref(plan_ref.as_deref(), &task_ref)
                    .map_err(|error| format!("unable to resolve linked task: {error}"))?;
                let plan = self
                    .plan_store
                    .get(&resolved.plan_id)
                    .ok_or_else(|| "resolved plan was not found".to_string())?;
                let task = plan
                    .tasks
                    .iter()
                    .find(|task| task.id == resolved.task_id)
                    .ok_or_else(|| "resolved task was not found".to_string())?;
                Some(TabTodoLink::DomiosTask {
                    plan_id: resolved.plan_id,
                    task_id: resolved.task_id,
                    label: label.unwrap_or_else(|| bounded_title(&task.name).0),
                })
            }
        };
        Ok(TabTodoNewItem {
            id,
            title: draft.title,
            description: draft.description,
            link,
        })
    }

    fn record_task_links(
        &self,
        actor_tab_id: &str,
        items: &[TabTodoNewItem],
    ) -> Result<(), String> {
        let now_ms = now_secs().saturating_mul(1000);
        for item in items {
            if let Some(TabTodoLink::DomiosTask {
                plan_id,
                task_id,
                label,
            }) = &item.link
            {
                self.plan_store
                    .record_tab_todo_linked(plan_id, task_id, &item.id, label, actor_tab_id, now_ms)
                    .map_err(|error| format!("unable to audit linked task: {error}"))?;
            }
        }
        Ok(())
    }

    fn team_is_live(&self, team_id: Option<&str>) -> bool {
        let Some(team_id) = team_id else {
            return true;
        };
        self.mux
            .read()
            .list_teams()
            .iter()
            .any(|team| team.id == team_id)
    }
}

fn require_human(actor_tab_id: Option<&str>) -> Result<(), String> {
    if actor_tab_id.is_none() {
        Ok(())
    } else {
        Err("this operation is reserved for the user".into())
    }
}

fn bounded_title(text: &str) -> (String, bool) {
    let mut chars = text.chars();
    let title: String = chars.by_ref().take(120).collect();
    let truncated = chars.next().is_some();
    (title, truncated)
}

fn validate_external_url(value: &str) -> Result<(), String> {
    let parsed = url::Url::parse(value).map_err(|error| format!("invalid todo URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("todo URL must be an absolute HTTP or HTTPS URL".into());
    }
    Ok(())
}

fn summary(checklist: &TabTodoChecklist) -> TabTodoSummary {
    let unfinished: Vec<_> = checklist
        .items
        .iter()
        .filter(|item| item.state != TabTodoState::Done)
        .collect();
    let mut titles = Vec::new();
    let mut used = 0usize;
    for item in unfinished.iter().take(MAX_TAB_TODO_SUMMARY_TITLES) {
        let cost = item.title.len() + usize::from(!titles.is_empty());
        if used + cost > MAX_TAB_TODO_SUMMARY_BYTES {
            break;
        }
        used += cost;
        titles.push(item.title.clone());
    }
    TabTodoSummary {
        tab_id: checklist.tab_id.clone(),
        generation: checklist.generation,
        unfinished_count: unfinished.len().min(MAX_TAB_TODO_ITEMS) as u8,
        titles,
    }
}

fn recitation_text(recitation: &TabTodoRecitation) -> String {
    let unfinished: Vec<_> = recitation
        .items
        .iter()
        .filter(|item| item.state != TabTodoState::Done)
        .collect();
    let mut text = format!("Checklist: {} unfinished item(s)", unfinished.len());
    for item in unfinished.into_iter().take(MAX_TAB_TODO_SUMMARY_TITLES) {
        let suffix = format!("\n- {}", item.title);
        let remaining = MAX_TAB_TODO_SUMMARY_BYTES.saturating_sub(text.len());
        if remaining == 0 {
            break;
        }
        text.push_str(&bounded_utf8_prefix(&suffix, remaining));
    }
    text
}

fn bounded_utf8_prefix(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests;

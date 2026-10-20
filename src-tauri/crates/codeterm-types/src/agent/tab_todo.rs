use serde::{Deserialize, Serialize};

mod progress;
mod write;

pub use progress::{
    tab_todo_bound_refusal, validate_tab_todo_reason, TabTodoAttemptSpan, TabTodoCounters,
    TabTodoEventKind, TabTodoItemEvent, MAX_TAB_TODO_ITEM_EVENTS, MAX_TAB_TODO_REASON_BYTES,
};
pub use write::{
    plan_tab_todo_write, TabTodoItemRef, TabTodoWriteBase, TabTodoWriteEntry, TabTodoWritePlan,
    TabTodoWriteStateChange, TabTodoWriteTarget,
};

pub const MAX_TAB_TODO_ITEMS: usize = 12;
pub const MAX_TAB_TODO_TITLE_CHARS: usize = 120;
pub const MAX_TAB_TODO_DESCRIPTION_BYTES: usize = 512;
pub const MAX_TAB_TODO_URL_BYTES: usize = 2048;
pub const MAX_TAB_TODO_SUMMARY_BYTES: usize = 512;
pub const MAX_TAB_TODO_SUMMARY_TITLES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TabTodoLinkDraft {
    DomiosTask {
        task_ref: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan_ref: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    Url {
        url: String,
        label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TabTodoLink {
    DomiosTask {
        plan_id: String,
        task_id: String,
        label: String,
    },
    Url {
        url: String,
        label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoDraft {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<TabTodoLinkDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoNewItem {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<TabTodoLink>,
}

impl TabTodoDraft {
    pub fn validate(&self) -> Result<(), String> {
        let title_chars = self.title.chars().count();
        if self.title.trim().is_empty() || title_chars > MAX_TAB_TODO_TITLE_CHARS {
            return Err(format!(
                "todo title must contain 1..={MAX_TAB_TODO_TITLE_CHARS} characters"
            ));
        }
        if self.title.chars().any(char::is_control) {
            return Err("todo title must not contain control characters".into());
        }
        if self
            .description
            .as_ref()
            .is_some_and(|description| description.len() > MAX_TAB_TODO_DESCRIPTION_BYTES)
        {
            return Err(format!(
                "todo description exceeds {MAX_TAB_TODO_DESCRIPTION_BYTES} bytes"
            ));
        }
        if let Some(link) = &self.link {
            match link {
                TabTodoLinkDraft::DomiosTask {
                    task_ref,
                    plan_ref,
                    label,
                } => {
                    bounded_reference("todo task reference", task_ref)?;
                    if let Some(plan_ref) = plan_ref {
                        bounded_reference("todo plan reference", plan_ref)?;
                    }
                    if let Some(label) = label {
                        validate_label(label)?;
                    }
                }
                TabTodoLinkDraft::Url { url, label } => {
                    if url.is_empty() || url.len() > MAX_TAB_TODO_URL_BYTES {
                        return Err(format!(
                            "todo URL must contain 1..={MAX_TAB_TODO_URL_BYTES} bytes"
                        ));
                    }
                    validate_label(label)?;
                }
            }
        }
        Ok(())
    }
}

fn bounded_reference(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(format!(
            "{field} must contain 1..=256 bytes without controls"
        ));
    }
    Ok(())
}

fn validate_label(label: &str) -> Result<(), String> {
    let chars = label.chars().count();
    if label.trim().is_empty()
        || chars > MAX_TAB_TODO_TITLE_CHARS
        || label.chars().any(char::is_control)
    {
        return Err(format!(
            "todo link label must contain 1..={MAX_TAB_TODO_TITLE_CHARS} characters without controls"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabTodoState {
    Open,
    InProgress,
    Done,
    Blocked,
    Skipped,
}

impl TabTodoState {
    /// Done and skipped items close the checklist; blocked items keep it open.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Skipped)
    }

    pub fn requires_reason(self) -> bool {
        matches!(self, Self::Blocked | Self::Skipped)
    }

    /// Recitation order: blocked work first, then the current step, then pending.
    pub fn recite_rank(self) -> u8 {
        match self {
            Self::Blocked => 0,
            Self::InProgress => 1,
            Self::Open => 2,
            Self::Skipped => 3,
            Self::Done => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabTodoChecklistStatus {
    Active,
    Complete,
    Cleared,
    Closed,
    Reset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoItem {
    pub id: String,
    pub position: u8,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub state: TabTodoState,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<TabTodoLink>,
    /// Locked items come from dispatch criteria or QA checks and are never deleted.
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub attempt: u32,
    #[serde(default)]
    pub counters: TabTodoCounters,
    /// Newest `MAX_TAB_TODO_ITEM_EVENTS` events, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<TabTodoItemEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoChecklist {
    pub tab_id: String,
    pub generation: u64,
    pub started_at: u64,
    pub revision: u64,
    pub status: TabTodoChecklistStatus,
    pub items: Vec<TabTodoItem>,
}

impl TabTodoChecklist {
    pub fn recite_order(&self) -> Vec<&TabTodoItem> {
        let mut items: Vec<_> = self.items.iter().collect();
        items.sort_by_key(|item| (item.state.recite_rank(), item.position));
        items
    }

    /// Unfinished items in recitation order; bounded by the checklist item cap.
    pub fn remaining(&self) -> Vec<TabTodoRecitedItem> {
        self.recite_order()
            .into_iter()
            .filter(|item| !item.state.is_terminal())
            .take(MAX_TAB_TODO_ITEMS)
            .map(TabTodoRecitedItem::from)
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoSummary {
    pub tab_id: String,
    pub generation: u64,
    pub unfinished_count: u8,
    #[serde(default)]
    pub blocked_count: u8,
    #[serde(default)]
    pub skipped_count: u8,
    pub titles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoRecitedItem {
    pub id: String,
    pub title: String,
    pub state: TabTodoState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub attempt: u32,
}

impl From<&TabTodoItem> for TabTodoRecitedItem {
    fn from(item: &TabTodoItem) -> Self {
        Self {
            id: item.id.clone(),
            title: item.title.clone(),
            state: item.state,
            reason: item.reason.clone(),
            attempt: item.attempt,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoRecitation {
    pub tab_id: String,
    pub generation: u64,
    pub revision: u64,
    pub items: Vec<TabTodoRecitedItem>,
}

#[cfg(test)]
mod tests;

use super::{TabTodoItem, MAX_TAB_TODO_ITEMS};
use serde::{Deserialize, Serialize};

pub const MAX_TAB_TODO_ITEM_EVENTS: usize = 32;
pub const MAX_TAB_TODO_REASON_BYTES: usize = 512;

/// Refusal for a checklist write beyond the item cap; the remedy is a task split.
pub fn tab_todo_bound_refusal() -> String {
    format!(
        "a tab checklist holds at most {MAX_TAB_TODO_ITEMS} items; this work is too large for one task: propose a task split in your report instead of growing the list"
    )
}

pub fn validate_tab_todo_reason(reason: &str) -> Result<(), String> {
    if reason.trim().is_empty()
        || reason.len() > MAX_TAB_TODO_REASON_BYTES
        || reason.chars().any(char::is_control)
    {
        return Err(format!(
            "todo reason must contain 1..={MAX_TAB_TODO_REASON_BYTES} bytes without controls"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabTodoEventKind {
    Started,
    Completed,
    Reopened,
    Blocked,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoItemEvent {
    pub kind: TabTodoEventKind,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub at: u64,
}

/// Lifetime per-item counts; trimmed history events stay counted here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoCounters {
    pub started: u32,
    pub completed: u32,
    pub reopened: u32,
    pub blocked: u32,
    pub skipped: u32,
    pub folded_events: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabTodoAttemptSpan {
    pub attempt: u32,
    pub started_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u64>,
}

impl TabTodoItem {
    /// Attempts whose first start is still in the retained history.
    pub fn attempt_spans(&self) -> Vec<TabTodoAttemptSpan> {
        let mut spans: Vec<TabTodoAttemptSpan> = Vec::new();
        for event in &self.history {
            if event.attempt == 0 {
                continue;
            }
            let existing = spans.iter_mut().find(|span| span.attempt == event.attempt);
            match (event.kind, existing) {
                (TabTodoEventKind::Started, None) => spans.push(TabTodoAttemptSpan {
                    attempt: event.attempt,
                    started_at: event.at,
                    finished_at: None,
                    duration_secs: None,
                }),
                (
                    TabTodoEventKind::Completed
                    | TabTodoEventKind::Skipped
                    | TabTodoEventKind::Reopened,
                    Some(span),
                ) if span.finished_at.is_none() => {
                    span.finished_at = Some(event.at);
                    span.duration_secs = Some(event.at.saturating_sub(span.started_at));
                }
                _ => {}
            }
        }
        spans
    }
}

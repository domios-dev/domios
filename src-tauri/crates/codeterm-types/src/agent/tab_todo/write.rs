//! An agent declares its whole checklist; Domios plans the change against the stored one.

use super::{
    tab_todo_bound_refusal, validate_tab_todo_reason, TabTodoChecklist, TabTodoDraft, TabTodoItem,
    TabTodoLink, TabTodoLinkDraft, TabTodoState, MAX_TAB_TODO_ITEMS, MAX_TAB_TODO_TITLE_CHARS,
    MAX_TAB_TODO_URL_BYTES,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabTodoWriteEntry {
    pub content: String,
    pub state: TabTodoState,
    /// Existing items: must match the current label, URL or task id. New items: a URL or task reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl TabTodoWriteEntry {
    pub fn validate(&self) -> Result<(), String> {
        if !bounded_text(&self.content, MAX_TAB_TODO_TITLE_CHARS) {
            return Err(format!(
                "todo content must contain 1..={MAX_TAB_TODO_TITLE_CHARS} characters without controls"
            ));
        }
        if let Some(link) = &self.link {
            if link.trim().is_empty()
                || link.len() > MAX_TAB_TODO_URL_BYTES
                || link.chars().any(char::is_control)
            {
                return Err(format!(
                    "todo link for {} must contain 1..={MAX_TAB_TODO_URL_BYTES} bytes without controls",
                    self.content
                ));
            }
        }
        match self.reason.as_deref() {
            Some(reason) => validate_tab_todo_reason(reason),
            None if self.state.requires_reason() => Err(format!(
                "todo write needs a reason for {}: blocked and skipped items explain why",
                self.content
            )),
            None => Ok(()),
        }
    }

    /// The unlocked draft a new entry becomes: an http(s) link is a URL, any other a Domios task.
    pub fn draft(&self) -> TabTodoDraft {
        let link = self.link.as_ref().map(|link| {
            if link.starts_with("https://") || link.starts_with("http://") {
                TabTodoLinkDraft::Url {
                    url: link.clone(),
                    label: self.content.clone(),
                }
            } else {
                TabTodoLinkDraft::DomiosTask {
                    task_ref: link.clone(),
                    plan_ref: None,
                    label: None,
                }
            }
        });
        TabTodoDraft {
            title: self.content.clone(),
            description: None,
            link,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTodoItemRef {
    pub item_id: String,
    pub expected_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabTodoWriteTarget {
    Existing(TabTodoItemRef),
    /// Index into `TabTodoWritePlan::appends`; the item starts at revision 1.
    Appended(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTodoWriteStateChange {
    pub target: TabTodoWriteTarget,
    pub state: TabTodoState,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabTodoWriteBase {
    /// Nothing survives from the stored list: a new generation holds exactly the appends.
    Fresh,
    /// Every stored item is removed and nothing is written: the generation is cleared.
    Clear,
    /// The stored generation stays; removals and appends edit it in place.
    Edit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTodoWritePlan {
    pub base: TabTodoWriteBase,
    pub removals: Vec<TabTodoItemRef>,
    pub appends: Vec<TabTodoWriteEntry>,
    /// Demotions come before the single promotion to in progress.
    pub state_changes: Vec<TabTodoWriteStateChange>,
    pub omitted_locked_items: usize,
}

impl TabTodoWritePlan {
    pub fn is_noop(&self) -> bool {
        self.base == TabTodoWriteBase::Edit
            && self.removals.is_empty()
            && self.appends.is_empty()
            && self.state_changes.is_empty()
    }
}

/// Reconciles the declared list with the stored one: matching titles keep their item and
/// history, new titles append unlocked items, omitted unlocked items are removed and
/// omitted locked items stay as they are.
pub fn plan_tab_todo_write(
    current: Option<&TabTodoChecklist>,
    entries: &[TabTodoWriteEntry],
) -> Result<TabTodoWritePlan, String> {
    if entries.len() > MAX_TAB_TODO_ITEMS {
        return Err(tab_todo_bound_refusal());
    }
    let mut contents = HashSet::with_capacity(entries.len());
    for entry in entries {
        entry.validate()?;
        if !contents.insert(entry.content.as_str()) {
            return Err(format!("todo write repeats content: {}", entry.content));
        }
    }
    let requested: Vec<&str> = entries
        .iter()
        .filter(|entry| entry.state == TabTodoState::InProgress)
        .map(|entry| entry.content.as_str())
        .collect();
    if requested.len() > 1 {
        return Err(format!(
            "todo write accepts at most one in_progress item; in_progress: {}",
            requested.join(", ")
        ));
    }

    let Some(checklist) = current.filter(|checklist| !checklist.items.is_empty()) else {
        if entries.is_empty() {
            return Err("todo write needs at least one item to create a checklist".into());
        }
        return Ok(fresh_plan(entries));
    };

    let mut matched = HashSet::new();
    let mut appends = Vec::new();
    let mut state_changes = Vec::new();
    for entry in entries {
        let matches: Vec<&TabTodoItem> = checklist
            .items
            .iter()
            .filter(|item| item.title == entry.content)
            .collect();
        match matches.as_slice() {
            [] => {
                if entry.state != TabTodoState::Open {
                    state_changes.push(change(TabTodoWriteTarget::Appended(appends.len()), entry));
                }
                appends.push(entry.clone());
            }
            [item] => {
                if entry
                    .link
                    .as_deref()
                    .is_some_and(|link| !item.link_matches(link))
                {
                    return Err(format!(
                        "todo write cannot change the link of {}",
                        item.title
                    ));
                }
                matched.insert(item.id.as_str());
                let reason_changed = entry.state.requires_reason() && item.reason != entry.reason;
                if item.state != entry.state || reason_changed {
                    state_changes.push(change(
                        TabTodoWriteTarget::Existing(TabTodoItemRef {
                            item_id: item.id.clone(),
                            expected_revision: item.revision,
                        }),
                        entry,
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "todo write content is ambiguous: {}",
                    entry.content
                ))
            }
        }
    }

    let omitted: Vec<&TabTodoItem> = checklist
        .items
        .iter()
        .filter(|item| !matched.contains(item.id.as_str()))
        .collect();
    let kept_locked: Vec<&TabTodoItem> =
        omitted.iter().copied().filter(|item| item.locked).collect();
    let removals: Vec<TabTodoItemRef> = omitted
        .iter()
        .filter(|item| !item.locked)
        .map(|item| TabTodoItemRef {
            item_id: item.id.clone(),
            expected_revision: item.revision,
        })
        .collect();

    let final_count = matched.len() + kept_locked.len() + appends.len();
    if final_count > MAX_TAB_TODO_ITEMS {
        return Err(tab_todo_bound_refusal());
    }
    let in_progress: Vec<&str> = kept_locked
        .iter()
        .filter(|item| item.state == TabTodoState::InProgress)
        .map(|item| item.title.as_str())
        .chain(requested.iter().copied())
        .collect();
    if in_progress.len() > 1 {
        return Err(format!(
            "todo write would leave more than one in_progress item: {}",
            in_progress.join(", ")
        ));
    }

    if matched.is_empty() && kept_locked.is_empty() {
        return Ok(if entries.is_empty() {
            TabTodoWritePlan {
                base: TabTodoWriteBase::Clear,
                removals,
                appends: Vec::new(),
                state_changes: Vec::new(),
                omitted_locked_items: 0,
            }
        } else {
            fresh_plan(entries)
        });
    }
    prioritize(&mut state_changes);
    Ok(TabTodoWritePlan {
        base: TabTodoWriteBase::Edit,
        removals,
        appends,
        state_changes,
        omitted_locked_items: kept_locked.len(),
    })
}

fn fresh_plan(entries: &[TabTodoWriteEntry]) -> TabTodoWritePlan {
    let mut state_changes: Vec<_> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.state != TabTodoState::Open)
        .map(|(index, entry)| change(TabTodoWriteTarget::Appended(index), entry))
        .collect();
    prioritize(&mut state_changes);
    TabTodoWritePlan {
        base: TabTodoWriteBase::Fresh,
        removals: Vec::new(),
        appends: entries.to_vec(),
        state_changes,
        omitted_locked_items: 0,
    }
}

fn change(target: TabTodoWriteTarget, entry: &TabTodoWriteEntry) -> TabTodoWriteStateChange {
    TabTodoWriteStateChange {
        target,
        state: entry.state,
        reason: entry.reason.clone(),
    }
}

fn prioritize(changes: &mut [TabTodoWriteStateChange]) {
    changes.sort_by_key(|change| change.state == TabTodoState::InProgress);
}

fn bounded_text(value: &str, max_chars: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= max_chars
        && !value.chars().any(char::is_control)
}

impl TabTodoItem {
    pub fn link_matches(&self, requested: &str) -> bool {
        match self.link.as_ref() {
            Some(TabTodoLink::DomiosTask { task_id, label, .. }) => {
                requested == task_id || requested == label
            }
            Some(TabTodoLink::Url { url, label }) => requested == url || requested == label,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests;

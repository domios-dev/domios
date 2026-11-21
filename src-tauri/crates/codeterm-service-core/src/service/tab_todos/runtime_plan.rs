//! In-memory projection of runtime `update_plan` entries merged into a tab's sidebar checklist.

use codeterm_types::agent::chat_types::ChatMsg;
use codeterm_types::agent::{
    TabTodoChecklist, TabTodoChecklistStatus, TabTodoCounters, TabTodoItem, TabTodoState,
    MAX_TAB_TODO_ITEMS, MAX_TAB_TODO_TITLE_CHARS,
};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RuntimePlanEntry {
    pub title: String,
    pub state: TabTodoState,
    pub priority: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RuntimePlanSnapshot {
    pub revision: u64,
    pub updated_at: u64,
    pub entries: Vec<RuntimePlanEntry>,
}

static RUNTIME_PLANS: LazyLock<Mutex<HashMap<String, RuntimePlanSnapshot>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn extract_plan_entries(item: &ChatMsg) -> Option<Vec<RuntimePlanEntry>> {
    if item.tool_name.as_deref() != Some("update_plan") {
        return None;
    }
    let raw_entries = item
        .tool_input
        .as_ref()
        .and_then(|input| input.get("plan"))
        .and_then(|plan| plan.as_array())?;
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for entry in raw_entries {
        if entries.len() >= MAX_TAB_TODO_ITEMS {
            break;
        }
        let Some(object) = entry.as_object() else {
            continue;
        };
        let raw_title = object
            .get("content")
            .or_else(|| object.get("step"))
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let title = sanitize_title(raw_title);
        if title.is_empty() {
            continue;
        }
        let key = title.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let status = object
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("pending");
        let state = match status {
            "completed" | "done" => TabTodoState::Done,
            "in_progress" => TabTodoState::InProgress,
            _ => TabTodoState::Open,
        };
        let priority = object
            .get("priority")
            .and_then(|value| value.as_str())
            .map(sanitize_title)
            .filter(|value| !value.is_empty());
        entries.push(RuntimePlanEntry {
            title,
            state,
            priority,
        });
    }
    Some(entries)
}

pub(super) fn record(
    tab_id: &str,
    entries: Vec<RuntimePlanEntry>,
    now_secs: u64,
) -> Option<RuntimePlanSnapshot> {
    let mut plans = RUNTIME_PLANS.lock();
    let previous = plans.get(tab_id);
    if previous.is_some_and(|existing| existing.entries == entries) {
        return None;
    }
    let revision = previous.map_or(1, |existing| existing.revision.saturating_add(1));
    let snapshot = RuntimePlanSnapshot {
        revision,
        updated_at: now_secs,
        entries,
    };
    plans.insert(tab_id.to_owned(), snapshot.clone());
    Some(snapshot)
}

pub(super) fn for_tab(tab_id: &str) -> Option<RuntimePlanSnapshot> {
    RUNTIME_PLANS.lock().get(tab_id).cloned()
}

pub(super) fn has_items(tab_id: &str) -> bool {
    RUNTIME_PLANS
        .lock()
        .get(tab_id)
        .is_some_and(|snapshot| !snapshot.entries.is_empty())
}

pub(super) fn revision_for(tab_id: &str) -> u64 {
    RUNTIME_PLANS
        .lock()
        .get(tab_id)
        .map_or(0, |snapshot| snapshot.revision)
}

pub(super) fn forget_tabs(tab_ids: &[String]) {
    let mut plans = RUNTIME_PLANS.lock();
    for tab_id in tab_ids {
        plans.remove(tab_id);
    }
}

/// Merges runtime plan entries into an optional Domios checklist without overwriting or clobbering
/// existing Domios checklist items.
pub(super) fn merge_checklist(
    tab_id: &str,
    stored: Option<TabTodoChecklist>,
    plan: Option<RuntimePlanSnapshot>,
) -> Option<TabTodoChecklist> {
    let Some(plan) = plan else {
        return stored;
    };
    if plan.entries.is_empty() {
        return stored;
    }
    match stored {
        Some(mut checklist) if !checklist.items.is_empty() => {
            let mut existing_titles: HashSet<String> = checklist
                .items
                .iter()
                .map(|item| item.title.trim().to_lowercase())
                .collect();
            let mut next_position = checklist
                .items
                .iter()
                .map(|item| item.position)
                .max()
                .map_or(0, |pos| pos.saturating_add(1));
            for (index, entry) in plan.entries.into_iter().enumerate() {
                if checklist.items.len() >= MAX_TAB_TODO_ITEMS {
                    break;
                }
                let key = entry.title.trim().to_lowercase();
                if !existing_titles.insert(key) {
                    continue;
                }
                checklist
                    .items
                    .push(plan_todo_item(index, next_position, plan.revision, entry));
                next_position = next_position.saturating_add(1);
            }
            checklist.revision = checklist.revision.saturating_add(plan.revision);
            Some(checklist)
        }
        Some(stored_empty) => {
            let items: Vec<TabTodoItem> = plan
                .entries
                .into_iter()
                .take(MAX_TAB_TODO_ITEMS)
                .enumerate()
                .map(|(index, entry)| plan_todo_item(index, index as u8, plan.revision, entry))
                .collect();
            let all_done = items.iter().all(|item| item.state.is_terminal());
            Some(TabTodoChecklist {
                proposal: None,
                tab_id: tab_id.to_owned(),
                generation: stored_empty.generation.max(1),
                started_at: plan.updated_at,
                revision: stored_empty.revision.saturating_add(plan.revision),
                status: if all_done {
                    TabTodoChecklistStatus::Complete
                } else {
                    TabTodoChecklistStatus::Active
                },
                items,
            })
        }
        None => {
            let items: Vec<TabTodoItem> = plan
                .entries
                .into_iter()
                .take(MAX_TAB_TODO_ITEMS)
                .enumerate()
                .map(|(index, entry)| plan_todo_item(index, index as u8, plan.revision, entry))
                .collect();
            let all_done = items.iter().all(|item| item.state.is_terminal());
            Some(TabTodoChecklist {
                proposal: None,
                tab_id: tab_id.to_owned(),
                generation: 1,
                started_at: plan.updated_at,
                revision: plan.revision,
                status: if all_done {
                    TabTodoChecklistStatus::Complete
                } else {
                    TabTodoChecklistStatus::Active
                },
                items,
            })
        }
    }
}

fn plan_todo_item(
    index: usize,
    position: u8,
    revision: u64,
    entry: RuntimePlanEntry,
) -> TabTodoItem {
    TabTodoItem {
        id: format!("runtime-plan-{index}"),
        position,
        title: entry.title,
        description: entry.priority.map(|priority| format!("Priority: {priority}")),
        state: entry.state,
        revision,
        link: None,
        locked: false,
        reason: None,
        attempt: 0,
        counters: TabTodoCounters::default(),
        history: Vec::new(),
    }
}

fn sanitize_title(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                character
            }
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(MAX_TAB_TODO_TITLE_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn runtime_plan_merges_without_clobbering_domios_todos() {
        let domios = TabTodoChecklist {
            proposal: None,
            tab_id: "tab-1".into(),
            generation: 1,
            started_at: 10,
            revision: 3,
            status: TabTodoChecklistStatus::Active,
            items: vec![TabTodoItem {
                id: "domios-1".into(),
                position: 0,
                title: "Ship API route".into(),
                description: None,
                state: TabTodoState::InProgress,
                revision: 2,
                link: None,
                locked: true,
                reason: None,
                attempt: 1,
                counters: TabTodoCounters::default(),
                history: Vec::new(),
            }],
        };
        let mut msg = ChatMsg::default();
        msg.tool_name = Some("update_plan".into());
        msg.tool_input = Some(json!({
            "plan": [
                { "content": "Ship API route", "status": "completed", "priority": "high" },
                { "content": "Verify UI states", "status": "in_progress", "priority": "medium" },
                { "content": "Run focused checks", "status": "pending" }
            ]
        }));
        let entries = extract_plan_entries(&msg).unwrap();
        let merged = merge_checklist(
            "tab-1",
            Some(domios),
            Some(RuntimePlanSnapshot {
                revision: 2,
                updated_at: 20,
                entries,
            }),
        )
        .unwrap();
        assert_eq!(merged.items.len(), 3);
        assert_eq!(merged.items[0].id, "domios-1");
        assert_eq!(merged.items[0].state, TabTodoState::InProgress);
        assert!(merged.items[0].locked);
        assert_eq!(merged.items[1].title, "Verify UI states");
        assert_eq!(merged.items[1].state, TabTodoState::InProgress);
        assert_eq!(merged.items[2].title, "Run focused checks");
        assert_eq!(merged.items[2].state, TabTodoState::Open);
        assert_eq!(merged.revision, 5);
    }

    #[test]
    fn runtime_plan_synthesizes_checklist_when_no_domios_todos_exist() {
        let mut msg = ChatMsg::default();
        msg.tool_name = Some("update_plan".into());
        msg.tool_input = Some(json!({
            "plan": [
                { "content": "Inspect source", "status": "completed" },
                { "content": "Apply fix", "status": "in_progress" }
            ]
        }));
        let entries = extract_plan_entries(&msg).unwrap();
        let checklist = merge_checklist(
            "tab-2",
            None,
            Some(RuntimePlanSnapshot {
                revision: 1,
                updated_at: 42,
                entries,
            }),
        )
        .unwrap();
        assert_eq!(checklist.generation, 1);
        assert_eq!(checklist.revision, 1);
        assert_eq!(checklist.items.len(), 2);
        assert_eq!(checklist.items[0].state, TabTodoState::Done);
        assert_eq!(checklist.items[1].state, TabTodoState::InProgress);
    }
}

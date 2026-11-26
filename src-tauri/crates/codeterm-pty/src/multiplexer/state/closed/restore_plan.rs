use std::collections::{HashMap, HashSet};

use crate::multiplexer::types::{ClosedTab, Tab, TabBacking};

pub(super) fn check_reopenable_surface(
    record: &ClosedTab,
    live_tabs: &HashMap<String, Tab>,
) -> Result<(), String> {
    if matches!(record.backing, TabBacking::Surface(_)) {
        if let Some(owner_id) = record.parent_tab_id.as_deref() {
            if !live_tabs.contains_key(owner_id) {
                return Err(format!(
                    "Cannot reopen surface tab {} because owner tab {} is closed",
                    record.tab_id, owner_id
                ));
            }
        }
    }
    Ok(())
}

/// Drop owner-bound surfaces whose owner is neither restored in the same group nor live.
pub(super) fn filter_reopenable_group_tabs(
    tabs: &[ClosedTab],
    live_tabs: &HashMap<String, Tab>,
) -> Vec<ClosedTab> {
    let mut kept: Vec<ClosedTab> = tabs.to_vec();
    loop {
        let group_ids: HashSet<String> = kept.iter().map(|tab| tab.tab_id.clone()).collect();
        let before = kept.len();
        kept.retain(|tab| {
            if !matches!(tab.backing, TabBacking::Surface(_)) {
                return true;
            }
            let Some(owner_id) = tab.parent_tab_id.as_deref() else {
                return true;
            };
            owner_id != tab.tab_id.as_str()
                && (group_ids.contains(owner_id) || live_tabs.contains_key(owner_id))
        });
        if kept.len() == before {
            break;
        }
    }
    kept
}

pub(super) fn restored_parent_tab_id(
    record: &ClosedTab,
    parent_index: Option<usize>,
    restored_ids: &[String],
    live_tabs: &HashMap<String, Tab>,
) -> Option<String> {
    if let Some(parent) = parent_index {
        return Some(restored_ids[parent].clone());
    }
    matches!(record.backing, TabBacking::Surface(_))
        .then_some(record.parent_tab_id.as_deref())
        .flatten()
        .filter(|owner| live_tabs.contains_key(*owner))
        .map(str::to_string)
}

pub(super) fn apply_restored_parents(
    plan: &GroupRestorePlan<'_>,
    restored_ids: &[String],
    tabs: &mut HashMap<String, Tab>,
) {
    for (index, parent_index) in plan.parent_indices.iter().enumerate() {
        let parent_id =
            restored_parent_tab_id(&plan.tabs[index], *parent_index, restored_ids, tabs);
        if let Some(tab) = tabs.get_mut(&restored_ids[index]) {
            tab.parent_tab_id = parent_id;
        }
    }
}

#[derive(Debug)]
pub(super) struct GroupRestorePlan<'a> {
    pub tabs: &'a [ClosedTab],
    pub parent_indices: Vec<Option<usize>>,
    pub active_index: usize,
}

impl<'a> GroupRestorePlan<'a> {
    pub fn new(tabs: &'a [ClosedTab]) -> Result<Self, String> {
        if tabs.is_empty() {
            return Err("Closed group has no Tabs".into());
        }
        let mut indices = HashMap::new();
        for (index, tab) in tabs.iter().enumerate() {
            if tab.tab_id.is_empty() || indices.insert(tab.tab_id.as_str(), index).is_some() {
                return Err("Closed group has empty or duplicate Tab identities".into());
            }
        }
        let parent_indices: Vec<_> = tabs
            .iter()
            .map(|tab| {
                tab.parent_tab_id
                    .as_deref()
                    .and_then(|parent| indices.get(parent).copied())
            })
            .collect();
        let mut checked = HashSet::new();
        for index in 0..tabs.len() {
            let mut path = HashSet::new();
            let mut current = Some(index);
            while let Some(node) = current {
                if checked.contains(&node) {
                    break;
                }
                if !path.insert(node) {
                    return Err("Closed group has a parent cycle".into());
                }
                current = parent_indices[node];
            }
            checked.extend(path);
        }
        let active_index = tabs
            .iter()
            .position(|tab| tab.parent_tab_id.is_none())
            .or_else(|| parent_indices.iter().position(Option::is_none))
            .ok_or("Closed group has no root Tab")?;
        Ok(Self {
            tabs,
            parent_indices,
            active_index,
        })
    }
}

#[cfg(test)]
mod tests;

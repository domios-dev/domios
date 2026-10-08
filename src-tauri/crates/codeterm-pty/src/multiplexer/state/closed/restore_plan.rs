use std::collections::{HashMap, HashSet};

use crate::multiplexer::types::ClosedTab;

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

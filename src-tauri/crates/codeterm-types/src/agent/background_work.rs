use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const LIMIT: usize = 256;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundWorkObservation {
    pub id: String,
    pub state: BackgroundWorkState,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundWorkState {
    Running,
    Unknown,
    SessionBoundary,
    Completed,
    Killed,
    Failed,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundRoster {
    pub running: BTreeMap<String, String>,
    #[serde(default)]
    pub unknown: BTreeMap<String, String>,
    #[serde(default)]
    pub ended: BTreeMap<String, String>,
    pub overflow: bool,
    #[serde(default)]
    pub history_incomplete: bool,
    #[serde(default)]
    pub history_initialized: bool,
    #[serde(skip)]
    completed: BTreeSet<String>,
}
impl BackgroundRoster {
    pub fn invalidate(&mut self) {
        for (id, label) in std::mem::take(&mut self.running) {
            if self.unknown.len() >= LIMIT {
                self.unknown.pop_first();
            }
            self.unknown.insert(id.clone(), label);
            if self.completed.len() >= LIMIT {
                self.completed.pop_first();
            }
            self.completed.insert(id);
        }
        for id in self.unknown.keys() {
            if self.completed.len() >= LIMIT {
                self.completed.pop_first();
            }
            self.completed.insert(id.clone());
        }
    }
    pub fn observe(&mut self, observation: &BackgroundWorkObservation, label: &str) {
        if observation.state == BackgroundWorkState::SessionBoundary {
            self.invalidate();
            return;
        }
        if observation.id.is_empty() || observation.id.len() > 256 {
            self.overflow = true;
            return;
        }
        match observation.state {
            BackgroundWorkState::Running => {
                if self.completed.contains(&observation.id)
                    || self.ended.contains_key(&observation.id)
                {
                    return;
                }
                if self.running.len() >= LIMIT && !self.running.contains_key(&observation.id) {
                    self.overflow = true;
                    return;
                }
                let previous_label = self.unknown.remove(&observation.id).unwrap_or_default();
                let value = self
                    .running
                    .entry(observation.id.clone())
                    .or_insert(previous_label);
                if !label.is_empty() {
                    *value = label.chars().take(256).collect();
                }
            }
            BackgroundWorkState::Unknown => {
                if !self.running.contains_key(&observation.id)
                    && !self.ended.contains_key(&observation.id)
                {
                    if self.unknown.len() >= LIMIT && !self.unknown.contains_key(&observation.id) {
                        self.unknown.pop_first();
                    }
                    self.unknown
                        .entry(observation.id.clone())
                        .or_insert_with(|| label.chars().take(256).collect());
                }
            }
            _ => {
                let label = self
                    .running
                    .remove(&observation.id)
                    .or_else(|| self.unknown.remove(&observation.id))
                    .unwrap_or_else(|| label.chars().take(256).collect());
                if self.ended.len() >= LIMIT {
                    self.ended.pop_first();
                }
                self.ended.insert(observation.id.clone(), label);
                if self.completed.len() >= LIMIT {
                    self.completed.pop_first();
                }
                self.completed.insert(observation.id.clone());
            }
        }
    }
    pub fn is_empty(&self) -> bool {
        self.running.is_empty() && !self.overflow && !self.history_incomplete
    }
    pub fn guarded_items(&self) -> Vec<String> {
        let mut items = self.items();
        if !self.history_initialized {
            items.push("background roster has not observed this session".into());
        }
        items
    }
    pub fn items(&self) -> Vec<String> {
        let mut items: Vec<_> = self
            .running
            .iter()
            .map(|(id, label)| {
                if label.is_empty() {
                    id.clone()
                } else {
                    format!("{id}: {label}")
                }
            })
            .collect();
        if self.overflow {
            items.push("additional background work exceeded the roster bound".into());
        }
        if self.history_incomplete {
            items.push("background history is incomplete; active work cannot be ruled out".into());
        }
        items
    }
}

#[cfg(test)]
mod tests;

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
    Completed,
    Killed,
    Failed,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundRoster {
    pub running: BTreeMap<String, String>,
    pub overflow: bool,
    #[serde(skip)]
    completed: BTreeSet<String>,
}
impl BackgroundRoster {
    pub fn observe(&mut self, observation: &BackgroundWorkObservation, label: &str) {
        if observation.id.is_empty() || observation.id.len() > 256 {
            self.overflow = true;
            return;
        }
        match observation.state {
            BackgroundWorkState::Running => {
                if self.completed.contains(&observation.id) {
                    return;
                }
                if self.running.len() >= LIMIT && !self.running.contains_key(&observation.id) {
                    self.overflow = true;
                    return;
                }
                let value = self.running.entry(observation.id.clone()).or_default();
                if !label.is_empty() {
                    *value = label.chars().take(256).collect();
                }
            }
            _ => {
                self.running.remove(&observation.id);
                if self.completed.len() >= LIMIT {
                    self.completed.pop_first();
                }
                self.completed.insert(observation.id.clone());
            }
        }
    }
    pub fn is_empty(&self) -> bool {
        self.running.is_empty() && !self.overflow
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
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(id: &str, state: BackgroundWorkState) -> BackgroundWorkObservation {
        BackgroundWorkObservation {
            id: id.into(),
            state,
        }
    }
    #[test]
    fn restart_guard_roster_drains_and_ignores_replayed_starts() {
        let mut roster = BackgroundRoster::default();
        roster.observe(&event("shell", BackgroundWorkState::Running), "fixture");
        assert!(!roster.is_empty());
        roster.observe(&event("shell", BackgroundWorkState::Completed), "");
        roster.observe(&event("shell", BackgroundWorkState::Running), "fixture");
        assert!(roster.is_empty());
        roster.observe(&event("agent", BackgroundWorkState::Running), "");
        roster.observe(&event("agent", BackgroundWorkState::Killed), "");
        assert!(roster.is_empty());
    }
    #[test]
    fn restart_guard_roster_bound_fails_closed_and_tabs_are_independent() {
        let mut first = BackgroundRoster::default();
        for index in 0..=LIMIT {
            first.observe(&event(&index.to_string(), BackgroundWorkState::Running), "");
        }
        assert_eq!(first.running.len(), LIMIT);
        assert!(first.overflow);
        assert!(!first.is_empty());
        assert!(BackgroundRoster::default().is_empty());
        first = BackgroundRoster::default();
        assert!(first.is_empty());
    }
}

//! Human task keys (`PREFIX-N`): the one owner of prefix allocation, counter
//! allocation, the legacy display fallback, and task reference matching.

use super::Task;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const TASK_ID_PREFIX: &str = "task_";
const EMPTY_NAME_STEM: &str = "WS";
const MIN_PREFIX: usize = 2;
const MAX_PREFIX: usize = 4;

/// Durable key state owned by one workspace. The prefix is fixed by the first
/// issued key; the counter only grows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceTaskKeys {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub next: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl WorkspaceTaskKeys {
    pub fn is_unissued(&self) -> bool {
        self.prefix.is_none() && self.next == 0
    }

    /// `high_water` holds the largest number already stored per prefix, so a
    /// lost or recreated workspace record can never hand out a used key.
    pub fn issue(
        &mut self,
        workspace_name: &str,
        taken_prefixes: &HashSet<String>,
        high_water: &HashMap<String, u64>,
    ) -> String {
        let prefix = match &self.prefix {
            Some(prefix) => prefix.clone(),
            None => {
                let assigned = assign_prefix(workspace_name, taken_prefixes);
                self.prefix = Some(assigned.clone());
                assigned
            }
        };
        let floor = high_water.get(&prefix).copied().unwrap_or(0) + 1;
        let number = self.next.max(floor);
        self.next = number + 1;
        format_key(&prefix, number)
    }
}

fn name_words(name: &str) -> Vec<String> {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_uppercase)
        .collect()
}

fn head(text: &str, len: usize) -> String {
    text.chars().take(len).collect()
}

/// Ordered, deterministic prefix candidates for a workspace name.
pub fn prefix_candidates(workspace_name: &str) -> Vec<String> {
    let words = name_words(workspace_name);
    let letters: String = words.concat();
    let mut base = if words.len() >= MIN_PREFIX {
        words
            .iter()
            .take(MAX_PREFIX)
            .filter_map(|word| word.chars().next())
            .collect()
    } else {
        head(&letters, 3)
    };
    if base.len() < MIN_PREFIX {
        base = if letters.is_empty() {
            EMPTY_NAME_STEM.to_string()
        } else {
            format!("{letters:X<width$}", width = MIN_PREFIX)
        };
    }
    let mut out: Vec<String> = Vec::new();
    let mut push = |candidate: String| {
        if (MIN_PREFIX..=MAX_PREFIX).contains(&candidate.len()) && !out.contains(&candidate) {
            out.push(candidate);
        }
    };
    push(base.clone());
    for len in MIN_PREFIX..=MAX_PREFIX {
        if letters.len() >= len {
            push(head(&letters, len));
        }
    }
    let stem3 = head(&base, 3);
    for digit in 2..=9 {
        push(format!("{stem3}{digit}"));
    }
    let stem2 = head(&base, MIN_PREFIX);
    for number in 10..=99 {
        push(format!("{stem2}{number}"));
    }
    out
}

/// First candidate no other local workspace holds; deterministic for a given
/// name and taken set.
pub fn assign_prefix(workspace_name: &str, taken_prefixes: &HashSet<String>) -> String {
    let candidates = prefix_candidates(workspace_name);
    if let Some(free) = candidates.iter().find(|c| !taken_prefixes.contains(*c)) {
        return free.clone();
    }
    let stem = head(&candidates[0], 1);
    (100..=999)
        .map(|number| format!("{stem}{number}"))
        .find(|candidate| !taken_prefixes.contains(candidate))
        .unwrap_or_else(|| format!("{stem}000"))
}

pub fn format_key(prefix: &str, number: u64) -> String {
    format!("{prefix}-{number}")
}

pub fn parse_key(key: &str) -> Option<(&str, u64)> {
    let (prefix, number) = key.rsplit_once('-')?;
    let valid_prefix = (MIN_PREFIX..=MAX_PREFIX).contains(&prefix.len())
        && prefix
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    let valid_number = !number.is_empty() && number.chars().all(|c| c.is_ascii_digit());
    if !valid_prefix || !valid_number {
        return None;
    }
    number
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .map(|n| (prefix, n))
}

/// Display and reference key for a task that was created before keys existed.
pub fn fallback_key(task_id: &str) -> String {
    task_id
        .strip_prefix(TASK_ID_PREFIX)
        .unwrap_or(task_id)
        .to_ascii_uppercase()
}

/// Keys are displayed uppercase but typed in any case.
pub fn key_matches(display_key: &str, reference: &str) -> bool {
    !reference.is_empty() && display_key.eq_ignore_ascii_case(reference)
}

/// The one case-folded name tier: a handle and a key compete on equal terms, so
/// a collision is refused no matter how the reference is spelled.
pub fn name_matches(handle: Option<&str>, display_key: &str, reference: &str) -> bool {
    handle.is_some_and(|handle| key_matches(handle, reference))
        || key_matches(display_key, reference)
}

/// Largest stored number per prefix across the given keys.
pub fn high_water<'a>(keys: impl IntoIterator<Item = &'a str>) -> HashMap<String, u64> {
    let mut out: HashMap<String, u64> = HashMap::new();
    for (prefix, number) in keys.into_iter().filter_map(parse_key) {
        let entry = out.entry(prefix.to_string()).or_default();
        *entry = (*entry).max(number);
    }
    out
}

impl Task {
    pub fn display_key(&self) -> String {
        self.key.clone().unwrap_or_else(|| fallback_key(&self.id))
    }

    pub fn key_matches(&self, reference: &str) -> bool {
        key_matches(&self.display_key(), reference)
    }

    pub fn matches_ref(&self, reference: &str) -> bool {
        self.id == reference || self.name_matches(reference)
    }

    pub fn name_matches(&self, reference: &str) -> bool {
        name_matches(self.handle.as_deref(), &self.display_key(), reference)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskRefMatch {
    Found(usize),
    Ambiguous(Vec<usize>),
    Missing,
}

/// Exact id wins; otherwise handle and key share one tier and must be unique.
pub fn match_task_ref(tasks: &[Task], reference: &str) -> TaskRefMatch {
    if let Some(index) = tasks.iter().position(|task| task.id == reference) {
        return TaskRefMatch::Found(index);
    }
    let hits: Vec<usize> = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.name_matches(reference))
        .map(|(index, _)| index)
        .collect();
    match hits.len() {
        0 => TaskRefMatch::Missing,
        1 => TaskRefMatch::Found(hits[0]),
        _ => TaskRefMatch::Ambiguous(hits),
    }
}

pub fn find_task_index(tasks: &[Task], reference: &str) -> Option<usize> {
    match match_task_ref(tasks, reference) {
        TaskRefMatch::Found(index) => Some(index),
        TaskRefMatch::Ambiguous(_) | TaskRefMatch::Missing => None,
    }
}

pub fn find_task<'a>(tasks: &'a [Task], reference: &str) -> Option<&'a Task> {
    find_task_index(tasks, reference).map(|index| &tasks[index])
}

pub fn find_task_mut<'a>(tasks: &'a mut [Task], reference: &str) -> Option<&'a mut Task> {
    find_task_index(tasks, reference).map(move |index| &mut tasks[index])
}

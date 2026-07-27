//! Observation snapshot assembly for watcher ticks.

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::{json, Value};

use crate::agent::chat_types::ChatMsg;
use crate::agent::reports::AgentReport;
use crate::agent::status::PaneStatus;
use crate::multiplexer::state::MultiplexerSnapshot;
use crate::multiplexer::types::{AgentRole, Pane};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationConfig {
    pub orchestration: bool,
    pub chat_tail: usize,
    pub reports: bool,
}

impl Default for ObservationConfig {
    fn default() -> Self {
        Self {
            orchestration: true,
            chat_tail: 0,
            reports: false,
        }
    }
}

pub fn observation_config_from_json(v: &Value) -> ObservationConfig {
    ObservationConfig {
        orchestration: v
            .get("orchestration")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        chat_tail: v.get("chatTail").and_then(Value::as_u64).unwrap_or(0) as usize,
        reports: v.get("reports").and_then(Value::as_bool).unwrap_or(false),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchGroup {
    pub orchestrator_id: String,
    pub pane_ids: Vec<String>,
}

/// External reads performed after the mux snapshot is dropped.
pub struct ObservationSources<'a> {
    pub chat_tail: &'a dyn Fn(&str, usize) -> Vec<ChatMsg>,
    pub reports: &'a [AgentReport],
    pub last_activity_ms: &'a HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize)]
struct PaneObservation {
    pane_id: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    role_profile: Option<String>,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_activity_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "chatTail")]
    chat_tail: Option<Vec<Value>>,
}

pub fn is_manager_watcher(pane: &Pane) -> bool {
    pane.role == Some(AgentRole::Manager)
        && pane
            .role_profile
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case("watcher"))
}

pub fn resolve_orchestrator_id(snapshot: &MultiplexerSnapshot, pane_id: &str) -> Option<String> {
    let mut current = pane_id.to_string();
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(current.clone()) {
            return None;
        }
        let pane = snapshot.panes.get(&current)?;
        if pane.role == Some(AgentRole::Orchestrator) {
            return Some(current);
        }
        current = pane.parent_pane_id.clone()?;
    }
}

fn descendant_ids(snapshot: &MultiplexerSnapshot, root: &str) -> HashSet<String> {
    let mut group = HashSet::new();
    group.insert(root.to_string());
    let mut queue = vec![root.to_string()];
    while let Some(id) = queue.pop() {
        for (pid, pane) in &snapshot.panes {
            if pane.parent_pane_id.as_deref() == Some(id.as_str()) && group.insert(pid.clone()) {
                queue.push(pid.clone());
            }
        }
    }
    group
}

pub fn collect_watch_group(
    snapshot: &MultiplexerSnapshot,
    watcher_pane_id: &str,
) -> Result<WatchGroup, String> {
    snapshot
        .panes
        .get(watcher_pane_id)
        .ok_or_else(|| format!("Tab not found: {watcher_pane_id}"))?;
    let orchestrator_id = resolve_orchestrator_id(snapshot, watcher_pane_id)
        .ok_or_else(|| "could not resolve orchestrator from parent chain".to_string())?;
    let mut pane_ids: Vec<String> = descendant_ids(snapshot, &orchestrator_id)
        .into_iter()
        .filter(|pid| pid != watcher_pane_id)
        .filter(|pid| {
            snapshot
                .panes
                .get(pid)
                .is_none_or(|p| !is_manager_watcher(p))
        })
        .collect();
    pane_ids.sort();
    Ok(WatchGroup {
        orchestrator_id,
        pane_ids,
    })
}

fn pane_status_label(snapshot: &MultiplexerSnapshot, pane_id: &str) -> String {
    snapshot
        .pane_statuses
        .get(pane_id)
        .map(|s| match s {
            PaneStatus::Working => "Working",
            PaneStatus::Waiting => "Waiting",
            PaneStatus::Idle => "Idle",
            PaneStatus::Dead => "Dead",
        })
        .unwrap_or("Unknown")
        .to_string()
}

fn role_label(role: Option<AgentRole>) -> Option<String> {
    role.map(|r| match r {
        AgentRole::Orchestrator => "Orchestrator",
        AgentRole::Manager => "Manager",
        AgentRole::Worker => "Worker",
    })
    .map(str::to_string)
}

fn chat_msgs_to_json(msgs: &[ChatMsg]) -> Vec<Value> {
    msgs.iter()
        .map(|m| {
            json!({
                "id": m.id,
                "kind": m.kind,
                "content": m.content,
            })
        })
        .collect()
}

fn reports_to_json(reports: &[AgentReport]) -> Vec<Value> {
    reports
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "from_pane_id": r.from_pane_id,
                "from_title": r.from_title,
                "message": r.message,
                "timestamp": r.timestamp,
                "status": r.status.map(|s| format!("{:?}", s)),
            })
        })
        .collect()
}

pub fn build_observation(
    snapshot: &MultiplexerSnapshot,
    watcher_pane_id: &str,
    config: &ObservationConfig,
    sources: &ObservationSources<'_>,
) -> Value {
    let group = match collect_watch_group(snapshot, watcher_pane_id) {
        Ok(g) => g,
        Err(e) => {
            return json!({ "error": e });
        }
    };

    let mut out = json!({
        "orchestrator_id": group.orchestrator_id,
    });

    if config.orchestration {
        let panes: Vec<PaneObservation> = group
            .pane_ids
            .iter()
            .filter_map(|pid| snapshot.panes.get(pid))
            .map(|pane| {
                let chat_tail = if config.chat_tail > 0 {
                    let msgs = (sources.chat_tail)(&pane.id, config.chat_tail);
                    Some(chat_msgs_to_json(&msgs))
                } else {
                    None
                };
                PaneObservation {
                    pane_id: pane.id.clone(),
                    title: pane.title.clone(),
                    role: role_label(pane.role),
                    role_profile: pane.role_profile.clone(),
                    status: pane_status_label(snapshot, &pane.id),
                    last_activity_ms: sources
                        .last_activity_ms
                        .get(&pane.id)
                        .copied()
                        .filter(|ms| *ms > 0),
                    chat_tail,
                }
            })
            .collect();
        out["panes"] = serde_json::to_value(panes).unwrap_or(Value::Array(vec![]));
    }

    if config.reports {
        out["reports"] = Value::Array(reports_to_json(sources.reports));
    }

    out
}

#[cfg(test)]
mod tests;

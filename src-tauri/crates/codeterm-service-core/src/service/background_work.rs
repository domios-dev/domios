use super::ServiceContext;
use crate::service::MuxEmitCause;
use codeterm_types::agent::background_work::{BackgroundWorkObservation, BackgroundWorkState};
use parking_lot::Mutex;
use std::collections::{HashMap, VecDeque};
use std::sync::OnceLock;

mod actions;
pub use actions::{ask_stop, read_output, BackgroundWorkTarget};

const TOOL_LABEL_LIMIT: usize = 256;

struct BindingToolLabels {
    identity: (String, String),
    labels: HashMap<String, String>,
    order: VecDeque<String>,
}

impl BindingToolLabels {
    fn new(session: &str, nonce: &str) -> Self {
        Self {
            identity: (session.to_owned(), nonce.to_owned()),
            labels: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn insert(&mut self, id: &str, label: &str) {
        let trimmed = label.trim();
        if id.is_empty() || id.len() > 256 || trimmed.is_empty() {
            return;
        }
        let value: String = trimmed.chars().take(256).collect();
        if !self.labels.contains_key(id) {
            if self.labels.len() >= TOOL_LABEL_LIMIT {
                if let Some(oldest) = self.order.pop_front() {
                    self.labels.remove(&oldest);
                }
            }
            self.order.push_back(id.to_owned());
        }
        self.labels.insert(id.to_owned(), value);
    }

    fn get(&self, id: &str) -> Option<&str> {
        self.labels.get(id).map(String::as_str)
    }
}

fn tool_label_store() -> &'static Mutex<HashMap<String, BindingToolLabels>> {
    static STORE: OnceLock<Mutex<HashMap<String, BindingToolLabels>>> = OnceLock::new();
    STORE.get_or_init(Mutex::default)
}

pub(crate) fn clear_tab_labels(tab_id: &str) {
    tool_label_store().lock().remove(tab_id);
}

fn record_and_resolve_tool_labels(
    tab_id: &str,
    session: &str,
    nonce: &str,
    forked: bool,
    records: &[crate::agent::chat_types::RichMessage],
) -> (bool, HashMap<String, String>) {
    let mut new_calls: Vec<(&str, &str)> = Vec::new();
    let mut needed_ids: Vec<&str> = Vec::new();
    let mut has_roster_events = false;

    for record in records {
        if !record_matches_session(record, forked, session) {
            continue;
        }
        for call in &record.background_tool_calls {
            if !call.id.is_empty() && !call.label.trim().is_empty() {
                new_calls.push((&call.id, &call.label));
            }
        }
        for block in &record.blocks {
            if block.kind == "tool_use" && block.data["tool_kind"] == "command" {
                if let (Some(call_id), Some(command)) = (
                    block.data.get("id").and_then(serde_json::Value::as_str),
                    block
                        .data
                        .get("input")
                        .and_then(|input| input.get("command"))
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| {
                            block
                                .data
                                .get("tool_detail")
                                .and_then(serde_json::Value::as_str)
                        })
                        .filter(|text| !text.trim().is_empty()),
                ) {
                    new_calls.push((call_id, command));
                }
            }
            if block.data.get("tool_background_task_id").is_some() {
                has_roster_events = true;
                if let Some(call_id) = block
                    .data
                    .get("tool_use_id")
                    .and_then(serde_json::Value::as_str)
                {
                    needed_ids.push(call_id);
                }
            }
        }
        if !record.background_work.is_empty() {
            has_roster_events = true;
            for observation in &record.background_work {
                if let Some(call_id) = observation.tool_use_id.as_deref() {
                    needed_ids.push(call_id);
                }
            }
        }
    }

    if new_calls.is_empty() && needed_ids.is_empty() {
        return (has_roster_events, HashMap::new());
    }

    let mut store = tool_label_store().lock();
    let entry = store
        .entry(tab_id.to_owned())
        .or_insert_with(|| BindingToolLabels::new(session, nonce));
    if entry.identity.0 != session || entry.identity.1 != nonce {
        *entry = BindingToolLabels::new(session, nonce);
    }
    for (id, label) in new_calls {
        entry.insert(id, label);
    }
    let mut resolved = HashMap::new();
    for id in needed_ids {
        if let Some(label) = entry.get(id) {
            resolved.insert(id.to_owned(), label.to_owned());
        }
    }
    (has_roster_events, resolved)
}

fn record_matches_session(
    record: &crate::agent::chat_types::RichMessage,
    forked: bool,
    session: &str,
) -> bool {
    if record
        .provider_session_id
        .as_deref()
        .is_some_and(|id| id != session)
    {
        return false;
    }
    if forked && record.provider_session_id.as_deref() != Some(session) {
        return false;
    }
    true
}

pub fn restart_guard(
    items: Vec<String>,
    force: bool,
) -> Result<(), super::agent_lifecycle::AgentRestartError> {
    if items.is_empty() || force {
        Ok(())
    } else {
        Err(super::agent_lifecycle::AgentRestartError::BackgroundWork { items })
    }
}

pub fn running(svc: &ServiceContext, tab_id: &str) -> Vec<String> {
    let provider = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.agent_type.clone());
    let Some(provider) = provider else {
        return Vec::new();
    };
    let enabled = svc
        .try_ext::<crate::config::ConfigState>()
        .is_some_and(|config| {
            config
                .lock()
                .agents
                .get(&provider)
                .and_then(|config| config.extra.get("background_work_guard"))
                .and_then(toml::Value::as_bool)
                == Some(true)
        });
    if !enabled
        || !svc
            .agent_registry
            .with_provider(&provider, |provider| provider.tracks_background_work())
            .unwrap_or(false)
    {
        return Vec::new();
    }
    svc.mux
        .read()
        .get_tab(tab_id)
        .filter(|tab| tab.agent_type.as_deref() == Some(&provider))
        .map(|tab| tab.provider_signals.background.guarded_items())
        .unwrap_or_default()
}

pub fn apply(
    svc: &ServiceContext,
    tab_id: &str,
    session: &str,
    nonce: &str,
    records: &[crate::agent::chat_types::RichMessage],
) {
    apply_observed(svc, tab_id, session, nonce, records, false);
}

pub(crate) fn apply_observed(
    svc: &ServiceContext,
    tab_id: &str,
    session: &str,
    nonce: &str,
    records: &[crate::agent::chat_types::RichMessage],
    live: bool,
) {
    let provider = svc
        .mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.agent_type.clone());
    let Some(provider) = provider else {
        return;
    };
    if !svc
        .agent_registry
        .with_provider(&provider, |provider| provider.tracks_background_work())
        .unwrap_or(false)
    {
        return;
    }
    let matches_binding = |tab: &crate::multiplexer::types::Tab| {
        tab.agent_type.as_deref() == Some(&provider)
            && tab.agent_session_id.as_deref() == Some(session)
            && tab.session_binding_nonce.as_deref() == Some(nonce)
    };
    let initial_fork_source = {
        let mux = svc.mux.read();
        mux.get_tab(tab_id)
            .filter(|tab| matches_binding(tab))
            .map(|tab| tab.fork_source_session_id.clone())
    };
    let Some(initial_fork_source) = initial_fork_source else {
        return;
    };
    let (has_roster_events, tool_labels) = record_and_resolve_tool_labels(
        tab_id,
        session,
        nonce,
        initial_fork_source.is_some(),
        records,
    );
    if !has_roster_events {
        return;
    }
    for _ in 0..2 {
        let (before, fork_source) = {
            let mux = svc.mux.read();
            let Some(tab) = mux.get_tab(tab_id).filter(|tab| matches_binding(tab)) else {
                return;
            };
            (
                tab.provider_signals.background.clone(),
                tab.fork_source_session_id.clone(),
            )
        };
        let mut next = observe_background_records(
            &before,
            fork_source.is_some(),
            session,
            records,
            live,
            &tool_labels,
        );
        let changed = next != before;
        if !changed {
            return;
        }
        let committed = {
            let mut mux = svc.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return;
            };
            if matches_binding(tab)
                && tab.fork_source_session_id == fork_source
                && tab.provider_signals.background == before
            {
                std::mem::swap(&mut tab.provider_signals.background, &mut next);
                true
            } else {
                false
            }
        };
        if committed {
            svc.emit_state_changed(MuxEmitCause::BackgroundWork);
            super::provider_update::background_work_changed(svc, tab_id);
            return;
        }
    }
    log::debug!("[background-work] discarded contended update tab={tab_id}");
}

fn lookup_label<'a>(
    call_id: Option<&str>,
    batch: &HashMap<&str, &'a str>,
    side_table: &'a HashMap<String, String>,
) -> Option<&'a str> {
    let id = call_id?;
    batch
        .get(id)
        .copied()
        .or_else(|| side_table.get(id).map(String::as_str))
}

fn observe_background_records(
    previous: &codeterm_types::agent::background_work::BackgroundRoster,
    forked: bool,
    session: &str,
    records: &[crate::agent::chat_types::RichMessage],
    live: bool,
    tool_labels: &HashMap<String, String>,
) -> codeterm_types::agent::background_work::BackgroundRoster {
    let mut roster = previous.clone();
    let mut batch_labels: HashMap<&str, &str> = HashMap::new();
    for record in records {
        if !record_matches_session(record, forked, session) {
            continue;
        }
        for call in &record.background_tool_calls {
            if !call.id.is_empty() && !call.label.trim().is_empty() {
                batch_labels.insert(&call.id, call.label.trim());
            }
        }
        for block in &record.blocks {
            if block.kind == "tool_use" && block.data["tool_kind"] == "command" {
                if let (Some(call_id), Some(command)) = (
                    block.data.get("id").and_then(serde_json::Value::as_str),
                    block
                        .data
                        .get("input")
                        .and_then(|input| input.get("command"))
                        .and_then(serde_json::Value::as_str)
                        .or_else(|| {
                            block
                                .data
                                .get("tool_detail")
                                .and_then(serde_json::Value::as_str)
                        })
                        .filter(|text| !text.trim().is_empty()),
                ) {
                    batch_labels.insert(call_id, command.trim());
                }
            }
            if let Some(id) = block
                .data
                .get("tool_background_task_id")
                .and_then(serde_json::Value::as_str)
            {
                let tool_use_id = block
                    .data
                    .get("tool_use_id")
                    .and_then(serde_json::Value::as_str);
                let label = block
                    .data
                    .get("tool_summary")
                    .and_then(serde_json::Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .or_else(|| lookup_label(tool_use_id, &batch_labels, tool_labels))
                    .unwrap_or("");
                roster.observe(
                    &BackgroundWorkObservation {
                        id: id.into(),
                        state: BackgroundWorkState::Unknown,
                        tool_use_id: tool_use_id.map(str::to_owned),
                        ..Default::default()
                    },
                    label,
                );
            }
        }
        for observation in &record.background_work {
            if !live
                && previous.history_initialized
                && observation.state == BackgroundWorkState::SessionBoundary
            {
                continue;
            }
            let block_label = record.blocks.iter().find_map(|block| {
                let input = &block.data["input"];
                if block.data["tool_kind"] != "agent"
                    || input["agent_id"].as_str() != Some(&observation.id)
                {
                    return None;
                }
                ["agent_description", "agent_type"]
                    .into_iter()
                    .find_map(|field| input[field].as_str().filter(|text| !text.trim().is_empty()))
            });
            let label = lookup_label(
                observation.tool_use_id.as_deref(),
                &batch_labels,
                tool_labels,
            )
            .or(block_label)
            .unwrap_or("");
            let mut observation = observation.clone();
            if record.provider_session_id.as_deref() != Some(session) {
                observation.output_path = None;
            }
            if !live && observation.state == BackgroundWorkState::Running {
                observation.state = BackgroundWorkState::Unknown;
            }
            roster.observe(&observation, label);
        }
    }
    roster
}

pub fn force_notice(svc: &ServiceContext, tab: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    log::warn!("[restart-guard] explicit force override tab={tab} running={items:?}");
    let owner = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.parent_tab_id.clone())
        .unwrap_or_else(|| tab.into());
    super::provider_signals::warn(
        svc,
        &owner,
        format!(
            "Forced restart of tab {tab} overrides running background work: {}",
            items.join(", ")
        ),
    );
}

#[cfg(test)]
mod tests;

pub fn incomplete_snapshot(svc: &ServiceContext, tab_id: &str, session: &str, nonce: &str) {
    let mut mux = svc.mux.write();
    let Some(tab) = mux.get_tab_mut(tab_id).filter(|tab| {
        tab.agent_session_id.as_deref() == Some(session)
            && tab.session_binding_nonce.as_deref() == Some(nonce)
    }) else {
        return;
    };
    if !tab.provider_signals.background.history_initialized {
        tab.provider_signals.background.history_incomplete = true;
    }
    drop(mux);
    svc.emit_state_changed(MuxEmitCause::BackgroundWork);
}

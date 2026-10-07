use super::lifecycle::{for_tab, rpc_error};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct ControlState {
    pub active: HashMap<String, String>,
    pub parents: HashMap<String, String>,
    completed: HashSet<(String, String)>,
    interrupted: HashSet<(String, String)>,
}

impl ControlState {
    pub fn started(&mut self, thread: &str, turn: &str) {
        self.active.insert(thread.into(), turn.into());
    }
    pub fn completed(&mut self, thread: &str, turn: &str) -> bool {
        if self.active.get(thread).is_some_and(|active| active == turn) {
            self.active.remove(thread);
        }
        self.completed.insert((thread.into(), turn.into()))
    }
    pub fn descendants(&self, root: &str) -> Vec<(String, String)> {
        self.active
            .iter()
            .filter(|(thread, _)| {
                let mut candidate = thread.as_str();
                let mut seen = HashSet::new();
                while candidate != root {
                    if !seen.insert(candidate) {
                        return false;
                    }
                    let Some(parent) = self.parents.get(candidate) else {
                        return false;
                    };
                    candidate = parent;
                }
                true
            })
            .map(|(thread, turn)| (thread.clone(), turn.clone()))
            .collect()
    }
    pub fn was_interrupted(&self, thread: &str, turn: &str) -> bool {
        self.interrupted.contains(&(thread.into(), turn.into()))
    }
}

pub fn active_turn(tab: &str) -> Option<String> {
    let sidecar = for_tab(tab)?;
    let thread = sidecar.thread.lock().clone();
    let active = sidecar.control.lock().active.get(&thread).cloned();
    active
}

pub async fn interrupt(tab: &str) -> Option<Result<bool, String>> {
    let sidecar = for_tab(tab)?;
    let root = sidecar.thread.lock().clone();
    let turns = {
        let mut control = sidecar.control.lock();
        let turns = control.descendants(&root);
        control.interrupted.extend(turns.iter().cloned());
        turns
    };
    if turns.is_empty() {
        return Some(Ok(false));
    }
    let mut settled = sidecar.turn_settled.subscribe();
    for (thread, turn) in &turns {
        if let Err(error) = sidecar
            .client
            .request("turn/interrupt", json!({"threadId":thread,"turnId":turn}))
            .await
        {
            return Some(Err(rpc_error(error)));
        }
    }
    let confirmed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let remaining = {
                let state = sidecar.control.lock();
                turns
                    .iter()
                    .any(|(thread, turn)| state.active.get(thread) == Some(turn))
            };
            if !remaining {
                return true;
            }
            if settled.changed().await.is_err() {
                return false;
            }
        }
    })
    .await
    .unwrap_or(false);
    Some(Ok(confirmed))
}

pub fn settled(svc: &crate::service::ServiceContext, tab: &str) {
    let title = svc.mux.read().get_tab(tab).map(|tab| tab.title.clone());
    if let Some(title) = title {
        svc.notify_event(
            "agent.task_complete",
            json!({"tab_id":tab,"title":format!("{title} finished"),"body":"Task complete"}),
        );
    }
    svc.prompt_detector.lock().clear(tab);
    if let Some(identity) = svc.clear_pending_prompt(tab) {
        svc.resolve_prompt_notifications(tab, &identity);
    }
    super::approvals::clear_tab(tab);
    svc.run_watcher.complete_via_stop_hook(svc, tab);
    svc.on_agent_turn_settled(tab);
    svc.complete_worker_turn_without_report(tab, crate::service::WorkerTurnSignal::ProviderStop);
    svc.schedule_session_model_refresh_after_stop(tab);
    svc.emit_state_changed();
}

pub fn turn_id(params: &Value) -> Option<&str> {
    params
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .or_else(|| params.get("turnId"))
        .and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_rpc_control_stop_deduplicates_and_interrupts_descendants() {
        let mut state = ControlState::default();
        state.parents.insert("child".into(), "root".into());
        state.started("root", "a");
        state.started("child", "b");
        state.started("foreign", "c");
        assert_eq!(state.descendants("root").len(), 2);
        assert!(state.completed("root", "a"));
        assert!(!state.completed("root", "a"));
        assert!(!state.active.contains_key("root"));
        state.interrupted.insert(("child".into(), "b".into()));
        assert!(state.was_interrupted("child", "b"));
    }
}

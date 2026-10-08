use super::lifecycle::{for_tab, rpc_error};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct ControlState {
    pub active: HashMap<String, String>,
    pub parents: HashMap<String, String>,
    pub retired_roots: HashSet<String>,
    completed: HashSet<(String, String)>,
    interrupted: HashSet<(String, String)>,
}

impl ControlState {
    pub fn started(&mut self, thread: &str, turn: &str) -> bool {
        if self.completed.contains(&(thread.into(), turn.into())) {
            return false;
        }
        self.active.insert(thread.into(), turn.into());
        true
    }
    pub fn restore(&mut self, thread: &str, active: Option<&str>, completed: &[String]) {
        self.active.remove(thread);
        self.completed
            .extend(completed.iter().map(|turn| (thread.into(), turn.clone())));
        if let Some(turn) = active {
            self.started(thread, turn);
        }
    }
    pub fn completed(&mut self, thread: &str, turn: &str) -> bool {
        let current = self.active.get(thread).is_none_or(|active| active == turn);
        if current {
            self.active.remove(thread);
        }
        self.completed.insert((thread.into(), turn.into())) && current
    }
    pub fn owns_thread(&self, root: &str, thread: &str) -> bool {
        let mut candidate = thread;
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
    }
    pub fn descendants(&self, root: &str) -> Vec<(String, String)> {
        self.active
            .iter()
            .filter(|(thread, _)| self.owns_thread(root, thread))
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
    if let Err(error) = interrupt_turns(&turns, |method, params| {
        sidecar.client.request(method, params)
    })
    .await
    {
        return Some(Err(rpc_error(error)));
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

pub(super) async fn interrupt_turns<F, Fut>(
    turns: &[(String, String)],
    mut request: F,
) -> Result<(), super::client::ClientError>
where
    F: FnMut(&'static str, Value) -> Fut,
    Fut: std::future::Future<Output = Result<Value, super::client::ClientError>>,
{
    for (thread, turn) in turns {
        request("turn/interrupt", json!({"threadId":thread,"turnId":turn})).await?;
    }
    Ok(())
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
    fn codex_rpc_control_restore_does_not_replay_stop_or_settle_a_newer_turn() {
        let mut state = ControlState::default();
        state.restore("root", Some("active"), &["historical".into()]);
        assert!(!state.started("root", "historical"));
        assert!(!state.completed("root", "historical"));
        assert!(!state.completed("root", "late"));
        assert_eq!(state.active.get("root").map(String::as_str), Some("active"));
        assert!(state.completed("root", "active"));
        state.parents.insert("child".into(), "root".into());
        state.parents.insert("grandchild".into(), "child".into());
        assert!(state.owns_thread("root", "grandchild"));
        assert!(!state.owns_thread("root", "foreign"));
        state.parents.insert("loop-a".into(), "loop-b".into());
        state.parents.insert("loop-b".into(), "loop-a".into());
        assert!(!state.owns_thread("root", "loop-a"));
    }
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

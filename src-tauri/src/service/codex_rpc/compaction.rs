use serde_json::{json, Value};
use std::collections::HashSet;

pub(crate) const CONTRACT: &str = "## Domios control\nDomios owns this interactive terminal session. Keep all work and replies in its bound thread. Approval requests are answered through Domios or the attached terminal. Treat model, effort and permission changes as per-thread controls; never persist them as machine-global defaults.";

pub fn developer_instructions(task: Option<&str>) -> String {
    match task.filter(|task| !task.trim().is_empty()) {
        Some(task) => format!("{CONTRACT}\n\n{task}"),
        None => CONTRACT.into(),
    }
}

#[derive(Default)]
pub struct CompactionState {
    completed: HashSet<(String, String)>,
    pub additional_context: serde_json::Map<String, Value>,
}

impl CompactionState {
    pub fn injection(
        &mut self,
        thread: &str,
        item: &Value,
        instructions: Option<&str>,
    ) -> Option<Value> {
        if item.get("type").and_then(Value::as_str) != Some("contextCompaction") {
            return None;
        }
        let id = item.get("id")?.as_str()?;
        if !self.completed.insert((thread.into(), id.into())) {
            return None;
        }
        let mut items = Vec::new();
        if let Some(text) = instructions.filter(|text| !text.is_empty()) {
            items.push(json!({"type":"message","role":"developer","content":[{"type":"input_text","text":text}]}));
        }
        for entry in self.additional_context.values() {
            if let Some(text) = entry.get("value").and_then(Value::as_str) {
                let role = if entry.get("kind").and_then(Value::as_str) == Some("application") {
                    "developer"
                } else {
                    "user"
                };
                items.push(json!({"type":"message","role":role,"content":[{"type":"input_text","text":text}]}));
            }
        }
        (!items.is_empty()).then(|| json!({"threadId":thread,"items":items}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_rpc_control_developer_contract_is_composed_without_task_loss() {
        assert_eq!(developer_instructions(None), CONTRACT);
        assert_eq!(
            developer_instructions(Some("opaque-task")),
            format!("{CONTRACT}\n\nopaque-task")
        );
    }
    #[test]
    fn codex_rpc_control_compaction_restores_contract_and_context_once_per_item() {
        let mut state = CompactionState::default();
        state.additional_context.insert(
            "owned-key".into(),
            json!({"kind":"application","value":"opaque-context"}),
        );
        let item = json!({"type":"contextCompaction","id":"compact-1"});
        let value = state
            .injection("bound", &item, Some("opaque-contract"))
            .unwrap();
        assert_eq!(
            value,
            json!({"threadId":"bound","items":[
                {"type":"message","role":"developer","content":[{"type":"input_text","text":"opaque-contract"}]},
                {"type":"message","role":"developer","content":[{"type":"input_text","text":"opaque-context"}]}
            ]})
        );
        assert!(state
            .injection("bound", &item, Some("opaque-contract"))
            .is_none());
        assert!(state
            .injection(
                "bound",
                &json!({"type":"agentMessage","id":"a"}),
                Some("opaque-contract")
            )
            .is_none());
        assert!(state
            .injection("new-thread", &item, Some("opaque-contract"))
            .is_some());
    }
}

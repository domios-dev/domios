use std::collections::HashMap;

use serde_json::Value;

use crate::service::plan::batch::BatchOp;
use crate::service::{AuthenticatedPlanCaller, ServiceContext};

impl ServiceContext {
    pub fn authorize_task_completion(
        &self,
        plan_id: &str,
        caller: &AuthenticatedPlanCaller,
        result: Option<&str>,
    ) -> Result<(), String> {
        if caller.tab_id.is_none() {
            return Err(
                "only the live Plan owner may complete a task; Workers submit for review".into(),
            );
        }
        self.authorize_plan_orchestrator(plan_id, caller)?;
        if result.is_none_or(|note| note.trim().is_empty()) {
            return Err("task completion requires a nonblank result note".into());
        }
        Ok(())
    }

    pub fn authorize_batch_completions(&self, plan_id: &str, body: &Value) -> Result<(), String> {
        let Some(ops) = body.get("ops") else {
            return Ok(());
        };
        let ops: Vec<BatchOp> = serde_json::from_value(ops.clone())
            .map_err(|error| format!("invalid batch ops: {error}"))?;
        let plan = self.plan_store.get(plan_id).ok_or("plan not found")?;
        let mut aliases = HashMap::new();
        let mut completions = Vec::new();
        let mut notes = Vec::new();
        let resolve = |selector: &str, aliases: &HashMap<String, String>| {
            let selector = aliases
                .get(selector)
                .map(String::as_str)
                .unwrap_or(selector);
            super::resolve_task(&plan.tasks, selector)
                .map(|task| task.id.clone())
                .unwrap_or_else(|| selector.to_string())
        };
        for op in ops {
            match op {
                BatchOp::UpsertTask {
                    client_id,
                    handle,
                    status,
                    ..
                } => {
                    let task = resolve(&handle, &aliases);
                    if status.as_deref() == Some("done") {
                        completions.push(task.clone());
                    }
                    if let Some(id) = client_id {
                        aliases.insert(format!("${id}"), task);
                    }
                }
                BatchOp::SetTaskStatus {
                    client_id,
                    task,
                    status,
                    ..
                } => {
                    let task = resolve(&task, &aliases);
                    if status == "done" {
                        completions.push(task.clone());
                    }
                    if let Some(id) = client_id {
                        aliases.insert(format!("${id}"), task);
                    }
                }
                BatchOp::SetTaskSections {
                    client_id: Some(id),
                    task,
                    ..
                } => {
                    aliases.insert(format!("${id}"), resolve(&task, &aliases));
                }
                BatchOp::AddMilestone { task, text, .. } if !text.trim().is_empty() => {
                    notes.push((resolve(&task, &aliases), text));
                }
                _ => {}
            }
        }
        let field = |key| body.get(key).and_then(Value::as_str).map(str::to_string);
        let caller = AuthenticatedPlanCaller {
            tab_id: field("caller_tab_id"),
            session_id: field("session_id"),
            binding_nonce: field("session_binding_nonce"),
        };
        for task in completions {
            let note = notes.iter().find(|(target, _)| target == &task);
            self.authorize_task_completion(plan_id, &caller, note.map(|(_, text)| text.as_str()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

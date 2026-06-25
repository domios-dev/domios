use serde::{Deserialize, Serialize};

use super::{PlanError, PlanStore};
use crate::plan::{HeartbeatLearning, OrchestratorHeartbeat, Plan, PlanStatus};

pub const STATE_RECONCILIATION_PROMPT: &str = concat!(
    "STATE-RECONCILIATION:\n",
    "- First run `codeterm agent reports` and process queued reports before inspecting panes.\n",
    "- Reconcile live panes against the plan: active tasks, pane status, branches, and worktrees.\n",
    "- Close verified tasks, bounce incomplete ones with the next concrete instruction, and record the plan milestone.\n",
    "- Clean finished lane worktrees/branches/dev daemons or record why cleanup is deferred.\n",
    "- Re-arm exactly one heartbeat carrying current focus and active lanes."
);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeartbeatLearnResult {
    pub plan_id: String,
    pub learning: HeartbeatLearning,
    pub deduped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeartbeatShowResult {
    pub plan_id: String,
    pub heartbeat: OrchestratorHeartbeat,
    pub reinjection: String,
}

fn gen_id() -> String {
    format!("learn_{}", uuid::Uuid::new_v4().simple())
}

fn normalized_learning(text: &str) -> String {
    let mut out = String::new();
    let mut last_space = true;
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_string()
}

fn render_list(items: &[String], out: &mut String) {
    for item in items.iter().filter(|item| !item.trim().is_empty()) {
        out.push_str("- ");
        out.push_str(item.trim());
        out.push('\n');
    }
}

fn render_heartbeat(plan_id: &str, heartbeat: &OrchestratorHeartbeat) -> String {
    let has_focus = heartbeat
        .current_focus
        .as_deref()
        .is_some_and(|focus| !focus.trim().is_empty());
    let has_lanes = heartbeat
        .active_lanes
        .iter()
        .any(|lane| !lane.trim().is_empty());
    let has_rules = heartbeat.rules.iter().any(|rule| !rule.trim().is_empty());
    let has_learnings = heartbeat
        .learnings
        .iter()
        .any(|learning| !learning.text.trim().is_empty());
    if !has_focus && !has_lanes && !has_rules && !has_learnings {
        return String::new();
    }
    let mut out = format!("<codeterm-orchestrator-heartbeat plan=\"{plan_id}\">\n");
    if let Some(focus) = heartbeat.current_focus.as_deref() {
        if !focus.trim().is_empty() {
            out.push_str("Current focus: ");
            out.push_str(focus.trim());
            out.push('\n');
        }
    }
    if has_lanes {
        out.push_str("Active lanes:\n");
        render_list(&heartbeat.active_lanes, &mut out);
    }
    if has_rules {
        out.push_str("Rules:\n");
        render_list(&heartbeat.rules, &mut out);
    }
    if has_learnings {
        out.push_str("Recent learnings:\n");
        for learning in heartbeat
            .learnings
            .iter()
            .rev()
            .filter(|learning| !learning.text.trim().is_empty())
            .take(12)
        {
            out.push_str("- ");
            out.push_str(learning.text.trim());
            out.push('\n');
        }
    }
    out.push_str("</codeterm-orchestrator-heartbeat>");
    out
}

fn prepend_heartbeat(plan_id: &str, heartbeat: &OrchestratorHeartbeat, text: &str) -> String {
    let block = render_heartbeat(plan_id, heartbeat);
    let reconciliation = format!(
        "<codeterm-state-reconciliation plan=\"{plan_id}\">\n{STATE_RECONCILIATION_PROMPT}\n</codeterm-state-reconciliation>"
    );
    let prelude = if block.is_empty() {
        reconciliation
    } else {
        format!("{reconciliation}\n\n{block}")
    };
    if text.trim().is_empty() {
        prelude
    } else {
        format!("{prelude}\n\n{text}")
    }
}

pub(super) fn prepend_plan_heartbeat(plan: &Plan, text: &str) -> String {
    prepend_heartbeat(&plan.id, &plan.heartbeat, text)
}

pub(super) fn prepend_heartbeat_snapshot(
    plan_id: &str,
    heartbeat: &OrchestratorHeartbeat,
    text: &str,
) -> String {
    prepend_heartbeat(plan_id, heartbeat, text)
}

impl PlanStore {
    pub fn resolve_active_plan_for_orchestrator(
        &self,
        plan_id: Option<&str>,
        pane_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<String, PlanError> {
        if let Some(plan_id) = plan_id.filter(|id| !id.trim().is_empty()) {
            return self
                .get(plan_id)
                .map(|plan| plan.id)
                .ok_or(PlanError::NotFound);
        }
        for entry in self.list_index() {
            let Some(plan) = self.get(&entry.plan_id) else {
                continue;
            };
            if plan.status != PlanStatus::Active {
                continue;
            }
            let pane_matches = pane_id
                .filter(|pane| !pane.trim().is_empty())
                .is_some_and(|pane| plan.owner.pane_id.as_deref() == Some(pane));
            let session_matches = session_id
                .filter(|session| !session.trim().is_empty())
                .is_some_and(|session| plan.owner.session_id.as_deref() == Some(session));
            if pane_matches || session_matches {
                return Ok(plan.id);
            }
        }
        Err(PlanError::NotFound)
    }

    pub fn add_heartbeat_learning(
        &self,
        plan_id: Option<&str>,
        pane_id: Option<&str>,
        session_id: Option<&str>,
        text: &str,
        tag: Option<&str>,
        now_ms: u64,
    ) -> Result<HeartbeatLearnResult, PlanError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(PlanError::Io("learning text is required".into()));
        }
        let plan_id = self.resolve_active_plan_for_orchestrator(plan_id, pane_id, session_id)?;
        let normalized = normalized_learning(text);
        let mut out = None;
        self.with_plan(&plan_id, now_ms, |plan| {
            if let Some(existing) = plan
                .heartbeat
                .learnings
                .iter()
                .find(|learning| normalized_learning(&learning.text) == normalized)
                .cloned()
            {
                out = Some(HeartbeatLearnResult {
                    plan_id: plan.id.clone(),
                    learning: existing,
                    deduped: true,
                });
                return Ok(());
            }
            let learning = HeartbeatLearning {
                id: gen_id(),
                text: text.to_string(),
                ts: now_ms,
                tag: tag
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                    .map(str::to_string),
            };
            plan.heartbeat.learnings.push(learning.clone());
            out = Some(HeartbeatLearnResult {
                plan_id: plan.id.clone(),
                learning,
                deduped: false,
            });
            Ok(())
        })?;
        out.ok_or_else(|| PlanError::Io("heartbeat learn produced no result".into()))
    }

    pub fn show_heartbeat(
        &self,
        plan_id: Option<&str>,
        pane_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<HeartbeatShowResult, PlanError> {
        let plan_id = self.resolve_active_plan_for_orchestrator(plan_id, pane_id, session_id)?;
        let plan = self.get(&plan_id).ok_or(PlanError::NotFound)?;
        Ok(HeartbeatShowResult {
            plan_id: plan.id.clone(),
            reinjection: render_heartbeat(&plan.id, &plan.heartbeat),
            heartbeat: plan.heartbeat,
        })
    }

    pub fn prepend_heartbeat_for_plan(&self, plan_id: &str, text: &str) -> String {
        let Some(plan) = self.get(plan_id) else {
            return text.to_string();
        };
        prepend_plan_heartbeat(&plan, text)
    }

    pub fn prepend_heartbeat_for_orchestrator_pane(&self, pane_id: &str, text: &str) -> String {
        let Ok(plan_id) = self.resolve_active_plan_for_orchestrator(None, Some(pane_id), None)
        else {
            return text.to_string();
        };
        self.prepend_heartbeat_for_plan(&plan_id, text)
    }
}

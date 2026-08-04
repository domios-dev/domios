use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::batch::{BatchOp, BatchRequest};
use super::{heartbeat, PlanError, PlanStore};
use crate::plan::{AgentRef, Plan, TaskStatus};
use codeterm_util::process_util;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HandoffCreateRequest {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub next_actions: Vec<String>,
    #[serde(default)]
    pub foundational_rules: Vec<String>,
    #[serde(default)]
    pub open_decisions: Vec<String>,
    #[serde(default)]
    pub narrative: Option<String>,
    #[serde(default)]
    pub from: Option<AgentRef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HandoffCreateResult {
    pub handoff_id: String,
    pub seq: u64,
    pub body_path: String,
    pub resume_prompt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HandoffDetail {
    pub id: String,
    pub seq: u64,
    pub content: Value,
    pub markdown: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HandoffListItem {
    pub id: String,
    pub seq: u64,
    pub mode: Option<String>,
    pub goal: Option<String>,
    pub ts: Option<u64>,
    pub reseed_state: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HandoffReseedRequest {
    #[serde(default)]
    pub selector: Option<String>,
    pub tab_id: String,
    pub source_session_id: String,
    #[serde(default = "default_reseed_mode")]
    pub mode: String,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HandoffReseedSend {
    pub order: u8,
    pub text: String,
    pub enter: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HandoffReseedResult {
    pub ok: bool,
    pub dry_run: bool,
    pub mode: String,
    pub plan_id: String,
    pub handoff_id: String,
    pub tab_id: String,
    pub source_session_id: String,
    pub sends: Vec<HandoffReseedSend>,
    pub state: String,
}

#[derive(Debug, Clone)]
pub struct HandoffResumeSend {
    pub plan_id: String,
    pub handoff_id: String,
    pub tab_id: String,
    pub text: String,
}

fn default_reseed_mode() -> String {
    "new".into()
}

fn git_value(project_path: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    process_util::no_window_std(&mut cmd);
    let output = cmd.arg("-C").arg(project_path).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn git_dirty(project_path: &str) -> bool {
    git_value(project_path, &["status", "--porcelain"])
        .map(|value| value.lines().any(|line| !is_sidecar_status_line(line)))
        .unwrap_or(false)
}

fn is_sidecar_status_line(line: &str) -> bool {
    let Some(rest) = line.get(3..) else {
        return false;
    };
    let mut paths = rest
        .split(" -> ")
        .map(|s| s.trim().trim_matches('"'))
        .filter(|s| !s.is_empty())
        .peekable();
    paths.peek().is_some()
        && paths.all(|p| p == ".codeterm/plans" || p.starts_with(".codeterm/plans/"))
}

fn content_seq(content: &Value) -> u64 {
    content.get("seq").and_then(|v| v.as_u64()).unwrap_or(0)
}

fn derive_goal(req: &HandoffCreateRequest, plan: &Plan) -> String {
    req.goal
        .clone()
        .or_else(|| {
            req.narrative
                .as_deref()
                .and_then(|body| body.lines().find(|line| !line.trim().is_empty()))
                .map(|line| line.trim().to_string())
        })
        .unwrap_or_else(|| plan.title.clone())
}

fn resume_prompt(plan: &Plan, seq: u64) -> String {
    format!(
        "Run `codeterm plan handoff show {} --latest --markdown` FIRST for handoff #{}. Load docs/contributing/agents.md and AGENTS.md, run `codeterm plan get {}`, resume from next_actions, and do not re-handoff.",
        plan.id, seq, plan.id
    )
}

fn plan_counts(plan: &Plan) -> (usize, usize) {
    let total = plan.tasks.len();
    let done = plan
        .tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled))
        .count();
    (done, total)
}

fn task_handle(task: &crate::plan::Task) -> String {
    task.handle.clone().unwrap_or_else(|| task.id.clone())
}

fn compose_content(plan: &Plan, req: &HandoffCreateRequest, seq: u64) -> Value {
    let (done, total) = plan_counts(plan);
    let coverage_pct = if total == 0 {
        0
    } else {
        ((done as f64 / total as f64) * 100.0).round() as u64
    };
    let in_flight = plan
        .tasks
        .iter()
        .filter(|task| !matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled))
        .map(|task| {
            json!({
                "task_id": task.id,
                "task_handle": task_handle(task),
                "status": task.status,
                "assigned_tab_id": task.assigned_tab_id,
                "peer": task.peer,
            })
        })
        .collect::<Vec<_>>();
    let milestone_decisions = plan
        .tasks
        .iter()
        .flat_map(|task| task.milestones.iter())
        .filter(|ms| ms.kind == "decision")
        .map(|ms| ms.text.clone())
        .collect::<Vec<_>>();
    let open_decisions = if req.open_decisions.is_empty() {
        milestone_decisions
    } else {
        req.open_decisions.clone()
    };
    json!({
        "goal": derive_goal(req, plan),
        "plan_snapshot": {
            "plan_id": plan.id,
            "plan_title": plan.title,
            "done": done,
            "total": total,
            "coverage_pct": coverage_pct,
            "updated_at": plan.updated_at,
        },
        "in_flight": in_flight,
        "next_actions": req.next_actions,
        "foundational_rules": req.foundational_rules,
        "open_decisions": open_decisions,
        "integrity": {
            "git": {
                "branch": git_value(&plan.project_path, &["branch", "--show-current"]),
                "head": git_value(&plan.project_path, &["rev-parse", "--short", "HEAD"]),
                "dirty": git_dirty(&plan.project_path),
            },
            "plan_updated_at": plan.updated_at,
        },
        "resume_prompt": resume_prompt(plan, seq),
    })
}

fn render_markdown(content: &Value, body: &str) -> String {
    let mut out = String::new();
    let seq = content_seq(content);
    let goal = content.get("goal").and_then(|v| v.as_str()).unwrap_or("");
    out.push_str(&format!("# Handoff #{}\n\n", seq));
    if !goal.is_empty() {
        out.push_str(&format!("Goal: {}\n\n", goal));
    }
    if let Some(items) = content.get("next_actions").and_then(|v| v.as_array()) {
        out.push_str("## Next Actions\n\n");
        for item in items.iter().filter_map(|v| v.as_str()) {
            out.push_str(&format!("- {}\n", item));
        }
        out.push('\n');
    }
    if !body.trim().is_empty() {
        out.push_str("## Narrative\n\n");
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

fn content_str(content: &Value, key: &str) -> Option<String> {
    content
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn active_reseed_state(state: &str) -> bool {
    matches!(state, "new_sent" | "session_seen" | "resume_sent")
}

fn pending_for_tab(content: &Value, tab_id: &str) -> bool {
    content
        .get("reseed_pending")
        .and_then(|v| v.as_object())
        .and_then(|obj| obj.get("tab_id"))
        .and_then(|v| v.as_str())
        == Some(tab_id)
}

fn element_has_active_reseed_for_tab(element: &crate::plan::CanvasElement, tab_id: &str) -> bool {
    element.kind == "handoff"
        && pending_for_tab(&element.content, tab_id)
        && reseed_state(&element.content).is_some_and(active_reseed_state)
}

fn plan_has_active_reseed_for_tab(plan: &Plan, tab_id: &str) -> bool {
    plan.elements
        .iter()
        .any(|element| element_has_active_reseed_for_tab(element, tab_id))
}

fn reseed_state(content: &Value) -> Option<&str> {
    content.get("reseed_state").and_then(|v| v.as_str())
}

fn select_handoff_index(plan: &Plan, selector: Option<&str>) -> Option<usize> {
    if let Some(selector) = selector {
        plan.elements.iter().position(|element| {
            element.kind == "handoff"
                && (element.id == selector
                    || selector.parse::<u64>().ok() == Some(content_seq(&element.content)))
        })
    } else {
        plan.elements
            .iter()
            .enumerate()
            .filter(|(_, element)| element.kind == "handoff")
            .max_by_key(|(_, element)| content_seq(&element.content))
            .map(|(idx, _)| idx)
    }
}

impl PlanStore {
    pub fn create_handoff(
        &self,
        plan_id: &str,
        req: HandoffCreateRequest,
        now_ms: u64,
    ) -> Result<HandoffCreateResult, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let seq = plan
            .elements
            .iter()
            .filter(|element| element.kind == "handoff")
            .map(|element| content_seq(&element.content))
            .max()
            .unwrap_or(0)
            + 1;
        let content = compose_content(&plan, &req, seq);
        let result = self.apply_batch(
            plan_id,
            BatchRequest {
                key: None,
                ops: vec![BatchOp::AddHandoff {
                    client_id: Some("handoff".into()),
                    mode: req.mode.clone(),
                    content,
                    body: req.narrative.clone().unwrap_or_default(),
                    task: req.task.clone(),
                    from: req.from.clone(),
                    to: req.from.clone(),
                }],
            },
            now_ms,
        )?;
        let handoff_id = result
            .ids
            .get("handoff")
            .cloned()
            .ok_or_else(|| PlanError::Io("handoff id missing".into()))?;
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let element = plan
            .elements
            .iter()
            .find(|element| element.id == handoff_id)
            .ok_or(PlanError::NotFound)?;
        Ok(HandoffCreateResult {
            handoff_id,
            seq,
            body_path: element
                .content
                .get("body_path")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            resume_prompt: element
                .content
                .get("resume_prompt")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        })
    }

    pub fn list_handoffs(&self, plan_id: &str) -> Result<Vec<HandoffListItem>, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let mut items = plan
            .elements
            .iter()
            .filter(|element| element.kind == "handoff")
            .map(|element| HandoffListItem {
                id: element.id.clone(),
                seq: content_seq(&element.content),
                mode: element
                    .content
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                goal: element
                    .content
                    .get("goal")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                ts: element.content.get("ts").and_then(|v| v.as_u64()),
                reseed_state: element
                    .content
                    .get("reseed_state")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            })
            .collect::<Vec<_>>();
        items.sort_by_key(|item| item.seq);
        Ok(items)
    }

    pub fn show_handoff(
        &self,
        plan_id: &str,
        selector: Option<&str>,
    ) -> Result<HandoffDetail, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let element = if let Some(selector) = selector {
            plan.elements
                .iter()
                .filter(|element| element.kind == "handoff")
                .find(|element| {
                    element.id == selector
                        || selector.parse::<u64>().ok() == Some(content_seq(&element.content))
                })
        } else {
            plan.elements
                .iter()
                .filter(|element| element.kind == "handoff")
                .max_by_key(|element| content_seq(&element.content))
        }
        .ok_or(PlanError::NotFound)?;
        let path = super::body_files::handoff_path(&plan, &element.id)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        let body = std::fs::read_to_string(path).unwrap_or_default();
        Ok(HandoffDetail {
            id: element.id.clone(),
            seq: content_seq(&element.content),
            content: element.content.clone(),
            markdown: render_markdown(&element.content, &body),
        })
    }

    pub fn prepare_handoff_reseed(
        &self,
        plan_id: &str,
        req: &HandoffReseedRequest,
        provider_command: &str,
        resume_seed: &str,
        now_ms: u64,
    ) -> Result<HandoffReseedResult, PlanError> {
        if req.source_session_id.trim().is_empty() {
            return Err(PlanError::Io("source_session_id is required".into()));
        }
        if req.mode != "new" && req.mode != "compact" {
            return Err(PlanError::Io("mode must be new or compact".into()));
        }
        let _guard = self.reseed_prepare_lock.lock();
        self.reject_active_reseed_for_tab(&req.tab_id, req.force, Some(plan_id))?;
        let selector = req.selector.clone();
        self.with_plan(plan_id, now_ms, |plan| {
            if !req.force && plan_has_active_reseed_for_tab(plan, &req.tab_id) {
                return Err(PlanError::Io(format!(
                    "handoff reseed already pending for tab {}; use --force to override",
                    req.tab_id
                )));
            }
            let idx = select_handoff_index(plan, selector.as_deref()).ok_or(PlanError::NotFound)?;
            let element = &mut plan.elements[idx];
            let state = if req.mode == "compact" {
                "resume_sent"
            } else {
                "new_sent"
            };
            if req.dry_run {
                return Ok(HandoffReseedResult {
                    ok: true,
                    dry_run: true,
                    mode: req.mode.clone(),
                    plan_id: plan.id.clone(),
                    handoff_id: element.id.clone(),
                    tab_id: req.tab_id.clone(),
                    source_session_id: req.source_session_id.clone(),
                    sends: reseed_sends(&req.mode, provider_command, resume_seed),
                    state: "dry_run".into(),
                });
            }
            let obj = element
                .content
                .as_object_mut()
                .ok_or_else(|| PlanError::Io("handoff content must be an object".into()))?;
            obj.insert("reseed_state".into(), Value::from(state));
            obj.insert(
                "reseed_pending".into(),
                json!({
                    "tab_id": req.tab_id,
                    "handoff_id": element.id,
                    "source_session_id": req.source_session_id,
                    "state": state,
                    "mode": req.mode,
                    "created_at": now_ms,
                    "updated_at": now_ms,
                    "resume_prompt": resume_seed,
                }),
            );
            Ok(HandoffReseedResult {
                ok: true,
                dry_run: false,
                mode: req.mode.clone(),
                plan_id: plan.id.clone(),
                handoff_id: element.id.clone(),
                tab_id: req.tab_id.clone(),
                source_session_id: req.source_session_id.clone(),
                sends: reseed_sends(&req.mode, provider_command, resume_seed),
                state: state.into(),
            })
        })
    }

    fn reject_active_reseed_for_tab(
        &self,
        tab_id: &str,
        force: bool,
        except_plan_id: Option<&str>,
    ) -> Result<(), PlanError> {
        if force {
            return Ok(());
        }
        for entry in self.list_index() {
            if except_plan_id == Some(entry.plan_id.as_str()) {
                continue;
            }
            if let Some(plan) = self.get(&entry.plan_id) {
                if plan_has_active_reseed_for_tab(&plan, tab_id) {
                    return Err(PlanError::Io(format!(
                        "handoff reseed already pending for tab {tab_id}; use --force to override"
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn observe_handoff_session_start(
        &self,
        tab_id: &str,
        new_session_id: &str,
        now_ms: u64,
    ) -> Result<Option<HandoffResumeSend>, PlanError> {
        for entry in self.list_index() {
            let Some(plan_snapshot) = self.get(&entry.plan_id) else {
                continue;
            };
            let candidate = plan_snapshot
                .elements
                .iter()
                .filter(|element| element.kind == "handoff")
                .find(|element| {
                    pending_for_tab(&element.content, tab_id)
                        && reseed_state(&element.content) == Some("new_sent")
                        && element
                            .content
                            .get("reseed_pending")
                            .and_then(|v| v.get("source_session_id"))
                            .and_then(|v| v.as_str())
                            .is_some_and(|source| source != new_session_id)
                })
                .map(|element| element.id.clone());
            let Some(handoff_id) = candidate else {
                continue;
            };
            let mut out = None;
            self.with_plan(&entry.plan_id, now_ms, |plan| {
                let heartbeat_plan_id = plan.id.clone();
                let heartbeat = plan.heartbeat.clone();
                let Some(element) = plan
                    .elements
                    .iter_mut()
                    .find(|element| element.id == handoff_id && element.kind == "handoff")
                else {
                    return Ok(());
                };
                let fallback_resume_prompt =
                    content_str(&element.content, "resume_prompt").unwrap_or_default();
                let Some(obj) = element.content.as_object_mut() else {
                    return Ok(());
                };
                let Some(pending) = obj
                    .get_mut("reseed_pending")
                    .and_then(|v| v.as_object_mut())
                else {
                    return Ok(());
                };
                if pending.get("state").and_then(|v| v.as_str()) != Some("new_sent") {
                    return Ok(());
                }
                let text = pending
                    .get("resume_prompt")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .unwrap_or(fallback_resume_prompt);
                let text = heartbeat::prepend_heartbeat_snapshot(
                    &heartbeat_plan_id,
                    &heartbeat,
                    &plan.loops,
                    &text,
                );
                pending.insert("state".into(), Value::from("session_seen"));
                pending.insert("target_session_id".into(), Value::from(new_session_id));
                pending.insert("updated_at".into(), Value::from(now_ms));
                pending.insert("session_seen_at".into(), Value::from(now_ms));
                obj.insert("reseed_state".into(), Value::from("session_seen"));
                out = Some(HandoffResumeSend {
                    plan_id: plan.id.clone(),
                    handoff_id: element.id.clone(),
                    tab_id: tab_id.to_string(),
                    text,
                });
                Ok(())
            })?;
            if out.is_some() {
                return Ok(out);
            }
        }
        Ok(None)
    }

    pub fn mark_handoff_resume_sent(
        &self,
        plan_id: &str,
        handoff_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let Some(element) = plan
                .elements
                .iter_mut()
                .find(|element| element.id == handoff_id && element.kind == "handoff")
            else {
                return Ok(());
            };
            let Some(obj) = element.content.as_object_mut() else {
                return Ok(());
            };
            let Some(pending) = obj
                .get_mut("reseed_pending")
                .and_then(|v| v.as_object_mut())
            else {
                return Ok(());
            };
            if pending.get("state").and_then(|v| v.as_str()) != Some("session_seen") {
                return Ok(());
            }
            pending.insert("state".into(), Value::from("resume_sent"));
            pending.insert("resume_sent_at".into(), Value::from(now_ms));
            pending.insert("updated_at".into(), Value::from(now_ms));
            obj.insert("reseed_state".into(), Value::from("resume_sent"));
            Ok(())
        })
    }

    pub fn ack_handoff_resume(
        &self,
        plan_id: &str,
        handoff_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let Some(element) = plan
                .elements
                .iter_mut()
                .find(|element| element.id == handoff_id && element.kind == "handoff")
            else {
                return Ok(());
            };
            let Some(obj) = element.content.as_object_mut() else {
                return Ok(());
            };
            let Some(pending) = obj
                .get_mut("reseed_pending")
                .and_then(|v| v.as_object_mut())
            else {
                return Ok(());
            };
            if pending.get("state").and_then(|v| v.as_str()) != Some("resume_sent") {
                return Ok(());
            }
            pending.insert("state".into(), Value::from("acked"));
            pending.insert("acked_at".into(), Value::from(now_ms));
            pending.insert("updated_at".into(), Value::from(now_ms));
            obj.insert("reseed_state".into(), Value::from("acked"));
            Ok(())
        })
    }
}

fn reseed_sends(mode: &str, provider_command: &str, resume_seed: &str) -> Vec<HandoffReseedSend> {
    if mode == "compact" {
        vec![
            HandoffReseedSend {
                order: 1,
                text: resume_seed.to_string(),
                enter: true,
                reason: "resume_seed".into(),
            },
            HandoffReseedSend {
                order: 2,
                text: provider_command.to_string(),
                enter: true,
                reason: "provider_compact".into(),
            },
        ]
    } else {
        vec![
            HandoffReseedSend {
                order: 1,
                text: provider_command.to_string(),
                enter: true,
                reason: "provider_fresh_context".into(),
            },
            HandoffReseedSend {
                order: 2,
                text: resume_seed.to_string(),
                enter: true,
                reason: "session_start_resume_seed".into(),
            },
        ]
    }
}

#[cfg(test)]
mod tests;

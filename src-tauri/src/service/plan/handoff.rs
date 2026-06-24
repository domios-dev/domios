use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::batch::{BatchOp, BatchRequest};
use super::{PlanError, PlanStore};
use crate::plan::{AgentRef, Plan, TaskStatus};

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

fn git_value(project_path: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn git_dirty(project_path: &str) -> bool {
    git_value(project_path, &["status", "--porcelain"])
        .map(|value| !value.is_empty())
        .unwrap_or(false)
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
                "assigned_pane_id": task.assigned_pane_id,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{MilestoneKind, OrchestratorRef};
    use crate::service::plan::NewTask;

    fn store_with_task() -> (tempfile::TempDir, PlanStore, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let plan = store
            .create_plan(
                "Canvas Handoff".into(),
                dir.path().display().to_string(),
                OrchestratorRef::from_pane("orchestrator"),
                vec![MilestoneKind {
                    key: "decision".into(),
                    label: "decision".into(),
                    color: None,
                }],
                10,
            )
            .unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "Backend".into(),
                    kind: "implement".into(),
                    handle: Some("backend".into()),
                    ..Default::default()
                },
                20,
            )
            .unwrap();
        (dir, store, plan.id, task_id)
    }

    #[test]
    fn create_handoff_adds_element_milestone_history_and_sidecar() {
        let (dir, store, plan_id, task_id) = store_with_task();
        let result = store
            .create_handoff(
                &plan_id,
                HandoffCreateRequest {
                    mode: Some("record".into()),
                    task: Some(task_id.clone()),
                    goal: Some("Land backend handoff".into()),
                    next_actions: vec!["Run handoff show first".into()],
                    foundational_rules: vec!["Never reseed parent pane".into()],
                    open_decisions: vec!["Choose reseed ack source".into()],
                    narrative: Some("Overlay body\n".into()),
                    from: Some(AgentRef::from_pane("worker")),
                },
                30,
            )
            .unwrap();

        let plan = store.get(&plan_id).unwrap();
        let element = plan
            .elements
            .iter()
            .find(|element| element.id == result.handoff_id)
            .unwrap();
        assert_eq!(element.kind, "handoff");
        assert_eq!(element.content["seq"], 1);
        assert_eq!(element.content["goal"], "Land backend handoff");
        assert_eq!(element.content["reseed_state"], "created");
        assert!(element.links.iter().any(|link| link == &task_id));
        assert!(plan
            .milestone_kinds
            .iter()
            .any(|kind| kind.key == "handoff"));
        assert_eq!(plan.owner_history.last().unwrap().reason, "handoff #1");
        let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
        assert!(task.milestones.iter().any(|ms| {
            ms.kind == "handoff" && ms.links.iter().any(|link| link == &result.handoff_id)
        }));

        let sidecar = dir.path().join(&result.body_path);
        assert_eq!(std::fs::read_to_string(sidecar).unwrap(), "Overlay body\n");
        let index = std::fs::read_to_string(
            dir.path()
                .join(".codeterm")
                .join("plans")
                .join(&plan_id)
                .join("INDEX.md"),
        )
        .unwrap();
        assert!(index.contains("## Handoffs"));
        assert!(index.contains(&result.handoff_id));
    }

    #[test]
    fn show_latest_handoff_returns_markdown_and_list_is_ordered() {
        let (_dir, store, plan_id, task_id) = store_with_task();
        store
            .create_handoff(
                &plan_id,
                HandoffCreateRequest {
                    task: Some(task_id.clone()),
                    goal: Some("First".into()),
                    narrative: Some("First body".into()),
                    ..HandoffCreateRequest::default()
                },
                30,
            )
            .unwrap();
        let second = store
            .create_handoff(
                &plan_id,
                HandoffCreateRequest {
                    task: Some(task_id),
                    goal: Some("Second".into()),
                    next_actions: vec!["Continue from next action".into()],
                    narrative: Some("Second body".into()),
                    ..HandoffCreateRequest::default()
                },
                40,
            )
            .unwrap();

        let list = store.list_handoffs(&plan_id).unwrap();
        assert_eq!(
            list.iter().map(|item| item.seq).collect::<Vec<_>>(),
            vec![1, 2]
        );
        let detail = store.show_handoff(&plan_id, None).unwrap();
        assert_eq!(detail.id, second.handoff_id);
        assert!(detail.markdown.contains("# Handoff #2"));
        assert!(detail.markdown.contains("- Continue from next action"));
        assert!(detail.markdown.contains("Second body"));
        assert_eq!(store.show_handoff(&plan_id, Some("1")).unwrap().seq, 1);
    }
}

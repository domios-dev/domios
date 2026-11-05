use super::store_db::hot_write::runtime::HeartbeatRows;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{PlanError, PlanStore};
use crate::plan::{HeartbeatLearning, LoopSpec, OrchestratorHeartbeat, Plan, PlanMode, TaskStatus};

pub(super) mod view;

pub(in crate::service_plan) const HANDOFF_ACTION_VERB: &str = "recover handoff";

pub const STATE_RECONCILIATION_PROMPT: &str = "STATE-RECONCILIATION";

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
    // A structured count, not text, so callers never have to parse `reinjection` to know it.
    pub blocked_or_gated_count: usize,
}

fn gen_id() -> String {
    crate::plan::ids::short_id("learn")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
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

pub(super) fn competitive_attempt_summary_line(
    task_id: &str,
    task_name: &str,
    live_attempts: usize,
) -> String {
    format!("- {task_id}: compare {live_attempts} attempts ({task_name})")
}

fn render_competitive_attempts(plan: &Plan, out: &mut String) {
    let summaries = plan
        .tasks
        .iter()
        .filter_map(|task| {
            let live_attempts = super::task_attempts::live_attempt_count(task);
            (live_attempts > 1
                && task.status == TaskStatus::InReview
                && crate::plan::reconcile::active_review_hold_note(task).is_none())
            .then(|| competitive_attempt_summary_line(&task.id, &task.name, live_attempts))
        })
        .collect::<Vec<_>>();
    if summaries.is_empty() {
        return;
    }
    out.push_str(super::prompts_plan::HEARTBEAT_COMPETITIVE_ATTEMPTS_TITLE);
    out.push('\n');
    for summary in summaries {
        out.push_str(&summary);
        out.push('\n');
    }
}

fn render_regression_gates(plan: &Plan, out: &mut String) {
    out.push_str(&regression_line(
        &super::milestone_gates::pending_regression_gates(plan),
        false,
    ));
}

fn render_group_retrospectives(plan: &Plan, out: &mut String) {
    out.push_str(&regression_line(
        &super::milestone_gates::pending_group_retrospectives(plan),
        true,
    ));
}

pub(in crate::service_plan) fn regression_line(
    gates: &[super::milestone_gates::RegressionGate],
    retrospective: bool,
) -> String {
    if gates.is_empty() {
        return String::new();
    }
    let title = if retrospective {
        "Close groups"
    } else {
        "Record group regression"
    };
    let groups = gates
        .iter()
        .map(|gate| gate.key.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!("{title}: {groups}\n")
}

fn render_open_gates(plan: &Plan, now_ms: u64, out: &mut String) {
    let open: Vec<_> = plan.gates.iter().filter(|g| g.is_open()).collect();
    if open.is_empty() {
        return;
    }
    out.push_str("Decision gates:\n");
    for gate in open {
        out.push_str(&gate_line(
            &gate.id,
            &gate.question,
            gate.asked_at,
            gate.blocking_task_ids.len(),
            gate.routed_to_tab_id.as_deref(),
            now_ms,
        ));
    }
}

pub(in crate::service_plan) fn gate_line(
    id: &str,
    question: &str,
    asked_at: u64,
    blocking: usize,
    routed: Option<&str>,
    now_ms: u64,
) -> String {
    let age_ms = now_ms.saturating_sub(asked_at);
    let age_h = age_ms / 3_600_000;
    let age_min = (age_ms % 3_600_000) / 60_000;
    let age = if age_h > 0 {
        format!("{age_h}h{age_min}m")
    } else {
        format!("{age_min}m")
    };
    format!(
        "- {id} (age:{age} blocking:{blocking} routed:{}): {}\n",
        routed.unwrap_or("unrouted"),
        question.chars().take(100).collect::<String>()
    )
}

fn role_identity_line(
    plan_id: &str,
    owner: Option<&str>,
    planner: Option<&str>,
    tab_id: Option<&str>,
) -> String {
    let role = match tab_id {
        Some(tab) if owner == Some(tab) => "Orchestrator",
        Some(tab) if planner == Some(tab) => "Planner",
        _ => "Orchestrator",
    };
    let peer_role = if role == "Planner" {
        "Orchestrator"
    } else {
        "Planner"
    };
    let peer = if role == "Planner" { owner } else { planner };
    let peer_text = peer
        .map(|tab| format!("; peer tab {peer_role} is tab {tab}"))
        .unwrap_or_else(|| format!("; peer tab {peer_role} is unbound"));
    codeterm_types::agent::role_identity::render(
        codeterm_types::agent::role_identity::RoleIdentity::Heartbeat {
            role,
            plan_id,
            peer_text: &peer_text,
        },
    )
}

pub const HEARTBEAT_BLOCK_OPEN: &str = "<codeterm-orchestrator-heartbeat";
pub const HEARTBEAT_MORE: &str =
    "More: domios docs ORCHESTRATION; all learnings: domios orchestrator heartbeat show";
pub const MAX_SITUATIONAL_LEARNINGS: usize = 3;

fn is_owner_rule(learning: &HeartbeatLearning) -> bool {
    learning.tag.as_deref() == Some("owner")
}

pub(in crate::service_plan) fn render_learning(learning: &HeartbeatLearning, out: &mut String) {
    render_learning_text(&learning.text, out);
}

pub(in crate::service_plan) fn render_learning_text(text: &str, out: &mut String) {
    out.push_str("- ");
    for (index, word) in text.split_whitespace().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        out.push_str(word);
    }
    out.push('\n');
}

const SITUATIONAL_LEARNING_CHARS: usize = 160;

pub(in crate::service_plan) fn render_situational_learning(
    learning: &HeartbeatLearning,
    out: &mut String,
) {
    render_situational_learning_fields(&learning.id, &learning.text, out);
}

pub(in crate::service_plan) fn render_situational_learning_fields(
    id: &str,
    text: &str,
    out: &mut String,
) {
    let normalized = text
        .split_whitespace()
        .enumerate()
        .flat_map(|(index, word)| {
            std::iter::once(' ')
                .filter(move |_| index > 0)
                .chain(word.chars())
        })
        .take(SITUATIONAL_LEARNING_CHARS + 1)
        .collect::<String>();
    let excerpt = if normalized.chars().count() > SITUATIONAL_LEARNING_CHARS {
        let prefix = normalized
            .chars()
            .take(SITUATIONAL_LEARNING_CHARS)
            .collect::<String>();
        let boundary = prefix.rfind(' ').unwrap_or(prefix.len());
        format!("{}…", &prefix[..boundary])
    } else {
        normalized
    };
    out.push_str(&format!("- {id}: {excerpt}\n"));
}

fn reminder_snapshot(plan: &Plan) -> super::state::PlanStateSnapshot {
    super::state::build_plan_state_snapshot(
        plan,
        crate::plan::coverage::compute_coverage_report(plan),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        &Default::default(),
        None,
        now_ms(),
    )
}

pub(in crate::service_plan) fn action_verb(
    kind: super::next_actions::NextActionKind,
) -> Option<&'static str> {
    use super::next_actions::NextActionKind;
    match kind {
        NextActionKind::Gate => Some("gate"),
        NextActionKind::ClaimQa => Some("claim QA"),
        NextActionKind::Dispatch => Some("dispatch"),
        NextActionKind::Advance => Some("unblock"),
        NextActionKind::WaitingOnDependency => Some("repair missing dependency"),
        _ => None,
    }
}

fn render_actions(
    plan: &Plan,
    snapshot: &super::state::PlanStateSnapshot,
    out: &mut String,
) -> Vec<String> {
    use super::next_actions::NextActionKind;
    let actionable = super::state::orchestrator_actionable_task_ids(plan);
    let mut named = std::collections::HashSet::new();
    let mut actions = Vec::new();
    for action in &snapshot.next_actions {
        let Some(verb) = action_verb(action.kind) else {
            if action.kind == NextActionKind::ConsiderStop {
                out.push_str(&format!("- {}\n", action.message));
            }
            continue;
        };
        if let Some(id) = action.task_id.as_ref().filter(|id| actionable.contains(id)) {
            if named.insert(id.clone()) {
                out.push_str(&format!("- {id}: {verb}\n"));
                actions.push(id.clone());
            }
        }
    }
    for task in &plan.tasks {
        if actionable.contains(&task.id)
            && !named.contains(&task.id)
            && super::state::task_projection(plan, task).presentation
                == super::state::TaskPresentation::HandoffRequired
        {
            out.push_str(&format!("- {}: {HANDOFF_ACTION_VERB}\n", task.id));
            actions.push(task.id.clone());
        }
    }
    actions
}

fn render_actionable_skills(plan: &Plan, actions: &[String], out: &mut String) {
    if actions.is_empty() {
        return;
    }
    for skill in super::skill_registry::list_plan_skill_summaries(plan) {
        let referenced = plan.tasks.iter().any(|task| {
            actions.contains(&task.id)
                && [
                    Some(task.body.as_str()),
                    task.test_plan.as_deref(),
                    task.qa_plan.as_deref(),
                ]
                .into_iter()
                .flatten()
                .any(|text| {
                    text.split(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
                        .any(|token| token == skill.slug)
                })
        });
        if referenced {
            out.push_str(&format!(
                "- skill {}: domios plan skill show --plan {} {}\n",
                skill.slug, plan.id, skill.slug
            ));
        }
    }
}

pub(super) fn reconciliation_prompt(plan_id: &str, tab_id: Option<&str>) -> String {
    let status = tab_id.map_or_else(
        || format!("domios orchestrator status --plan {plan_id}"),
        |tab| format!("domios orchestrator status --plan {plan_id} --tab {tab}"),
    );
    format!(
        "STATE-RECONCILIATION:\n\
         - This tab owns an execution-mode plan with unfinished work; act on actionable tasks, and let dependency-waiting or machine-paused tasks wait.\n\
         - If you forgot how to orchestrate, read `domios docs ORCHESTRATION` first.\n\
         - Worker reports are pushed into this tab automatically; judge each incoming report without polling an inbox.\n\
         - After dispatching a worker, end your turn immediately. Never sleep, wait, poll, or call `tab inspect`, `tab get-text`, `orchestrator status`, or inbox checks just to keep the turn open or watch progress; resume only when a worker report, user message, or system event is delivered automatically. Periodic-progress duties do not apply while waiting on a pushed report.\n\
         - Read the compact plan-owned status with `{status}`.\n\
         - Use `domios orchestrator inspect --plan {plan_id} --json` only when a specific discrepancy requires full report bodies or raw lane state.\n\
         - Reconcile live tabs against the plan with `domios plan reconcile {plan_id}`.\n\
         - {} \n\
         - Parallelize independent writer lanes, but serialize broad/heavy gates in one warm integration checkout so worker worktrees do not all rebuild/test at once.\n\
         - Close verified tasks with `domios plan task close {plan_id} <task_id>`.\n\
         - Bounce incomplete work with `domios send \"<next concrete instruction>\" --tab <worker_tab_id>` and record `domios plan record {plan_id} --task <task_id> --step decision:<text>`.\n\
         - Clean finished lane worktrees/branches/dev daemons, then record `domios plan task update {plan_id} <task_id> --cleanup-status done` or why cleanup is deferred.\n\
         - Heartbeat is derived automatically from plan and task state; never maintain heartbeat fields manually.",
        super::prompts_plan::QA_PIPELINE_DISCIPLINE.trim()
    )
}

fn prepend_heartbeat(
    plan_id: &str,
    heartbeat: &OrchestratorHeartbeat,
    loops: &[LoopSpec],
    text: &str,
    tab_id: Option<&str>,
) -> String {
    let parts = view::HeartbeatParts::from_snapshot(plan_id, loops);
    let block = parts.render_block(&view::learning_lines(heartbeat), &[]);
    let identity = role_identity_line(plan_id, None, None, tab_id);
    let prompt = reconciliation_prompt(plan_id, tab_id);
    let reconciliation = format!(
        "<codeterm-state-reconciliation plan=\"{plan_id}\">\n{prompt}\n</codeterm-state-reconciliation>"
    );
    let prelude = if block.is_empty() {
        format!("{identity}\n\n{reconciliation}")
    } else {
        format!("{identity}\n\n{reconciliation}\n\n{block}")
    };
    if text.trim().is_empty() {
        prelude
    } else {
        format!("{prelude}\n\n{text}")
    }
}

pub fn prepend_plan_heartbeat(plan: &Plan, text: &str) -> String {
    prepend_plan_heartbeat_with_digests(plan, text, &[])
}

pub fn prepend_plan_heartbeat_with_digests(
    plan: &Plan,
    text: &str,
    digest_lines: &[String],
) -> String {
    prepend_plan_heartbeat_with_selection(plan, text, digest_lines, None)
}

pub fn prepend_plan_heartbeat_with_selection(
    plan: &Plan,
    text: &str,
    digest_lines: &[String],
    learning_ids: Option<&[String]>,
) -> String {
    if matches!(plan.mode, PlanMode::Paused | PlanMode::Complete) || is_plan_complete(plan) {
        return text.to_string();
    }
    view::HeartbeatView::from_plan(plan).prepend(text, digest_lines, learning_ids)
}

pub(super) fn prepend_heartbeat_snapshot(
    plan_id: &str,
    heartbeat: &OrchestratorHeartbeat,
    loops: &[LoopSpec],
    text: &str,
) -> String {
    prepend_heartbeat(plan_id, heartbeat, loops, text, None)
}

/// A plan is complete only when every recorded task is terminal.
pub fn is_plan_complete(plan: &Plan) -> bool {
    !plan.tasks.is_empty() && plan.tasks.iter().all(|task| task.status.is_terminal())
}

pub(super) fn heartbeat_resolution_priority(mode: &PlanMode, open_work: bool) -> u8 {
    match (mode, open_work) {
        (PlanMode::Execution, true) => 5,
        (PlanMode::Execution, false) => 4,
        (PlanMode::Approved, true) => 3,
        (PlanMode::Planning, true) => 2,
        (PlanMode::Approved | PlanMode::Planning, false) => 1,
        (PlanMode::Paused | PlanMode::Complete, _) => 0,
    }
}

impl PlanStore {
    pub fn resolve_active_plan_for_orchestrator(
        &self,
        plan_id: Option<&str>,
        tab_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<String, PlanError> {
        self.database
            .resolve_heartbeat_plan(plan_id, tab_id, session_id)
    }

    pub fn add_heartbeat_learning(
        &self,
        plan_id: Option<&str>,
        tab_id: Option<&str>,
        session_id: Option<&str>,
        text: &str,
        tag: Option<&str>,
        now_ms: u64,
    ) -> Result<HeartbeatLearnResult, PlanError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(PlanError::Io("learning text is required".into()));
        }
        let plan_id = self.resolve_active_plan_for_orchestrator(plan_id, tab_id, session_id)?;
        let normalized = normalized_learning(text);
        let mut out = None;
        self.with_runtime_rows::<HeartbeatRows, _>(&plan_id, now_ms, |plan| {
            if let Some(existing) = plan
                .heartbeat
                .learnings
                .iter()
                .find(|learning| normalized_learning(&learning.text) == normalized)
                .cloned()
            {
                out = Some(HeartbeatLearnResult {
                    plan_id: plan_id.clone(),
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
                plan_id: plan_id.clone(),
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
        tab_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<HeartbeatShowResult, PlanError> {
        let plan_id = self.resolve_active_plan_for_orchestrator(plan_id, tab_id, session_id)?;
        self.database.heartbeat_show_projection(&plan_id, now_ms())
    }

    pub fn prepend_heartbeat_with_selected_learnings(
        &self,
        plan_id: &str,
        text: &str,
        digests: &[String],
        learning_ids: Option<&[String]>,
    ) -> Result<String, PlanError> {
        let read = match self
            .database
            .heartbeat_projection(plan_id, now_ms(), learning_ids)
        {
            Ok(read) => read,
            Err(PlanError::NotFound) => return Ok(text.into()),
            Err(error) => return Err(error),
        };
        Ok(read.prepend(text, digests))
    }

    pub fn prepend_heartbeat_for_plan(
        &self,
        plan_id: &str,
        text: &str,
    ) -> Result<String, PlanError> {
        self.prepend_heartbeat_with_selected_learnings(plan_id, text, &[], None)
    }

    pub fn prepend_heartbeat_for_plan_with_digests(
        &self,
        plan_id: &str,
        text: &str,
        digest_lines: &[String],
    ) -> Result<String, PlanError> {
        self.prepend_heartbeat_with_selected_learnings(plan_id, text, digest_lines, None)
    }

    pub fn prepend_heartbeat_for_orchestrator_tab(
        &self,
        tab_id: &str,
        text: &str,
    ) -> Result<String, PlanError> {
        let plan_id = match self.resolve_active_plan_for_orchestrator(None, Some(tab_id), None) {
            Ok(id) => id,
            Err(PlanError::NotFound) => return Ok(text.to_string()),
            Err(error) => return Err(error),
        };
        self.prepend_heartbeat_for_plan(&plan_id, text)
    }
}

#[cfg(test)]
mod compact_tests;

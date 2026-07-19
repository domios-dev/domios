use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use super::readback::pending_after_readback;
use super::{PlanError, PlanStore};
use crate::plan::{
    HeartbeatLearning, LoopSpec, LoopStatus, OrchestratorHeartbeat, Plan, PlanMode, PlanStatus,
    PlannerRef, QaState, ReviewStage, TaskStatus,
};

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

fn clean_list(items: Vec<String>) -> Vec<String> {
    items
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn render_list(items: &[String], out: &mut String) {
    for item in items.iter().filter(|item| !item.trim().is_empty()) {
        out.push_str("- ");
        out.push_str(item.trim());
        out.push('\n');
    }
}

fn render_running_loops(loops: &[LoopSpec], out: &mut String) {
    let running = loops
        .iter()
        .filter(|loop_spec| loop_spec.status == LoopStatus::Running)
        .collect::<Vec<_>>();
    if running.is_empty() {
        return;
    }
    out.push_str("Running loops:\n");
    for loop_spec in running {
        out.push_str("- ");
        out.push_str(&loop_spec.id);
        out.push_str(": ");
        out.push_str(loop_spec.target.trim());
        out.push_str(" (");
        out.push_str(&format!("{}/{}", loop_spec.count, loop_spec.budget));
        out.push(')');
        out.push('\n');
    }
}

pub(super) fn competitive_attempt_summary_line(
    task_id: &str,
    task_name: &str,
    live_attempts: usize,
) -> String {
    format!("- {task_id} ({task_name}): {live_attempts} live attempts")
}

fn render_competitive_attempts(plan: &Plan, out: &mut String) {
    let summaries = plan
        .tasks
        .iter()
        .filter_map(|task| {
            let live_attempts = super::task_attempts::live_attempt_count(task);
            (live_attempts > 1)
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

fn render_qa_queue(plan: &Plan, out: &mut String) {
    let queued = plan
        .tasks
        .iter()
        .filter(|task| {
            task.status == TaskStatus::InReview
                && task.review_stage == ReviewStage::Qa
                && task.qa_state == Some(QaState::Queued)
        })
        .count();
    let running = plan.tasks.iter().find(|task| {
        task.status == TaskStatus::InReview
            && task.review_stage == ReviewStage::Qa
            && task.qa_state == Some(QaState::Running)
    });
    if queued == 0 && running.is_none() {
        return;
    }
    out.push_str("QA queue:\n");
    out.push_str(&format!("- queued: {queued}\n"));
    if let Some(task) = running {
        let age_ms = now_ms().saturating_sub(task.status_changed_at);
        out.push_str(&format!(
            "- running: {} ({}) age_ms={age_ms}\n",
            task.id, task.name
        ));
    }
}

fn render_review_gate(plan: &Plan, out: &mut String) {
    let status = super::review_gate::review_gate_status(plan);
    if !status.blocked {
        return;
    }
    out.push_str("Review backpressure:\n");
    out.push_str(&format!(
        "- {}\n",
        status.reason.as_deref().unwrap_or("review_wip_exceeded")
    ));
}

fn render_regression_gates(plan: &Plan, out: &mut String) {
    let gates = super::milestone_gates::pending_regression_gates(plan);
    if gates.is_empty() {
        return;
    }
    out.push_str(super::prompts_plan::MILESTONE_GATE_HEARTBEAT_TITLE);
    out.push('\n');
    for gate in gates {
        out.push_str("- ");
        out.push_str(&super::prompts_plan::milestone_gate_suggestion(
            &gate.key,
            &gate.label,
        ));
        out.push('\n');
    }
}

static REFLECT_EVERY_GATES: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

pub fn init_reflect_every(every_gates: u32) {
    REFLECT_EVERY_GATES.get_or_init(|| every_gates);
}

fn reflect_every_gates() -> u32 {
    *REFLECT_EVERY_GATES.get().unwrap_or(&5)
}

fn render_open_gates(plan: &Plan, now_ms: u64, out: &mut String) {
    let open: Vec<_> = plan.gates.iter().filter(|g| g.is_open()).collect();
    if open.is_empty() {
        return;
    }
    out.push_str("Decision gates:\n");
    for gate in open {
        let age_ms = now_ms.saturating_sub(gate.asked_at);
        let age_h = age_ms / 3_600_000;
        let age_min = (age_ms % 3_600_000) / 60_000;
        let age = if age_h > 0 {
            format!("{age_h}h{age_min}m")
        } else {
            format!("{age_min}m")
        };
        let blocking = gate.blocking_task_ids.len();
        let routed = gate.routed_to_pane_id.as_deref().unwrap_or("unrouted");
        out.push_str(&format!(
            "- {} (age:{age} blocking:{blocking} routed:{routed}): {}\n",
            gate.id,
            gate.question.chars().take(100).collect::<String>()
        ));
    }
}

fn role_identity_line(plan: &Plan, pane_id: Option<&str>) -> String {
    let owner = plan.owner.pane_id.as_deref();
    let planner = match &plan.planner {
        Some(PlannerRef::Pane { pane_id, .. }) => Some(pane_id.as_str()),
        _ => None,
    };
    let role = match pane_id {
        Some(pane) if owner == Some(pane) => "Orchestrator",
        Some(pane) if planner == Some(pane) => "Planner",
        _ => "Orchestrator",
    };
    let peer_role = if role == "Planner" {
        "Orchestrator"
    } else {
        "Planner"
    };
    let peer = if role == "Planner" { owner } else { planner };
    let peer_text = peer
        .map(|pane| format!("; peer tab {peer_role} is pane {pane}"))
        .unwrap_or_else(|| format!("; peer tab {peer_role} is unbound"));
    codeterm_types::agent::role_identity::render(
        codeterm_types::agent::role_identity::RoleIdentity::Heartbeat {
            role,
            plan_id: &plan.id,
            peer_text: &peer_text,
        },
    )
}

fn render_heartbeat(
    plan: &Plan,
    heartbeat: &OrchestratorHeartbeat,
    loops: &[LoopSpec],
    extra_digest_lines: &[String],
) -> String {
    let plan_id = &plan.id;
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
    let has_running_loops = loops
        .iter()
        .any(|loop_spec| loop_spec.status == LoopStatus::Running);
    let has_competitive_attempts = plan
        .tasks
        .iter()
        .any(|task| super::task_attempts::live_attempt_count(task) > 1);
    let has_qa = plan.tasks.iter().any(|task| {
        task.status == TaskStatus::InReview
            && task.review_stage == ReviewStage::Qa
            && task.qa_state.is_some()
    });
    let has_open_gates = plan.gates.iter().any(|g| g.is_open());
    let has_regression_gates = !super::milestone_gates::pending_regression_gates(plan).is_empty();
    let has_review_gate = super::review_gate::review_gate_status(plan).blocked;
    let has_reflection =
        !super::reflection::pending_reflections(plan, reflect_every_gates()).is_empty();
    let has_digests = !extra_digest_lines.is_empty();
    if !has_focus
        && !has_lanes
        && !has_rules
        && !has_learnings
        && !has_running_loops
        && !has_competitive_attempts
        && !has_qa
        && !has_open_gates
        && !has_regression_gates
        && !has_review_gate
        && !has_reflection
        && !has_digests
    {
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
    render_qa_queue(plan, &mut out);
    render_review_gate(plan, &mut out);
    render_regression_gates(plan, &mut out);
    super::reflection::render_reflection(plan, reflect_every_gates(), &mut out);
    render_competitive_attempts(plan, &mut out);
    render_running_loops(loops, &mut out);
    render_open_gates(plan, now_ms(), &mut out);
    if has_digests {
        out.push_str("Digests:\n");
        for line in extra_digest_lines
            .iter()
            .filter(|line| !line.trim().is_empty())
        {
            out.push_str("- ");
            out.push_str(line.trim());
            out.push('\n');
        }
    }
    let skill_slugs = super::skill_registry::list_plan_skill_slugs(plan);
    out.push_str(&super::skill_registry::render_plan_skills_registry_line(
        skill_slugs.len(),
        &skill_slugs,
    ));
    out.push('\n');
    out.push_str("</codeterm-orchestrator-heartbeat>");
    out
}

pub(super) fn reconciliation_prompt(plan_id: &str, pane_id: Option<&str>) -> String {
    let reports = pane_id.map_or_else(
        || "codeterm agent reports".to_string(),
        |pane| format!("codeterm agent reports --tab {pane}"),
    );
    let status = pane_id.map_or_else(
        || format!("codeterm orchestrator status --plan {plan_id} --json"),
        |pane| format!("codeterm orchestrator status --plan {plan_id} --tab {pane} --json"),
    );
    format!(
        "STATE-RECONCILIATION:\n\
         - This pane owns an execution-mode plan with unfinished work; do not pause while open tasks remain.\n\
         - If you forgot how to orchestrate, read `codeterm docs ORCHESTRATION` first.\n\
         - First run `{reports}` and process queued reports before inspecting panes.\n\
         - Pull the full plan-owned snapshot with `{status}`.\n\
         - Reconcile live panes against the plan with `codeterm plan reconcile {plan_id}`.\n\
         - {} \n\
         - Parallelize independent writer lanes, but serialize broad/heavy gates in one warm integration checkout so worker worktrees do not all rebuild/test at once.\n\
         - Close verified tasks with `codeterm plan task close {plan_id} <task_id>`.\n\
         - Bounce incomplete work with `codeterm send \"<next concrete instruction>\" --tab <worker_tab_id>` and record `codeterm plan record {plan_id} --task <task_id> --step decision:<text>`.\n\
         - Clean finished lane worktrees/branches/dev daemons, then record `codeterm plan task update {plan_id} <task_id> --cleanup-status done` or why cleanup is deferred.\n\
         - Keep the plan-owned heartbeat current with `codeterm orchestrator heartbeat set --plan {plan_id} --focus \"<now>\" --lane \"<active-lane>\"`.",
        super::prompts_plan::QA_PIPELINE_DISCIPLINE.trim()
    )
}

fn prepend_heartbeat(
    plan_id: &str,
    heartbeat: &OrchestratorHeartbeat,
    loops: &[LoopSpec],
    text: &str,
    pane_id: Option<&str>,
) -> String {
    let plan = Plan {
        id: plan_id.to_string(),
        heartbeat: heartbeat.clone(),
        loops: loops.to_vec(),
        ..Plan::new(String::new(), String::new(), Default::default())
    };
    let block = render_heartbeat(&plan, heartbeat, loops, &[]);
    let identity = role_identity_line(&plan, pane_id);
    let prompt = reconciliation_prompt(plan_id, pane_id);
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

pub(super) fn prepend_plan_heartbeat(plan: &Plan, text: &str) -> String {
    prepend_plan_heartbeat_with_digests(plan, text, &[])
}

pub(super) fn prepend_plan_heartbeat_with_digests(
    plan: &Plan,
    text: &str,
    digest_lines: &[String],
) -> String {
    if matches!(plan.mode, PlanMode::Paused | PlanMode::Complete) || is_plan_complete(plan) {
        return text.to_string();
    }
    let block = render_heartbeat(plan, &plan.heartbeat, &plan.loops, digest_lines);
    let identity = role_identity_line(plan, plan.owner.pane_id.as_deref());
    let prompt = reconciliation_prompt(&plan.id, plan.owner.pane_id.as_deref());
    let reconciliation = format!(
        "<codeterm-state-reconciliation plan=\"{}\">\n{prompt}\n</codeterm-state-reconciliation>",
        plan.id
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

pub(super) fn prepend_heartbeat_snapshot(
    plan_id: &str,
    heartbeat: &OrchestratorHeartbeat,
    loops: &[LoopSpec],
    text: &str,
) -> String {
    prepend_heartbeat(plan_id, heartbeat, loops, text, None)
}

fn has_open_work(plan: &Plan) -> bool {
    plan.tasks
        .iter()
        .any(|task| task.status.counts_as_open_work())
}

/// A plan is complete only when every recorded task is terminal.
pub fn is_plan_complete(plan: &Plan) -> bool {
    !plan.tasks.is_empty() && plan.tasks.iter().all(|task| task.status.is_terminal())
}

fn owner_matches(plan: &Plan, pane_id: Option<&str>, session_id: Option<&str>) -> bool {
    let pane_matches = pane_id
        .filter(|pane| !pane.trim().is_empty())
        .is_some_and(|pane| plan.owner.pane_id.as_deref() == Some(pane));
    let session_matches = session_id
        .filter(|session| !session.trim().is_empty())
        .is_some_and(|session| plan.owner.session_id.as_deref() == Some(session));
    pane_matches || session_matches
}

fn heartbeat_resolution_rank(plan: &Plan) -> (u8, u64) {
    let rank = match (&plan.mode, has_open_work(plan)) {
        (PlanMode::Execution, true) => 5,
        (PlanMode::Execution, false) => 4,
        (PlanMode::Approved, true) => 3,
        (PlanMode::Planning, true) => 2,
        (PlanMode::Approved | PlanMode::Planning, false) => 1,
        (PlanMode::Paused | PlanMode::Complete, _) => 0,
    };
    (rank, plan.updated_at)
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
        let mut best_match = None;
        for entry in self.list_index() {
            let Some(plan) = self.get(&entry.plan_id) else {
                continue;
            };
            if plan.status != PlanStatus::Active {
                continue;
            }
            if !owner_matches(&plan, pane_id, session_id) {
                continue;
            }
            let rank = heartbeat_resolution_rank(&plan);
            if rank.0 == 0 {
                continue;
            }
            if best_match
                .as_ref()
                .is_none_or(|(_, best_rank)| rank > *best_rank)
            {
                best_match = Some((plan.id, rank));
            }
        }
        best_match
            .map(|(plan_id, _)| plan_id)
            .ok_or(PlanError::NotFound)
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

    #[allow(clippy::too_many_arguments)]
    pub fn set_heartbeat(
        &self,
        plan_id: Option<&str>,
        pane_id: Option<&str>,
        session_id: Option<&str>,
        focus: Option<&str>,
        lanes: Option<Vec<String>>,
        rules: Option<Vec<String>>,
        clear_lanes: bool,
        clear_rules: bool,
        now_ms: u64,
    ) -> Result<HeartbeatShowResult, PlanError> {
        let plan_id = self.resolve_active_plan_for_orchestrator(plan_id, pane_id, session_id)?;
        let write = self.with_plan(&plan_id, now_ms, |plan| {
            let writes_runtime_state = focus.is_some()
                || lanes
                    .as_ref()
                    .is_some_and(|items| !clean_list(items.clone()).is_empty())
                || rules
                    .as_ref()
                    .is_some_and(|items| !clean_list(items.clone()).is_empty());
            if plan.mode == PlanMode::Planning && writes_runtime_state {
                return Err(PlanError::HeartbeatRequiresExecution);
            }
            if let Some(focus) = focus {
                let focus = focus.trim();
                if focus.to_ascii_lowercase().starts_with("waiting") {
                    let names_open_gate = plan
                        .gates
                        .iter()
                        .any(|g| g.is_open() && focus.contains(&g.id));
                    if !names_open_gate {
                        return Err(PlanError::WaitingFocusRequiresOpenGate);
                    }
                }
                plan.heartbeat.current_focus = (!focus.is_empty()).then(|| focus.to_string());
            }
            if clear_lanes {
                plan.heartbeat.active_lanes.clear();
            } else if let Some(lanes) = lanes {
                plan.heartbeat.active_lanes = clean_list(lanes);
            }
            if clear_rules {
                plan.heartbeat.rules.clear();
            } else if let Some(rules) = rules {
                plan.heartbeat.rules = clean_list(rules);
            }
            Ok(())
        });
        pending_after_readback(write, self.show_heartbeat(Some(&plan_id), None, None))
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
            reinjection: render_heartbeat(&plan, &plan.heartbeat, &plan.loops, &[]),
            heartbeat: plan.heartbeat,
        })
    }

    pub fn prepend_heartbeat_for_plan(&self, plan_id: &str, text: &str) -> String {
        let Some(plan) = self.get(plan_id) else {
            return text.to_string();
        };
        prepend_plan_heartbeat(&plan, text)
    }

    pub fn prepend_heartbeat_for_plan_with_digests(
        &self,
        plan_id: &str,
        text: &str,
        digest_lines: &[String],
    ) -> String {
        let Some(plan) = self.get(plan_id) else {
            return text.to_string();
        };
        prepend_plan_heartbeat_with_digests(&plan, text, digest_lines)
    }

    pub fn prepend_heartbeat_for_orchestrator_pane(&self, pane_id: &str, text: &str) -> String {
        let Ok(plan_id) = self.resolve_active_plan_for_orchestrator(None, Some(pane_id), None)
        else {
            return text.to_string();
        };
        self.prepend_heartbeat_for_plan(&plan_id, text)
    }
}

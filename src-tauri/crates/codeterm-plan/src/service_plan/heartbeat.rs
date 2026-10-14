use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{PlanError, PlanStore};
use crate::plan::{
    HeartbeatLearning, LoopSpec, OrchestratorHeartbeat, Plan, PlanMode, PlanStatus, PlannerRef,
    TaskStatus,
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
    let gates = super::milestone_gates::pending_regression_gates(plan);
    if gates.is_empty() {
        return;
    }
    let groups = gates
        .iter()
        .map(|gate| gate.key.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("Record group regression: {groups}\n"));
}

fn render_group_retrospectives(plan: &Plan, out: &mut String) {
    let pending = super::milestone_gates::pending_group_retrospectives(plan);
    if pending.is_empty() {
        return;
    }
    let groups = pending
        .iter()
        .map(|gate| gate.key.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!("Close groups: {groups}\n"));
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
        let routed = gate.routed_to_tab_id.as_deref().unwrap_or("unrouted");
        out.push_str(&format!(
            "- {} (age:{age} blocking:{blocking} routed:{routed}): {}\n",
            gate.id,
            gate.question.chars().take(100).collect::<String>()
        ));
    }
}

fn role_identity_line(plan: &Plan, tab_id: Option<&str>) -> String {
    let owner = plan.owner.tab_id.as_deref();
    let planner = match &plan.planner {
        Some(PlannerRef::Tab { tab_id, .. }) => Some(tab_id.as_str()),
        _ => None,
    };
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
            plan_id: &plan.id,
            peer_text: &peer_text,
        },
    )
}

pub const HEARTBEAT_BLOCK_OPEN: &str = "<codeterm-orchestrator-heartbeat";
pub const HEARTBEAT_MORE: &str =
    "More: codeterm docs ORCHESTRATION; all learnings: codeterm orchestrator heartbeat show";
pub const MAX_SITUATIONAL_LEARNINGS: usize = 3;

fn is_owner_rule(learning: &HeartbeatLearning) -> bool {
    learning.tag.as_deref() == Some("owner")
}

fn render_learning(learning: &HeartbeatLearning, out: &mut String) {
    out.push_str("- ");
    out.push_str(
        &learning
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
    );
    out.push('\n');
}

const SITUATIONAL_LEARNING_CHARS: usize = 160;

fn render_situational_learning(learning: &HeartbeatLearning, out: &mut String) {
    let text = learning
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let excerpt = if text.chars().count() > SITUATIONAL_LEARNING_CHARS {
        let prefix = text
            .chars()
            .take(SITUATIONAL_LEARNING_CHARS)
            .collect::<String>();
        let boundary = prefix.rfind(' ').unwrap_or(prefix.len());
        format!("{}…", &prefix[..boundary])
    } else {
        text
    };
    out.push_str(&format!("- {}: {excerpt}\n", learning.id));
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
        let verb = match action.kind {
            NextActionKind::Gate => "gate",
            NextActionKind::ClaimQa => "claim QA",
            NextActionKind::Dispatch => "dispatch",
            NextActionKind::Advance => "unblock",
            NextActionKind::WaitingOnDependency => "repair missing dependency",
            NextActionKind::ConsiderStop => {
                out.push_str(&format!("- {}\n", action.message));
                continue;
            }
            _ => continue,
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
            out.push_str(&format!("- {}: recover handoff\n", task.id));
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
                "- skill {}: codeterm plan skill show --plan {} {}\n",
                skill.slug, plan.id, skill.slug
            ));
        }
    }
}

fn render_heartbeat(
    plan: &Plan,
    heartbeat: &OrchestratorHeartbeat,
    _loops: &[LoopSpec],
    extra_digest_lines: &[String],
) -> String {
    let snapshot = reminder_snapshot(plan);
    let pending = snapshot
        .open_lanes
        .iter()
        .filter(|lane| lane.presentation == super::state::TaskPresentation::ActionablePending)
        .count();
    let mut out = format!("{HEARTBEAT_BLOCK_OPEN} plan=\"{}\">\nState: in_review={} in_progress={} pending_actionable={pending} blocked={}\n",
        plan.id, snapshot.task_counts.in_review, snapshot.task_counts.in_progress, snapshot.task_counts.blocked);
    let actions = render_actions(plan, &snapshot, &mut out);
    for learning in heartbeat
        .learnings
        .iter()
        .filter(|learning| is_owner_rule(learning))
    {
        render_learning(learning, &mut out);
    }
    for learning in heartbeat
        .learnings
        .iter()
        .rev()
        .filter(|learning| !is_owner_rule(learning) && !learning.text.trim().is_empty())
        .take(MAX_SITUATIONAL_LEARNINGS)
    {
        render_situational_learning(learning, &mut out);
    }
    render_regression_gates(plan, &mut out);
    render_group_retrospectives(plan, &mut out);
    render_competitive_attempts(plan, &mut out);
    render_open_gates(plan, now_ms(), &mut out);
    for line in extra_digest_lines
        .iter()
        .filter(|line| !line.trim().is_empty())
    {
        out.push_str("- ");
        out.push_str(line.trim());
        out.push('\n');
    }
    render_actionable_skills(plan, &actions, &mut out);
    out.push_str(HEARTBEAT_MORE);
    out.push_str("\n</codeterm-orchestrator-heartbeat>");
    out
}

pub(super) fn reconciliation_prompt(plan_id: &str, tab_id: Option<&str>) -> String {
    let status = tab_id.map_or_else(
        || format!("codeterm orchestrator status --plan {plan_id}"),
        |tab| format!("codeterm orchestrator status --plan {plan_id} --tab {tab}"),
    );
    format!(
        "STATE-RECONCILIATION:\n\
         - This tab owns an execution-mode plan with unfinished work; act on actionable tasks, and let dependency-waiting or machine-paused tasks wait.\n\
         - If you forgot how to orchestrate, read `codeterm docs ORCHESTRATION` first.\n\
         - Worker reports are pushed into this tab automatically; judge each incoming report without polling an inbox.\n\
         - After dispatching a worker, end your turn immediately. Never sleep, wait, poll, or call `tab inspect`, `tab get-text`, `orchestrator status`, or inbox checks just to keep the turn open or watch progress; resume only when a worker report, user message, or system event is delivered automatically. Periodic-progress duties do not apply while waiting on a pushed report.\n\
         - Read the compact plan-owned status with `{status}`.\n\
         - Use `codeterm orchestrator inspect --plan {plan_id} --json` only when a specific discrepancy requires full report bodies or raw lane state.\n\
         - Reconcile live tabs against the plan with `codeterm plan reconcile {plan_id}`.\n\
         - {} \n\
         - Parallelize independent writer lanes, but serialize broad/heavy gates in one warm integration checkout so worker worktrees do not all rebuild/test at once.\n\
         - Close verified tasks with `codeterm plan task close {plan_id} <task_id>`.\n\
         - Bounce incomplete work with `codeterm send \"<next concrete instruction>\" --tab <worker_tab_id>` and record `codeterm plan record {plan_id} --task <task_id> --step decision:<text>`.\n\
         - Clean finished lane worktrees/branches/dev daemons, then record `codeterm plan task update {plan_id} <task_id> --cleanup-status done` or why cleanup is deferred.\n\
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
    let plan = Plan {
        id: plan_id.to_string(),
        heartbeat: heartbeat.clone(),
        loops: loops.to_vec(),
        ..Plan::new(String::new(), String::new(), Default::default())
    };
    let block = render_heartbeat(&plan, heartbeat, loops, &[]);
    let identity = role_identity_line(&plan, tab_id);
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
    if text.trim().is_empty() {
        block
    } else {
        format!("{block}\n\n{text}")
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

fn owner_matches(plan: &Plan, tab_id: Option<&str>, session_id: Option<&str>) -> bool {
    let tab_matches = tab_id
        .filter(|tab| !tab.trim().is_empty())
        .is_some_and(|tab| plan.owner.tab_id.as_deref() == Some(tab));
    let session_matches = session_id
        .filter(|session| !session.trim().is_empty())
        .is_some_and(|session| plan.owner.session_id.as_deref() == Some(session));
    tab_matches || session_matches
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
        tab_id: Option<&str>,
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
            if !owner_matches(&plan, tab_id, session_id) {
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
        tab_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<HeartbeatShowResult, PlanError> {
        let plan_id = self.resolve_active_plan_for_orchestrator(plan_id, tab_id, session_id)?;
        let plan = self.get(&plan_id).ok_or(PlanError::NotFound)?;
        let mut heartbeat = plan.heartbeat.clone();
        heartbeat.current_focus = None;
        heartbeat.active_lanes.clear();
        heartbeat.rules.clear();
        Ok(HeartbeatShowResult {
            plan_id: plan.id.clone(),
            reinjection: render_heartbeat(&plan, &heartbeat, &plan.loops, &[]),
            blocked_or_gated_count: super::state::outstanding_blocked_count(&plan),
            heartbeat,
        })
    }

    pub fn prepend_heartbeat_with_selected_learnings(
        &self,
        plan_id: &str,
        text: &str,
        digests: &[String],
        learning_ids: Option<&[String]>,
    ) -> String {
        let Some(mut plan) = self.get(plan_id) else {
            return text.to_string();
        };
        if let Some(ids) = learning_ids {
            let allowed: Vec<_> = ids
                .iter()
                .filter(|id| {
                    plan.heartbeat
                        .learnings
                        .iter()
                        .any(|learning| &learning.id == *id && !is_owner_rule(learning))
                })
                .take(MAX_SITUATIONAL_LEARNINGS)
                .collect();
            plan.heartbeat
                .learnings
                .retain(|learning| is_owner_rule(learning) || allowed.contains(&&learning.id));
        }
        prepend_plan_heartbeat_with_digests(&plan, text, digests)
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

    pub fn prepend_heartbeat_for_orchestrator_tab(&self, tab_id: &str, text: &str) -> String {
        let Ok(plan_id) = self.resolve_active_plan_for_orchestrator(None, Some(tab_id), None)
        else {
            return text.to_string();
        };
        self.prepend_heartbeat_for_plan(&plan_id, text)
    }
}

#[cfg(test)]
mod compact_tests;

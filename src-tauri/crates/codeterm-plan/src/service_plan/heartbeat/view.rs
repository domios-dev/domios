use super::*;
use crate::service_plan::state::TaskCounts;

pub(in crate::service_plan) struct HeartbeatView {
    pub mode: PlanMode,
    pub complete: bool,
    pub heartbeat: OrchestratorHeartbeat,
    pub parts: HeartbeatParts,
}

#[derive(Debug)]
pub(in crate::service_plan) struct HeartbeatParts {
    pub plan_id: String,
    pub counts: TaskCounts,
    pub pending_actionable: usize,
    pub blocked_or_gated_count: usize,
    pub actions: String,
    pub regression: String,
    pub retrospectives: String,
    pub competition: String,
    pub gates: String,
    pub skills: String,
    pub control_line: String,
}

pub(in crate::service_plan) enum HeartbeatRead {
    Suppressed,
    Ready {
        parts: HeartbeatParts,
        learnings: String,
    },
}

impl HeartbeatRead {
    pub(in crate::service_plan) fn prepend(&self, text: &str, digests: &[String]) -> String {
        let Self::Ready { parts, learnings } = self else {
            return text.into();
        };
        let mut lines = digests.to_vec();
        lines.push(parts.control_line.clone());
        let block = parts.render_block(learnings, &lines);
        if text.trim().is_empty() {
            block
        } else {
            format!("{block}\n\n{text}")
        }
    }
}

impl HeartbeatView {
    pub(super) fn from_plan(plan: &Plan) -> Self {
        Self::from_plan_at(plan, now_ms())
    }

    pub(in crate::service_plan) fn from_plan_at(plan: &Plan, now: u64) -> Self {
        let snapshot = reminder_snapshot(plan);
        let mut actions = String::new();
        let ids = render_actions(plan, &snapshot, &mut actions);
        let mut regression = String::new();
        render_regression_gates(plan, &mut regression);
        let mut retrospectives = String::new();
        render_group_retrospectives(plan, &mut retrospectives);
        let mut competition = String::new();
        render_competitive_attempts(plan, &mut competition);
        let mut gates = String::new();
        render_open_gates(plan, now, &mut gates);
        let mut skills = String::new();
        render_actionable_skills(plan, &ids, &mut skills);
        Self {
            mode: plan.mode.clone(),
            complete: is_plan_complete(plan),
            heartbeat: plan.heartbeat.clone(),
            parts: HeartbeatParts {
                plan_id: plan.id.clone(),
                counts: snapshot.task_counts,
                pending_actionable: snapshot
                    .open_lanes
                    .iter()
                    .filter(|lane| {
                        lane.presentation
                            == super::super::state::TaskPresentation::ActionablePending
                    })
                    .count(),
                blocked_or_gated_count: super::super::state::outstanding_blocked_count(plan),
                actions,
                regression,
                retrospectives,
                competition,
                gates,
                skills,
                control_line: control_line(
                    &plan.control_loop.mode,
                    plan.control_loop.next_at,
                    plan.control_loop
                        .fallback_reason
                        .map(|reason| reason.as_str()),
                ),
            },
        }
    }

    pub(in crate::service_plan) fn render_block(
        &self,
        heartbeat: &OrchestratorHeartbeat,
        extra: &[String],
    ) -> String {
        self.parts.render_block(&learning_lines(heartbeat), extra)
    }

    pub(in crate::service_plan) fn prepend(
        &self,
        text: &str,
        digests: &[String],
        learning_ids: Option<&[String]>,
    ) -> String {
        if matches!(self.mode, PlanMode::Paused | PlanMode::Complete) || self.complete {
            return text.to_string();
        }
        let mut heartbeat = self.heartbeat.clone();
        if let Some(ids) = learning_ids {
            let allowed: Vec<_> = ids
                .iter()
                .filter(|id| {
                    heartbeat
                        .learnings
                        .iter()
                        .any(|learning| &learning.id == *id && !is_owner_rule(learning))
                })
                .take(MAX_SITUATIONAL_LEARNINGS)
                .collect();
            heartbeat
                .learnings
                .retain(|learning| is_owner_rule(learning) || allowed.contains(&&learning.id));
        }
        let mut lines = digests.to_vec();
        lines.push(self.parts.control_line.clone());
        let block = self.render_block(&heartbeat, &lines);
        if text.trim().is_empty() {
            block
        } else {
            format!("{block}\n\n{text}")
        }
    }
}

impl HeartbeatParts {
    pub(in crate::service_plan) fn from_snapshot(plan_id: &str, loops: &[LoopSpec]) -> Self {
        use crate::service_plan::next_actions::{
            compute_next_actions_from, ActionLoop, NextActionInput, NextActionKind,
        };
        let actions = compute_next_actions_from(&NextActionInput {
            plan_id,
            execution_tasks_unlocked: false,
            planning_approval_blockers: &[],
            planner_resolution: &crate::plan::peer::PeerResolution::Unbound,
            open_lanes: Vec::new(),
            blocked_lanes: Vec::new(),
            drift_total: 0,
            has_live_agents: false,
            loops: loops
                .iter()
                .map(|item| ActionLoop {
                    id: &item.id,
                    status: item.status.clone(),
                    count: item.count,
                    budget: item.budget,
                })
                .collect(),
            pending_regression_gates: &[],
            pending_group_retrospectives: &[],
        })
        .into_iter()
        .filter(|action| action.kind == NextActionKind::ConsiderStop)
        .map(|action| format!("- {}\n", action.message))
        .collect::<String>();
        Self {
            plan_id: plan_id.into(),
            counts: TaskCounts::default(),
            pending_actionable: 0,
            blocked_or_gated_count: 0,
            actions,
            regression: String::new(),
            retrospectives: String::new(),
            competition: String::new(),
            gates: String::new(),
            skills: String::new(),
            control_line: String::new(),
        }
    }

    pub(in crate::service_plan) fn render_block(
        &self,
        learnings: &str,
        extra: &[String],
    ) -> String {
        let mut out = state_header(&self.plan_id, &self.counts, self.pending_actionable);
        out.push_str(&self.actions);
        out.push_str(learnings);
        out.push_str(&self.regression);
        out.push_str(&self.retrospectives);
        out.push_str(&self.competition);
        out.push_str(&self.gates);
        for line in extra.iter().filter(|line| !line.trim().is_empty()) {
            out.push_str("- ");
            out.push_str(line.trim());
            out.push('\n');
        }
        out.push_str(&self.skills);
        out.push_str(HEARTBEAT_MORE);
        out.push_str("\n</codeterm-orchestrator-heartbeat>");
        out
    }
}

pub(in crate::service_plan) fn control_line(
    mode: &crate::plan::ControlLoopMode,
    next_at: Option<u64>,
    fallback: Option<&str>,
) -> String {
    format!(
        "control loop: {mode:?}, next {next_at:?}{}",
        fallback
            .map(|reason| format!(", fallback: heartbeat ({reason})"))
            .unwrap_or_default()
    )
}

pub(in crate::service_plan) fn state_header(
    id: &str,
    counts: &TaskCounts,
    pending: usize,
) -> String {
    format!("{HEARTBEAT_BLOCK_OPEN} plan=\"{id}\">\nState: in_review={} in_progress={} pending_actionable={pending} blocked={}\n",counts.in_review,counts.in_progress,counts.blocked)
}

pub(in crate::service_plan) fn learning_lines(heartbeat: &OrchestratorHeartbeat) -> String {
    let mut learnings = String::new();
    for learning in heartbeat
        .learnings
        .iter()
        .filter(|learning| is_owner_rule(learning))
    {
        render_learning(learning, &mut learnings);
    }
    for learning in heartbeat
        .learnings
        .iter()
        .rev()
        .filter(|learning| !is_owner_rule(learning) && !learning.text.trim().is_empty())
        .take(MAX_SITUATIONAL_LEARNINGS)
    {
        render_situational_learning(learning, &mut learnings);
    }
    learnings
}

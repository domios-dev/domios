use super::*;
use crate::service_plan::state::TaskCounts;

pub(in crate::service_plan) struct HeartbeatView {
    pub plan_id: String,
    pub mode: PlanMode,
    pub complete: bool,
    pub counts: TaskCounts,
    pub pending_actionable: usize,
    pub heartbeat: OrchestratorHeartbeat,
    pub actions: String,
    pub regression: String,
    pub retrospectives: String,
    pub competition: String,
    pub gates: String,
    pub skills: String,
    pub control_line: String,
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
            plan_id: plan.id.clone(),
            mode: plan.mode.clone(),
            complete: is_plan_complete(plan),
            counts: snapshot.task_counts,
            pending_actionable: snapshot
                .open_lanes
                .iter()
                .filter(|lane| {
                    lane.presentation == super::super::state::TaskPresentation::ActionablePending
                })
                .count(),
            heartbeat: plan.heartbeat.clone(),
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
        }
    }

    pub(in crate::service_plan) fn render_block(
        &self,
        heartbeat: &OrchestratorHeartbeat,
        extra: &[String],
    ) -> String {
        let mut out = state_header(&self.plan_id, &self.counts, self.pending_actionable);
        out.push_str(&self.actions);
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
        lines.push(self.control_line.clone());
        let block = self.render_block(&heartbeat, &lines);
        if text.trim().is_empty() {
            block
        } else {
            format!("{block}\n\n{text}")
        }
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

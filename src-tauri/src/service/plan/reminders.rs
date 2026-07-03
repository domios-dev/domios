use crate::multiplexer::types::AgentRole;
use parking_lot::Mutex;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudgeKind {
    Worker,
    Orchestrator,
    Escalated,
}

#[derive(Debug, Clone)]
pub struct NudgeConfig {
    pub plan_reminders: bool,
    pub nudge_after_turns: u32,
    pub role: Option<AgentRole>,
}

pub fn should_nudge(turns: u32, last_nudge: u32, cfg: &NudgeConfig) -> Option<NudgeKind> {
    if !cfg.plan_reminders || cfg.nudge_after_turns == 0 || turns < cfg.nudge_after_turns {
        return None;
    }
    let interval = next_interval(cfg.nudge_after_turns, last_nudge);
    if last_nudge > 0 && turns < last_nudge.saturating_add(interval) {
        return None;
    }
    if last_nudge >= cfg.nudge_after_turns.saturating_mul(3) {
        return Some(NudgeKind::Escalated);
    }
    match cfg.role {
        Some(AgentRole::Worker) => Some(NudgeKind::Worker),
        _ => Some(NudgeKind::Orchestrator),
    }
}

pub fn nudge_text(kind: NudgeKind) -> String {
    match kind {
        NudgeKind::Worker => {
            "Plan reminder: you are assigned to an active task. Run `codeterm agent report --plan <plan_id> --task <task_id> \"status update\"` so your parent can see progress."
                .to_string()
        }
        NudgeKind::Orchestrator => format!(
            "Plan reminder: this pane owns unfinished execution-plan work. This nudge {CONTRACT} Give every open task one of five exits: {EXITS}. Read `codeterm docs ORCHESTRATION` if the orchestration loop is unclear, then process reports, reconcile, review, close, or bounce tasks."
        ),
        NudgeKind::Escalated => format!(
            "ESCALATED plan reminder: open tasks are aging unattended. This nudge {CONTRACT} Give every open task one of five exits: {EXITS}. Workers: an UNREPORTED task leaves your parent blind until you report."
        ),
    }
}

// Verbatim heartbeat/nudge contract — MUST appear in every open-work nudge.
pub const CONTRACT: &str = "repeats until every open task is dispatched, parked with a reason, or sent to replanning; silence is not an option.";

// The five sanctioned exits for an open task.
pub const EXITS: &str = "(1) dispatch it to a worker, (2) park with a reason (`codeterm plan task park <plan> <task> --reason \"<why>\"`), (3) judge an in_review task to done or bounce it, (4) send it to replanning, (5) escalate to a smarter model";

fn next_interval(base: u32, last_nudge: u32) -> u32 {
    if last_nudge == 0 {
        return base;
    }
    (((last_nudge.max(base) as f64) * 1.5).ceil() as u32).clamp(base, base.saturating_mul(8))
}

#[derive(Debug, Default)]
pub struct TurnCounter {
    panes: Mutex<HashMap<String, (u32, u32)>>,
    timed: Mutex<HashMap<String, u64>>,
}

impl TurnCounter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn increment(&self, pane_id: &str) -> (u32, u32) {
        let mut panes = self.panes.lock();
        let entry = panes.entry(pane_id.to_string()).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(1);
        *entry
    }

    pub fn mark_nudged(&self, pane_id: &str, turns: u32) {
        let mut panes = self.panes.lock();
        let entry = panes.entry(pane_id.to_string()).or_insert((turns, 0));
        entry.0 = turns;
        entry.1 = turns;
    }

    pub fn time_nudge_due(&self, pane_id: &str, now_secs: u64, interval_secs: u64) -> bool {
        if interval_secs == 0 {
            return false;
        }
        let mut timed = self.timed.lock();
        let Some(last_nudge) = timed.get(pane_id).copied() else {
            timed.insert(pane_id.to_string(), now_secs);
            return false;
        };
        now_secs.saturating_sub(last_nudge) >= interval_secs
    }

    pub fn mark_time_nudged(&self, pane_id: &str, now_secs: u64) {
        self.timed.lock().insert(pane_id.to_string(), now_secs);
    }

    pub fn reset(&self, pane_id: &str) {
        self.panes.lock().insert(pane_id.to_string(), (0, 0));
        self.timed.lock().remove(pane_id);
    }

    pub fn get(&self, pane_id: &str) -> (u32, u32) {
        self.panes.lock().get(pane_id).copied().unwrap_or((0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(role: AgentRole) -> NudgeConfig {
        NudgeConfig {
            plan_reminders: true,
            nudge_after_turns: 12,
            role: Some(role),
        }
    }

    #[test]
    fn should_nudge_respects_threshold_and_disabled_config() {
        assert_eq!(should_nudge(11, 0, &cfg(AgentRole::Worker)), None);
        let disabled = NudgeConfig {
            plan_reminders: false,
            nudge_after_turns: 12,
            role: Some(AgentRole::Worker),
        };
        assert_eq!(should_nudge(12, 0, &disabled), None);
        assert_eq!(
            should_nudge(12, 0, &cfg(AgentRole::Worker)),
            Some(NudgeKind::Worker)
        );
    }

    #[test]
    fn should_nudge_grows_interval_and_selects_role_copy() {
        assert_eq!(should_nudge(20, 12, &cfg(AgentRole::Worker)), None);
        assert_eq!(
            should_nudge(30, 12, &cfg(AgentRole::Worker)),
            Some(NudgeKind::Worker)
        );
        assert_eq!(
            should_nudge(12, 0, &cfg(AgentRole::Orchestrator)),
            Some(NudgeKind::Orchestrator)
        );
    }

    #[test]
    fn orchestrator_nudge_body_defers_state_reconciliation_to_plan_prepend() {
        let text = nudge_text(NudgeKind::Orchestrator);
        assert!(text.contains("unfinished execution-plan work"));
        assert!(text.contains("codeterm docs ORCHESTRATION"));
        assert!(!text.contains("STATE-RECONCILIATION"));

        let worker_text = nudge_text(NudgeKind::Worker);
        assert!(!worker_text.contains("STATE-RECONCILIATION"));
    }

    #[test]
    fn open_work_nudges_carry_contract_and_five_exits() {
        for kind in [NudgeKind::Orchestrator, NudgeKind::Escalated] {
            let text = nudge_text(kind);
            assert!(text.contains("repeats until every open task is dispatched, parked with a reason, or sent to replanning; silence is not an option."));
            assert!(text.contains("dispatch"));
            assert!(text.contains("park with a reason"));
            assert!(text.contains("judge"));
            assert!(text.contains("replanning"));
            assert!(text.contains("escalate to a smarter model"));
        }
    }

    #[test]
    fn should_nudge_escalates_after_repeated_ignored_nudges() {
        assert_eq!(
            should_nudge(90, 36, &cfg(AgentRole::Worker)),
            Some(NudgeKind::Escalated)
        );
    }

    #[test]
    fn turn_counter_increments_and_resets() {
        let counter = TurnCounter::new();
        assert_eq!(counter.increment("p1"), (1, 0));
        assert_eq!(counter.increment("p1"), (2, 0));
        counter.mark_nudged("p1", 2);
        assert_eq!(counter.get("p1"), (2, 2));
        assert!(!counter.time_nudge_due("p1", 100, 30));
        assert!(counter.time_nudge_due("p1", 130, 30));
        counter.mark_time_nudged("p1", 130);
        assert!(!counter.time_nudge_due("p1", 150, 30));
        counter.reset("p1");
        assert_eq!(counter.get("p1"), (0, 0));
        assert!(!counter.time_nudge_due("p1", 200, 30));
    }
}

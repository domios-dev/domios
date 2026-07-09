use crate::multiplexer::types::AgentRole;
use parking_lot::Mutex;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::plan::{Plan, PlanMode, PlanStatus, TaskStatus};
use crate::service::plan::heartbeat::is_plan_complete;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudgeKind {
    Worker,
    Orchestrator,
    Escalated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimedPlanReminderKind {
    Owner { plan_id: String },
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedPlanReminderTarget {
    pub pane_id: String,
    pub kind: TimedPlanReminderKind,
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

pub const IDLE_POKE_QUESTION_TEMPLATES: [&str; 3] = [
    "You are idle with {pending_tasks} unblocked pending task(s) and {active_lanes} live lane(s). Want to dispatch the next one now?",
    "Nice and quiet. There are {pending_tasks} frontier task(s) and {active_lanes} active lane(s). Want to poke a task and move it forward?",
    "Idle checkpoint: {pending_tasks} unblocked pending task(s) against {active_lanes} live lane(s). Want to send one onward?",
];

const IDLE_POKE_BACKOFF_MAX_STEPS: u32 = 8;
const SECONDS_PER_DAY: u64 = 86_400;

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
struct IdlePokeState {
    last_nudge_at_secs: u64,
    day_bucket: u64,
    sent_today: u32,
}

fn day_bucket(timestamp_secs: u64) -> u64 {
    timestamp_secs / SECONDS_PER_DAY
}

fn idle_backoff_interval(base: u64, sent_today: u32) -> u64 {
    if sent_today == 0 {
        return 0;
    }
    let step = sent_today
        .saturating_sub(1)
        .min(IDLE_POKE_BACKOFF_MAX_STEPS);
    base.saturating_mul(1u64 << step)
}

pub fn idle_poke_text(pending_tasks: usize, active_lanes: usize, phrase_index: u32) -> String {
    let template =
        IDLE_POKE_QUESTION_TEMPLATES[(phrase_index as usize) % IDLE_POKE_QUESTION_TEMPLATES.len()];
    template
        .replace("{pending_tasks}", &pending_tasks.to_string())
        .replace("{active_lanes}", &active_lanes.to_string())
}

pub fn timed_plan_reminder_targets(plans: &[Plan]) -> Vec<TimedPlanReminderTarget> {
    let mut targets = BTreeMap::<String, TimedPlanReminderKind>::new();
    for plan in plans.iter().filter(|plan| {
        plan.status == PlanStatus::Active
            && plan.mode == PlanMode::Execution
            && !is_plan_complete(plan)
            && plan
                .tasks
                .iter()
                .any(|task| task.status.counts_as_open_work())
    }) {
        if let Some(pane_id) = non_empty(plan.owner.pane_id.as_deref()) {
            targets.insert(
                pane_id.to_string(),
                TimedPlanReminderKind::Owner {
                    plan_id: plan.id.clone(),
                },
            );
        }
    }
    for plan in plans.iter().filter(|plan| {
        plan.status == PlanStatus::Active
            && plan.mode == PlanMode::Execution
            && !is_plan_complete(plan)
    }) {
        for task in plan.tasks.iter().filter(|task| {
            matches!(
                task.status,
                TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::Blocked
            )
        }) {
            let Some(pane_id) = non_empty(task.assigned_pane_id.as_deref()) else {
                continue;
            };
            targets
                .entry(pane_id.to_string())
                .or_insert(TimedPlanReminderKind::Worker);
        }
    }
    targets
        .into_iter()
        .map(|(pane_id, kind)| TimedPlanReminderTarget { pane_id, kind })
        .collect()
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    let value = value?.trim();
    (!value.is_empty()).then_some(value)
}

#[derive(Debug, Default)]
pub struct TurnCounter {
    panes: Mutex<HashMap<String, (u32, u32)>>,
    timed: Mutex<HashMap<String, u64>>,
    digest_rules: Mutex<HashMap<String, u64>>,
    idle: Mutex<HashMap<String, IdlePokeState>>,
    complete_wrapup: Mutex<HashSet<String>>,
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
        self.digest_rules
            .lock()
            .retain(|key, _| !key.starts_with(pane_id));
        self.idle.lock().remove(pane_id);
    }

    pub fn digest_rule_due(
        &self,
        pane_id: &str,
        rule_id: &str,
        now_secs: u64,
        interval_secs: u64,
    ) -> bool {
        if interval_secs == 0 {
            return false;
        }
        let key = format!("{pane_id}:{rule_id}");
        let last = self.digest_rules.lock().get(&key).copied();
        match last {
            None => true,
            Some(last_fired) => now_secs.saturating_sub(last_fired) >= interval_secs,
        }
    }

    pub fn mark_digest_rule_fired(&self, pane_id: &str, rule_id: &str, now_secs: u64) {
        let key = format!("{pane_id}:{rule_id}");
        self.digest_rules.lock().insert(key, now_secs);
    }

    pub fn complete_wrapup_due(&self, plan_id: &str) -> bool {
        !self.complete_wrapup.lock().contains(plan_id)
    }

    pub fn mark_complete_wrapup_sent(&self, plan_id: &str) {
        self.complete_wrapup.lock().insert(plan_id.to_string());
    }

    pub fn idle_poke_due(
        &self,
        pane_id: &str,
        now_secs: u64,
        max_per_day: u32,
        backoff_base_secs: u64,
    ) -> bool {
        if max_per_day == 0 {
            return false;
        }
        let mut idle = self.idle.lock();
        let day = day_bucket(now_secs);
        if let std::collections::hash_map::Entry::Vacant(vacant) = idle.entry(pane_id.to_string()) {
            let _ = vacant.insert(IdlePokeState {
                last_nudge_at_secs: now_secs,
                day_bucket: day,
                sent_today: 0,
            });
            return false;
        }
        let state = idle.get_mut(pane_id).unwrap_or_else(|| unreachable!());
        if state.day_bucket != day {
            state.day_bucket = day;
            state.sent_today = 0;
        }
        if state.sent_today >= max_per_day {
            return false;
        }
        let required_interval = idle_backoff_interval(backoff_base_secs, state.sent_today);
        now_secs.saturating_sub(state.last_nudge_at_secs) >= required_interval
    }

    pub fn mark_idle_nudged(&self, pane_id: &str, now_secs: u64) {
        let mut idle = self.idle.lock();
        let day = day_bucket(now_secs);
        let state = idle.entry(pane_id.to_string()).or_insert(IdlePokeState {
            last_nudge_at_secs: now_secs,
            day_bucket: day,
            sent_today: 0,
        });
        if state.day_bucket != day {
            state.day_bucket = day;
            state.sent_today = 0;
        }
        state.last_nudge_at_secs = now_secs;
        state.sent_today = state.sent_today.saturating_add(1);
    }

    pub fn idle_phrase_index(&self, pane_id: &str) -> u32 {
        self.idle
            .lock()
            .get(pane_id)
            .map(|state| state.sent_today)
            .unwrap_or(0)
    }

    pub fn get(&self, pane_id: &str) -> (u32, u32) {
        self.panes.lock().get(pane_id).copied().unwrap_or((0, 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{
        ElementCanvas, OrchestratorRef, PlanMode, Task, TaskLaneMetadata, TaskPriority, TaskStatus,
    };

    fn cfg(role: AgentRole) -> NudgeConfig {
        NudgeConfig {
            plan_reminders: true,
            nudge_after_turns: 12,
            role: Some(role),
        }
    }

    fn plan(id: &str, owner: &str, mode: PlanMode, tasks: Vec<Task>) -> Plan {
        let mut plan = Plan::new(
            "Reminder".into(),
            "/tmp/project".into(),
            OrchestratorRef::from_pane(owner),
        );
        plan.id = id.into();
        plan.mode = mode;
        plan.tasks = tasks;
        plan
    }

    fn task(id: &str, status: TaskStatus, pane_id: Option<&str>) -> Task {
        Task {
            id: id.into(),
            name: id.into(),
            kind: "status_update".into(),
            model: None,
            model_rationale: None,
            acceptance_criteria: vec![],
            test_plan: None,
            correctness: None,
            status,
            review_stage: Default::default(),
            qa_state: None,
            status_label: None,
            priority: TaskPriority::P2,
            created_at: 0,
            updated_at: 0,
            status_changed_at: 0,
            handle: None,
            tags: vec![],
            section_ids: vec![],
            body: String::new(),
            role_guidance: None,
            executor_role: None,
            competition: None,
            intent: None,
            parent_task_id: None,
            assigned_pane_id: pane_id.map(str::to_string),
            lane: TaskLaneMetadata::default(),
            taken_by_role: None,
            peer: None,
            group: None,
            canvas: ElementCanvas::default(),
            milestones: vec![],
            execution_attempts: vec![],
            park_reason: None,
            attachments: vec![],
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
    fn digest_rule_rate_limit_blocks_repeat_within_interval() {
        let counter = TurnCounter::new();
        assert!(counter.digest_rule_due("orch", "in_review_pressure", 1000, 3600));
        counter.mark_digest_rule_fired("orch", "in_review_pressure", 1000);
        assert!(!counter.digest_rule_due("orch", "in_review_pressure", 2000, 3600));
        assert!(counter.digest_rule_due("orch", "in_review_pressure", 4601, 3600));
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

    #[test]
    fn idle_poke_phrase_text_is_driven_by_shared_templates() {
        for (idx, template) in IDLE_POKE_QUESTION_TEMPLATES.iter().enumerate() {
            let rendered = idle_poke_text(3, 2, idx as u32);
            let expected = template
                .replace("{pending_tasks}", "3")
                .replace("{active_lanes}", "2");
            assert_eq!(rendered, expected);
        }
    }

    #[test]
    fn time_targets_include_execution_owner_and_worker() {
        let plans = vec![plan(
            "pl_1",
            "orch",
            PlanMode::Execution,
            vec![task("task_1", TaskStatus::Pending, Some("worker"))],
        )];

        assert_eq!(
            timed_plan_reminder_targets(&plans),
            vec![
                TimedPlanReminderTarget {
                    pane_id: "orch".into(),
                    kind: TimedPlanReminderKind::Owner {
                        plan_id: "pl_1".into()
                    }
                },
                TimedPlanReminderTarget {
                    pane_id: "worker".into(),
                    kind: TimedPlanReminderKind::Worker
                }
            ]
        );
    }

    #[test]
    fn time_targets_skip_paused_plans_with_open_owner_and_worker_work() {
        let mut paused = plan(
            "pl_paused",
            "orch",
            PlanMode::Execution,
            vec![task("task_1", TaskStatus::Pending, Some("worker"))],
        );
        paused.mode = PlanMode::Paused;
        paused.paused_from_mode = Some(PlanMode::Execution);

        assert_eq!(timed_plan_reminder_targets(&[paused]), Vec::new());
    }

    #[test]
    fn time_targets_skip_planning_and_terminal_work() {
        let plans = vec![
            plan(
                "pl_planning",
                "orch",
                PlanMode::Planning,
                vec![task("task_1", TaskStatus::Pending, Some("worker"))],
            ),
            plan(
                "pl_done",
                "done-orch",
                PlanMode::Execution,
                vec![task("task_2", TaskStatus::Done, Some("done-worker"))],
            ),
            plan(
                "pl_cancelled",
                "cancelled-orch",
                PlanMode::Execution,
                vec![task(
                    "task_3",
                    TaskStatus::Cancelled,
                    Some("cancelled-worker"),
                )],
            ),
        ];

        assert!(timed_plan_reminder_targets(&plans).is_empty());
    }

    #[test]
    fn time_targets_prioritize_owner_over_worker_for_same_pane() {
        let plans = vec![plan(
            "pl_1",
            "orch",
            PlanMode::Execution,
            vec![task("task_1", TaskStatus::Pending, Some("orch"))],
        )];

        assert_eq!(
            timed_plan_reminder_targets(&plans),
            vec![TimedPlanReminderTarget {
                pane_id: "orch".into(),
                kind: TimedPlanReminderKind::Owner {
                    plan_id: "pl_1".into()
                }
            }]
        );
    }

    #[test]
    fn time_targets_workers_only_for_report_pending_statuses() {
        let plans = vec![plan(
            "pl_1",
            "orch",
            PlanMode::Execution,
            vec![
                task("pending", TaskStatus::Pending, Some("p1")),
                task("working", TaskStatus::InProgress, Some("p2")),
                task("blocked", TaskStatus::Blocked, Some("p3")),
                task("review", TaskStatus::InReview, Some("p4")),
            ],
        )];
        let targets = timed_plan_reminder_targets(&plans);

        assert!(targets.iter().any(|target| target.pane_id == "p1"));
        assert!(targets.iter().any(|target| target.pane_id == "p2"));
        assert!(targets.iter().any(|target| target.pane_id == "p3"));
        assert!(!targets.iter().any(|target| target.pane_id == "p4"));
    }

    #[test]
    fn timed_targets_silent_for_all_done_and_complete_mode_plans() {
        let all_done = plan(
            "pl_done",
            "orch-done",
            PlanMode::Execution,
            vec![
                task("t1", TaskStatus::Done, Some("w1")),
                task("t2", TaskStatus::Done, Some("w2")),
                task("t3", TaskStatus::Cancelled, Some("w3")),
            ],
        );
        let mut complete_mode = plan(
            "pl_complete",
            "orch-complete",
            PlanMode::Execution,
            vec![task("t4", TaskStatus::Done, None)],
        );
        complete_mode.mode = PlanMode::Complete;

        let targets = timed_plan_reminder_targets(&[all_done, complete_mode]);
        assert!(
            targets.is_empty(),
            "complete plans must produce zero timed reminder targets: {targets:?}"
        );
    }

    #[test]
    fn timed_targets_fire_for_in_review_open_work() {
        let plans = vec![plan(
            "pl_review",
            "orch-review",
            PlanMode::Execution,
            vec![
                task("t_done", TaskStatus::Done, None),
                task("t_review", TaskStatus::InReview, Some("worker-review")),
            ],
        )];
        let targets = timed_plan_reminder_targets(&plans);
        assert!(
            targets.iter().any(|t| t.pane_id == "orch-review"),
            "in_review open work must keep plan non-complete and owner nudge-eligible"
        );
    }

    #[test]
    fn idle_poke_due_uses_exponential_backoff() {
        let counter = TurnCounter::new();
        assert!(!counter.idle_poke_due("idle-owner", 0, 3, 10));
        assert!(counter.idle_poke_due("idle-owner", 1, 3, 10));
        counter.mark_idle_nudged("idle-owner", 1);

        assert!(!counter.idle_poke_due("idle-owner", 10, 3, 10));
        assert!(counter.idle_poke_due("idle-owner", 11, 3, 10));
        counter.mark_idle_nudged("idle-owner", 11);

        assert!(!counter.idle_poke_due("idle-owner", 30, 3, 10));
        assert!(counter.idle_poke_due("idle-owner", 31, 3, 10));
    }

    #[test]
    fn idle_poke_respects_daily_cap() {
        let counter = TurnCounter::new();
        assert!(!counter.idle_poke_due("daily", 0, 2, 10));
        assert!(counter.idle_poke_due("daily", 1, 2, 10));
        counter.mark_idle_nudged("daily", 1);

        assert!(counter.idle_poke_due("daily", 11, 2, 10));
        counter.mark_idle_nudged("daily", 11);

        assert!(!counter.idle_poke_due("daily", 200, 2, 10));
    }

    #[test]
    fn idle_poke_state_resets_with_common_counter_reset() {
        let counter = TurnCounter::new();
        assert!(!counter.idle_poke_due("reset", 0, 5, 10));
        assert!(counter.idle_poke_due("reset", 1, 5, 10));
        counter.mark_idle_nudged("reset", 1);
        counter.reset("reset");
        assert!(!counter.idle_poke_due("reset", 1, 5, 10));
        assert!(counter.idle_poke_due("reset", 2, 5, 10));
    }
}

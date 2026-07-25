use codeterm_types::agent::AgentRole;
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

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
        Some(AgentRole::Manager) => Some(NudgeKind::Worker),
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
            "Plan reminder: this tab owns unfinished execution-plan work. This nudge {CONTRACT} Give every open task one of five exits: {EXITS}. Read `codeterm docs ORCHESTRATION` if the orchestration loop is unclear, then process reports, reconcile, review, close, or bounce tasks."
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

// Consecutive coordinator polls of a worker pane before one anti-polling nudge fires.
pub const POLL_NUDGE_THRESHOLD: u32 = 3;
pub const UNCHANGED_REMINDER_COOLDOWN_SECS: u64 = 300;

pub fn polling_nudge_text() -> String {
    "Polling smell: worker reports auto-inject into this tab the moment they land — repeatedly reading a worker tab's output/text/chat just burns your turn. Wait for the report; don't poll.".to_string()
}

// Verbatim heartbeat/nudge contract — MUST appear in every open-work nudge.
pub const CONTRACT: &str = "repeats until every open task is dispatched, paused with a reason, or sent to replanning; silence is not an option.";

// The five sanctioned exits for an open task.
pub const EXITS: &str = "(1) dispatch it to a worker, (2) pause with a reason (`codeterm plan task pause <plan> <task> --reason \"<why>\"`), (3) judge an in_review task to done or bounce it, (4) send it to replanning, (5) escalate to a smarter model";

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

#[derive(Debug, Default, Clone, Copy)]
struct PollState {
    consecutive: u32,
    nudged: bool,
}

/// Scheduler telemetry is deliberately independent from turn/activity state.
/// A pane can be continuously active and still retain its time deadline.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct TimeReminderTelemetry {
    pub last_tick: Option<u64>,
    pub last_due: Option<u64>,
    pub last_delivery: Option<u64>,
    pub delivery_failure_retry_reason: Option<String>,
}

#[derive(Debug, Default)]
pub struct TurnCounter {
    panes: Mutex<HashMap<String, (u32, u32)>>,
    timed: Mutex<HashMap<String, u64>>,
    time_telemetry: Mutex<HashMap<String, TimeReminderTelemetry>>,
    digest_rules: Mutex<HashMap<String, u64>>,
    idle: Mutex<HashMap<String, IdlePokeState>>,
    complete_wrapup: Mutex<HashSet<String>>,
    polls: Mutex<HashMap<String, PollState>>,
    reminder_content: Mutex<HashMap<String, (String, u64)>>,
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
        self.time_nudge_due_with_initial_state(pane_id, now_secs, interval_secs, false)
    }

    pub fn idle_owner_time_nudge_due(
        &self,
        pane_id: &str,
        now_secs: u64,
        interval_secs: u64,
    ) -> bool {
        self.time_nudge_due_with_initial_state(pane_id, now_secs, interval_secs, true)
    }

    fn time_nudge_due_with_initial_state(
        &self,
        pane_id: &str,
        now_secs: u64,
        interval_secs: u64,
        initially_due: bool,
    ) -> bool {
        if interval_secs == 0 {
            return false;
        }
        let mut timed = self.timed.lock();
        let Some(last_nudge) = timed.get(pane_id).copied() else {
            let initial_deadline = if initially_due {
                now_secs.saturating_sub(interval_secs)
            } else {
                now_secs
            };
            timed.insert(pane_id.to_string(), initial_deadline);
            return initially_due;
        };
        now_secs.saturating_sub(last_nudge) >= interval_secs
    }

    pub fn mark_time_nudged(&self, pane_id: &str, now_secs: u64) {
        self.timed.lock().insert(pane_id.to_string(), now_secs);
    }

    pub fn reminder_content_due(&self, pane_id: &str, state_key: &str, now_secs: u64) -> bool {
        self.reminder_content
            .lock()
            .get(pane_id)
            .is_none_or(|(last_key, last_at)| {
                last_key != state_key
                    || now_secs.saturating_sub(*last_at) >= UNCHANGED_REMINDER_COOLDOWN_SECS
            })
    }

    pub fn mark_reminder_content_delivered(&self, pane_id: &str, state_key: String, now_secs: u64) {
        self.reminder_content
            .lock()
            .insert(pane_id.to_string(), (state_key, now_secs));
    }

    /// Reset counters coupled to normal pane activity, without touching the
    /// independent time-reminder deadline. Use this for Idle->Working and
    /// ordinary reports.
    pub fn reset_activity(&self, pane_id: &str) {
        self.panes.lock().insert(pane_id.to_string(), (0, 0));
        self.digest_rules
            .lock()
            .retain(|key, _| !key.starts_with(pane_id));
        self.idle.lock().remove(pane_id);
        self.polls.lock().remove(pane_id);
        self.reminder_content.lock().remove(pane_id);
    }

    /// Clear all reminder state when a pane is no longer reminder-eligible.
    pub fn reset(&self, pane_id: &str) {
        self.reset_activity(pane_id);
        self.timed.lock().remove(pane_id);
        self.time_telemetry.lock().remove(pane_id);
    }

    pub fn retain_time_reminders(&self, eligible_panes: &HashSet<String>) {
        self.timed
            .lock()
            .retain(|pane_id, _| eligible_panes.contains(pane_id));
        self.time_telemetry
            .lock()
            .retain(|pane_id, _| eligible_panes.contains(pane_id));
    }

    pub fn clear_time_reminders(&self) {
        self.timed.lock().clear();
        self.time_telemetry.lock().clear();
    }

    pub fn record_time_tick(&self, pane_id: &str, now_secs: u64) {
        self.time_telemetry
            .lock()
            .entry(pane_id.to_string())
            .or_default()
            .last_tick = Some(now_secs);
    }

    pub fn record_time_due(&self, pane_id: &str, now_secs: u64) {
        self.time_telemetry
            .lock()
            .entry(pane_id.to_string())
            .or_default()
            .last_due = Some(now_secs);
    }

    pub fn record_time_delivery(&self, pane_id: &str, now_secs: u64) {
        let mut telemetry = self.time_telemetry.lock();
        let entry = telemetry.entry(pane_id.to_string()).or_default();
        entry.last_delivery = Some(now_secs);
        entry.delivery_failure_retry_reason = None;
    }

    pub fn record_time_failure(&self, pane_id: &str, reason: impl Into<String>) {
        self.time_telemetry
            .lock()
            .entry(pane_id.to_string())
            .or_default()
            .delivery_failure_retry_reason = Some(reason.into());
    }

    pub fn time_reminder_telemetry(&self, pane_id: &str) -> TimeReminderTelemetry {
        self.time_telemetry
            .lock()
            .get(pane_id)
            .cloned()
            .unwrap_or_default()
    }

    // Count one poll by `pane_id`; return true exactly once when it crosses the
    // threshold, then stay silent until `reset_polls` clears the streak.
    pub fn record_poll(&self, pane_id: &str) -> bool {
        let mut polls = self.polls.lock();
        let state = polls.entry(pane_id.to_string()).or_default();
        state.consecutive = state.consecutive.saturating_add(1);
        if state.consecutive >= POLL_NUDGE_THRESHOLD && !state.nudged {
            state.nudged = true;
            true
        } else {
            false
        }
    }

    pub fn reset_polls(&self, pane_id: &str) {
        self.polls.lock().remove(pane_id);
    }

    pub fn poll_streak(&self, pane_id: &str) -> u32 {
        self.polls.lock().get(pane_id).map_or(0, |s| s.consecutive)
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
        assert_eq!(
            should_nudge(12, 0, &cfg(AgentRole::Manager)),
            Some(NudgeKind::Worker)
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
            assert!(text.contains("repeats until every open task is dispatched, paused with a reason, or sent to replanning; silence is not an option."));
            assert!(text.contains("dispatch"));
            assert!(text.contains("pause with a reason"));
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
    fn activity_resets_do_not_restart_a_live_time_deadline() {
        let counter = TurnCounter::new();
        let interval = 30 * 60;
        assert!(!counter.time_nudge_due("orch", 0, interval));

        // Simulate 30 minutes of ordinary reports and Idle->Working transitions.
        for now in (60..=interval).step_by(60) {
            counter.reset_activity("orch");
            assert_eq!(counter.get("orch"), (0, 0));
            assert_eq!(
                counter.time_nudge_due("orch", now, interval),
                now == interval
            );
        }

        // Due survives more activity until a delivery acknowledges it.
        counter.reset_activity("orch");
        assert!(counter.time_nudge_due("orch", interval + 60, interval));
        counter.mark_time_nudged("orch", interval + 60);
        assert!(!counter.time_nudge_due("orch", interval + 61, interval));
    }

    #[test]
    fn unchanged_reminder_content_is_rate_limited_until_state_changes() {
        let counter = TurnCounter::new();
        assert!(counter.reminder_content_due("orch", "plan:1", 100));
        counter.mark_reminder_content_delivered("orch", "plan:1".into(), 100);
        assert!(!counter.reminder_content_due("orch", "plan:1", 101));
        assert!(counter.reminder_content_due("orch", "plan:2", 101));
        assert!(counter.reminder_content_due(
            "orch",
            "plan:1",
            100 + UNCHANGED_REMINDER_COOLDOWN_SECS
        ));
    }

    #[test]
    fn idle_owner_is_due_when_first_observed_and_then_respects_cadence() {
        let counter = TurnCounter::new();
        let interval = 30 * 60;
        let now = interval * 10;

        assert!(counter.idle_owner_time_nudge_due("idle-owner", now, interval));
        assert!(counter.idle_owner_time_nudge_due("idle-owner", now + 1, interval));
        let delivered_at = now + 1;
        counter.mark_time_nudged("idle-owner", delivered_at);
        assert!(!counter.idle_owner_time_nudge_due(
            "idle-owner",
            delivered_at + interval - 1,
            interval
        ));
        assert!(counter.idle_owner_time_nudge_due("idle-owner", delivered_at + interval, interval));
    }

    #[test]
    fn time_telemetry_tracks_due_delivery_and_retry_reason() {
        let counter = TurnCounter::new();
        counter.record_time_tick("orch", 10);
        counter.record_time_due("orch", 11);
        counter.record_time_failure("orch", "tab unavailable; retry on next tick");
        assert_eq!(
            counter.time_reminder_telemetry("orch"),
            TimeReminderTelemetry {
                last_tick: Some(10),
                last_due: Some(11),
                last_delivery: None,
                delivery_failure_retry_reason: Some("tab unavailable; retry on next tick".into()),
            }
        );
        counter.record_time_delivery("orch", 12);
        assert_eq!(
            counter
                .time_reminder_telemetry("orch")
                .delivery_failure_retry_reason,
            None
        );
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
    fn poll_nudge_fires_once_at_threshold_then_stays_silent() {
        let counter = TurnCounter::new();
        // First two polls below the threshold: silent.
        assert!(!counter.record_poll("orch"));
        assert!(!counter.record_poll("orch"));
        // Third poll crosses POLL_NUDGE_THRESHOLD == 3: exactly one nudge.
        assert!(counter.record_poll("orch"));
        // Fourth and later polls do not re-nudge until a reset.
        assert!(!counter.record_poll("orch"));
        assert!(!counter.record_poll("orch"));
        assert_eq!(counter.poll_streak("orch"), 5);
    }

    #[test]
    fn poll_streak_is_per_pane() {
        let counter = TurnCounter::new();
        assert!(!counter.record_poll("a"));
        assert!(!counter.record_poll("a"));
        // A different caller pane keeps its own independent streak.
        assert!(!counter.record_poll("b"));
        assert!(counter.record_poll("a"));
        assert_eq!(counter.poll_streak("b"), 1);
    }

    #[test]
    fn reset_polls_rearms_the_nudge() {
        let counter = TurnCounter::new();
        assert!(!counter.record_poll("orch"));
        assert!(!counter.record_poll("orch"));
        assert!(counter.record_poll("orch"));
        counter.reset_polls("orch");
        assert_eq!(counter.poll_streak("orch"), 0);
        // After reset the streak re-arms: three more polls fire one fresh nudge.
        assert!(!counter.record_poll("orch"));
        assert!(!counter.record_poll("orch"));
        assert!(counter.record_poll("orch"));
    }

    #[test]
    fn common_reset_clears_poll_streak() {
        let counter = TurnCounter::new();
        assert!(!counter.record_poll("orch"));
        assert!(!counter.record_poll("orch"));
        counter.reset("orch");
        assert_eq!(counter.poll_streak("orch"), 0);
        assert!(!counter.record_poll("orch"));
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

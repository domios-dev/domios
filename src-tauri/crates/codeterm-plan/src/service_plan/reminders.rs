use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::plan::Plan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NudgeKind {
    Worker,
    Orchestrator,
    StalledTeam,
}

pub const WORKER_REMINDER: &str = "Plan reminder: you are assigned to an active task. Run `codeterm agent report --plan <plan_id> --task <task_id> \"status update\"` so your parent can see progress.";

pub const ORCHESTRATOR_REMINDER: &str = "Plan reminder: actionable tasks: {actionable}. {parked} Dispatch, judge, unblock, or replan each actionable task. Tasks waiting on dependencies and machine-paused tasks need nothing; they advance when the dependency lands or the hold clears. Pause the plan only when the owner must decide before any task can advance. Read `codeterm docs ORCHESTRATION` if the loop is unclear.";

pub const ORCHESTRATOR_STALLED_TEAM_REMINDER: &str = "Stalled team: no live Worker remains for actionable tasks: {actionable}. {parked} Dispatch, unblock, or replan actionable tasks.";

pub fn worker_nudge_text() -> String {
    WORKER_REMINDER.to_string()
}

pub fn orchestrator_nudge_text(plan: &Plan) -> String {
    let actionable = super::state::orchestrator_actionable_task_ids(plan);
    let listed = if actionable.is_empty() {
        "none".to_string()
    } else {
        actionable.join(", ")
    };
    ORCHESTRATOR_REMINDER
        .replace("{actionable}", &listed)
        .replace("{parked}", &parked_later_group_summary(plan))
}

pub fn parked_later_group_summary(plan: &Plan) -> String {
    let count = super::state::parked_later_group_task_count(plan);
    if count == 0 {
        String::new()
    } else {
        format!("{count} tasks parked for later groups.")
    }
}

pub const UNCHANGED_REMINDER_COOLDOWN_SECS: u64 = 300;

/// Scheduler telemetry is deliberately independent from turn/activity state.
/// A tab can be continuously active and still retain its time deadline.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct TimeReminderTelemetry {
    pub last_tick: Option<u64>,
    pub last_due: Option<u64>,
    pub last_delivery: Option<u64>,
    pub delivery_failure_retry_reason: Option<String>,
}

#[derive(Debug, Default)]
pub struct TurnCounter {
    timed: Mutex<HashMap<String, u64>>,
    time_telemetry: Mutex<HashMap<String, TimeReminderTelemetry>>,
    digest_rules: Mutex<HashMap<String, u64>>,
    complete_wrapup: Mutex<HashSet<String>>,
    reminder_content: Mutex<HashMap<String, (String, u64)>>,
}

impl TurnCounter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn time_nudge_due(&self, tab_id: &str, now_secs: u64, interval_secs: u64) -> bool {
        if interval_secs == 0 {
            return false;
        }
        let mut timed = self.timed.lock();
        let Some(last_nudge) = timed.get(tab_id).copied() else {
            timed.insert(tab_id.to_string(), now_secs);
            return false;
        };
        now_secs.saturating_sub(last_nudge) >= interval_secs
    }

    pub fn mark_time_nudged(&self, tab_id: &str, now_secs: u64) {
        self.timed.lock().insert(tab_id.to_string(), now_secs);
    }

    pub fn reminder_content_due(&self, tab_id: &str, state_key: &str, now_secs: u64) -> bool {
        self.reminder_content
            .lock()
            .get(tab_id)
            .is_none_or(|(last_key, last_at)| {
                last_key != state_key
                    || now_secs.saturating_sub(*last_at) >= UNCHANGED_REMINDER_COOLDOWN_SECS
            })
    }

    pub fn mark_reminder_content_delivered(&self, tab_id: &str, state_key: String, now_secs: u64) {
        self.reminder_content
            .lock()
            .insert(tab_id.to_string(), (state_key, now_secs));
    }

    /// Report activity may refresh digest/content state, but never the
    /// independent scheduler deadline.
    pub fn reset_activity(&self, tab_id: &str) {
        self.digest_rules
            .lock()
            .retain(|key, _| !key.starts_with(tab_id));
        self.reminder_content.lock().remove(tab_id);
    }

    pub fn reset(&self, tab_id: &str) {
        self.reset_activity(tab_id);
        self.timed.lock().remove(tab_id);
        self.time_telemetry.lock().remove(tab_id);
    }

    pub fn retain_time_reminders(&self, eligible_tabs: &HashSet<String>) {
        self.timed
            .lock()
            .retain(|tab_id, _| eligible_tabs.contains(tab_id));
        self.time_telemetry
            .lock()
            .retain(|tab_id, _| eligible_tabs.contains(tab_id));
    }

    pub fn clear_time_reminders(&self) {
        self.timed.lock().clear();
        self.time_telemetry.lock().clear();
    }

    pub fn record_time_tick(&self, tab_id: &str, now_secs: u64) {
        self.time_telemetry
            .lock()
            .entry(tab_id.to_string())
            .or_default()
            .last_tick = Some(now_secs);
    }

    pub fn record_time_due(&self, tab_id: &str, now_secs: u64) {
        self.time_telemetry
            .lock()
            .entry(tab_id.to_string())
            .or_default()
            .last_due = Some(now_secs);
    }

    pub fn record_time_delivery(&self, tab_id: &str, now_secs: u64) {
        let mut telemetry = self.time_telemetry.lock();
        let entry = telemetry.entry(tab_id.to_string()).or_default();
        entry.last_delivery = Some(now_secs);
        entry.delivery_failure_retry_reason = None;
    }

    pub fn record_time_failure(&self, tab_id: &str, reason: impl Into<String>) {
        self.time_telemetry
            .lock()
            .entry(tab_id.to_string())
            .or_default()
            .delivery_failure_retry_reason = Some(reason.into());
    }

    pub fn time_reminder_telemetry(&self, tab_id: &str) -> TimeReminderTelemetry {
        self.time_telemetry
            .lock()
            .get(tab_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn digest_rule_due(
        &self,
        tab_id: &str,
        rule_id: &str,
        now_secs: u64,
        interval_secs: u64,
    ) -> bool {
        if interval_secs == 0 {
            return false;
        }
        let key = format!("{tab_id}:{rule_id}");
        let last = self.digest_rules.lock().get(&key).copied();
        match last {
            None => true,
            Some(last_fired) => now_secs.saturating_sub(last_fired) >= interval_secs,
        }
    }

    pub fn mark_digest_rule_fired(&self, tab_id: &str, rule_id: &str, now_secs: u64) {
        let key = format!("{tab_id}:{rule_id}");
        self.digest_rules.lock().insert(key, now_secs);
    }

    pub fn complete_wrapup_due(&self, plan_id: &str) -> bool {
        !self.complete_wrapup.lock().contains(plan_id)
    }

    pub fn mark_complete_wrapup_sent(&self, plan_id: &str) {
        self.complete_wrapup.lock().insert(plan_id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, TaskGroup};

    #[test]
    fn digest_rule_rate_limit_blocks_repeat_within_interval() {
        let counter = TurnCounter::new();
        assert!(counter.digest_rule_due("orch", "in_review_pressure", 1000, 3600));
        counter.mark_digest_rule_fired("orch", "in_review_pressure", 1000);
        assert!(!counter.digest_rule_due("orch", "in_review_pressure", 2000, 3600));
        assert!(counter.digest_rule_due("orch", "in_review_pressure", 4601, 3600));
    }

    #[test]
    fn report_activity_does_not_restart_scheduler_deadline() {
        let counter = TurnCounter::new();
        let interval = 30 * 60;
        assert!(!counter.time_nudge_due("orch", 0, interval));

        // Simulate 30 minutes of ordinary reports.
        for now in (60..=interval).step_by(60) {
            counter.reset_activity("orch");
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
    fn owner_reminder_lists_later_group_work_as_one_count() {
        let mut plan = Plan::new(
            "P".into(),
            "/repo".into(),
            OrchestratorRef::from_tab("orch"),
        );
        plan.task_groups = vec![
            TaskGroup {
                key: "release".into(),
                label: "Release".into(),
                color: None,
            },
            TaskGroup {
                key: "after-release".into(),
                label: "After release".into(),
                color: None,
            },
        ];
        plan.tasks = vec![
            serde_json::from_value(serde_json::json!({
                "id": "release-task",
                "name": "Release",
                "type": "implement",
                "model": null,
                "model_rationale": null,
                "test_plan": null,
                "correctness": null,
                "status": "in_progress",
                "status_label": null,
                "intent": null,
                "assigned_tab_id": null,
                "peer": null,
                "group": "release"
            }))
            .unwrap(),
            serde_json::from_value(serde_json::json!({
                "id": "after-release-task",
                "name": "After release",
                "type": "implement",
                "model": null,
                "model_rationale": null,
                "test_plan": null,
                "correctness": null,
                "status": "pending",
                "status_label": null,
                "intent": null,
                "assigned_tab_id": null,
                "peer": null,
                "group": "after-release",
                "depends_on": ["planned-later"]
            }))
            .unwrap(),
        ];

        let text = orchestrator_nudge_text(&plan);
        let summary = "1 tasks parked for later groups.";
        assert_eq!(text.matches(summary).count(), 1);
        assert!(!text.contains("after-release-task"));
    }

    #[test]
    fn idle_owner_waits_for_the_first_full_interval() {
        let counter = TurnCounter::new();
        let interval = 30 * 60;
        let now = interval * 10;

        assert!(!counter.time_nudge_due("idle-owner", now, interval));
        assert!(!counter.time_nudge_due("idle-owner", now + interval - 1, interval));
        assert!(counter.time_nudge_due("idle-owner", now + interval, interval));
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
}

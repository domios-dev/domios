use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use super::PlanStateSnapshot;
use crate::service_plan::next_actions::NextActionKind;

pub const STATE_DETAIL_LIMIT: usize = 50;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct StateWindowRequest {
    pub page: usize,
    pub all: bool,
}

impl Default for StateWindowRequest {
    fn default() -> Self {
        Self {
            page: 1,
            all: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DetailWindow {
    pub page: usize,
    pub total: usize,
    pub returned: usize,
    pub more: usize,
    pub next_page: Option<usize>,
    pub hint: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct StateDetails {
    pub totals: BTreeMap<String, usize>,
    pub live_open_lanes: usize,
    pub live_blocked_lanes: usize,
    pub next_action_counts: BTreeMap<NextActionKind, usize>,
    pub windows: BTreeMap<String, DetailWindow>,
}

impl StateDetails {
    fn nested_window<T>(&mut self, name: &str, rows: &mut Vec<T>, request: StateWindowRequest) {
        self.window(
            name,
            rows,
            StateWindowRequest {
                page: 1,
                all: request.all,
            },
        );
        if let Some(window) = self.windows.get_mut(name) {
            window.hint = format!("{} more; use --all", window.more);
        }
    }

    fn window<T>(&mut self, name: &str, rows: &mut Vec<T>, request: StateWindowRequest) {
        let total = rows.len();
        self.totals.insert(name.into(), total);
        if request.all || (total <= STATE_DETAIL_LIMIT && request.page <= 1) {
            return;
        }
        let page = request.page.max(1);
        let offset = page
            .saturating_sub(1)
            .saturating_mul(STATE_DETAIL_LIMIT)
            .min(total);
        let end = offset.saturating_add(STATE_DETAIL_LIMIT).min(total);
        rows.truncate(end);
        rows.drain(..offset);
        let more = total.saturating_sub(end);
        let next_page = (more > 0).then(|| page.saturating_add(1));
        let hint = match next_page {
            Some(next) => format!("{more} more; use --page {next} or --all"),
            None => format!("{total} total; use --page 1 or --all"),
        };
        self.windows.insert(
            name.into(),
            DetailWindow {
                page,
                total,
                returned: rows.len(),
                more,
                next_page,
                hint,
            },
        );
    }
}

impl PlanStateSnapshot {
    pub fn window_details(&mut self, request: StateWindowRequest) {
        let agents: HashSet<_> = self
            .live_agents
            .iter()
            .map(|agent| agent.task_id.as_str())
            .collect();
        let mut details = StateDetails {
            live_open_lanes: self
                .open_lanes
                .iter()
                .filter(|lane| agents.contains(lane.task_id.as_str()))
                .count(),
            live_blocked_lanes: self
                .blocked_lanes
                .iter()
                .filter(|lane| agents.contains(lane.task_id.as_str()))
                .count(),
            ..Default::default()
        };
        for action in &self.next_actions {
            *details.next_action_counts.entry(action.kind).or_default() += 1;
        }
        details.window("next_actions", &mut self.next_actions, request);
        details.window("open_lanes", &mut self.open_lanes, request);
        details.window("blocked_lanes", &mut self.blocked_lanes, request);
        details.window("lanes", &mut self.lanes, request);
        details.window("live_agents", &mut self.live_agents, request);
        details.window("cleanup_debt", &mut self.cleanup_debt, request);
        details.window("loops", &mut self.loops, request);
        details.window("drift.items", &mut self.drift.items, request);
        details.window("coverage.sections", &mut self.coverage.sections, request);
        details.window("coverage.gaps", &mut self.coverage.gaps, request);
        details.window(
            "coverage.unlinked_tasks",
            &mut self.coverage.unlinked_tasks,
            request,
        );
        details.window(
            "pending_regression_gates",
            &mut self.pending_regression_gates,
            request,
        );
        details.window(
            "blocking_regression_gates",
            &mut self.blocking_regression_gates,
            request,
        );
        details.window(
            "pending_group_retrospectives",
            &mut self.pending_group_retrospectives,
            request,
        );
        details.window(
            "regression_overrides",
            &mut self.regression_overrides,
            request,
        );
        details.window(
            "heartbeat.active_lanes",
            &mut self.heartbeat.active_lanes,
            request,
        );
        details.window(
            "mission_control.held.held_reviews",
            &mut self.mission_control.held.held_reviews,
            request,
        );
        details.window(
            "mission_control.held.open_gates",
            &mut self.mission_control.held.open_gates,
            request,
        );
        details.window(
            "mission_control.event_log.entries",
            &mut self.mission_control.event_log.entries,
            request,
        );
        details.window(
            "mission_control.areas.groups",
            &mut self.mission_control.areas.groups,
            request,
        );
        details.window(
            "mission_control.areas.role_fallback",
            &mut self.mission_control.areas.role_fallback,
            request,
        );
        for lane in self.open_lanes.iter_mut().chain(&mut self.blocked_lanes) {
            details.nested_window(
                &format!("tasks.{}.subtasks", lane.task_id),
                &mut lane.subtasks,
                request,
            );
            details.nested_window(
                &format!("tasks.{}.dependency_waits", lane.task_id),
                &mut lane.dependency_waits,
                request,
            );
        }
        for group in &mut self.mission_control.areas.groups {
            details.nested_window(
                &format!("areas.{}.task_ids", group.value),
                &mut group.task_ids,
                request,
            );
        }
        for group in &mut self.mission_control.areas.role_fallback {
            details.nested_window(
                &format!(
                    "roles.{}.task_ids",
                    group.executor_role.as_deref().unwrap_or("unassigned")
                ),
                &mut group.task_ids,
                request,
            );
        }
        for (index, event) in self
            .mission_control
            .event_log
            .entries
            .iter_mut()
            .enumerate()
        {
            details.nested_window(
                &format!("events.{index}.linked_tasks"),
                &mut event.linked_tasks,
                request,
            );
        }
        if details.totals["mission_control.event_log.entries"]
            > self.mission_control.event_log.entries.len()
        {
            self.mission_control.event_log.truncated = true;
        }
        self.details = Some(details);
    }
}

#[cfg(test)]
mod tests;

#[derive(Debug, Serialize)]
pub struct NextActionsSnapshot {
    pub plan_id: String,
    pub next_actions: Vec<crate::service_plan::next_actions::NextAction>,
    pub blocked_or_gated_count: usize,
    pub details: Option<StateDetails>,
}

impl PlanStateSnapshot {
    pub fn into_next_actions(self) -> NextActionsSnapshot {
        let details = self.details.map(|mut details| {
            details.totals.retain(|name, _| name == "next_actions");
            details.windows.retain(|name, _| name == "next_actions");
            details
        });
        NextActionsSnapshot {
            plan_id: self.plan_id,
            next_actions: self.next_actions,
            blocked_or_gated_count: self.blocked_or_gated_count,
            details,
        }
    }
}

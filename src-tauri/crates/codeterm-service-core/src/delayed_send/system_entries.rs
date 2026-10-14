use serde::{Deserialize, Serialize};

use crate::service::hierarchy::plan_reminder_nudge::{
    current_epoch_secs, project_for_tab, ProjectedHeartbeat, ProjectedTick,
};
use crate::service::ServiceContext;

use super::{DelayedMessage, DelayedMessageState, DelayedMessageView, RecurrenceRule};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOrigin {
    User,
    System,
}

impl Default for MessageOrigin {
    fn default() -> Self {
        Self::User
    }
}

impl MessageOrigin {
    pub(crate) fn is_user(&self) -> bool {
        matches!(self, Self::User)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryTickView {
    pub at_ms: u64,
    pub outcome: String,
    pub spinal_verdict: String,
    pub spinal_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemEntryRefusal {
    pub code: &'static str,
    pub origin: &'static str,
}

pub(crate) fn is_system_entry_id(id: &str) -> bool {
    id.starts_with("system:")
}

pub fn system_entry_refusal(id: &str) -> Option<SystemEntryRefusal> {
    is_system_entry_id(id).then_some(SystemEntryRefusal {
        code: "system_entry_immutable",
        origin: "system",
    })
}

pub(crate) fn refuse_system_mutation(id: &str) -> Result<(), String> {
    if is_system_entry_id(id) {
        Err("system_entry_immutable".into())
    } else {
        Ok(())
    }
}

pub(super) fn views_for_tab(svc: &ServiceContext, tab_id: &str) -> Vec<DelayedMessageView> {
    let cfg = svc.configuration().orchestration;
    let plans = svc.plan_store.list_full_reminder_candidates();
    let projected = project_for_tab(
        &plans,
        tab_id,
        cfg.nudge_interval_secs,
        current_epoch_secs(),
        svc.plan_turn_counter.time_nudge_deadline(tab_id),
        &svc.heartbeat_gate.ticks_for_tab(tab_id),
        cfg.plan_reminders,
    );
    projected
        .into_iter()
        .map(|item| super::to_view(svc, message_from_projection(item)))
        .collect()
}

pub(crate) fn message_from_projection(item: ProjectedHeartbeat) -> DelayedMessage {
    DelayedMessage {
        id: item.id,
        tab_id: item.tab_id,
        workspace_id: None,
        text: item.text,
        scheduled_at_ms: item.next_due_ms,
        created_at_ms: 0,
        state: DelayedMessageState::Queued,
        error: None,
        session_id_at_create: None,
        created_by_tab: None,
        created_by_label: None,
        created_by_model: None,
        recurrence: Some(RecurrenceRule {
            every_ms: item.cadence_ms,
            until_ms: None,
            count: None,
        }),
        series_start_at_ms: Some(item.next_due_ms),
        tick_index: 0,
        skipped_ticks: 0,
        team_id: None,
        last_tick_unconfirmed: false,
        last_tick_send_sequence: 0,
        origin: MessageOrigin::System,
        reason: Some(item.reason),
        ticks: item.ticks.into_iter().map(tick_view).collect(),
    }
}

fn tick_view(tick: ProjectedTick) -> DeliveryTickView {
    DeliveryTickView {
        at_ms: tick.at_ms,
        outcome: tick.outcome,
        spinal_verdict: tick.spinal_verdict,
        spinal_reason: tick.spinal_reason,
    }
}

pub use super::store_db::hot_write::runtime::{
    CuratorRoundRows, CuratorSaveRows, CuratorStateRows,
};
use super::{PlanError, PlanStore};
use crate::plan::{
    ControlLoop, ControlLoopMode, CuratorRound, HeartbeatLearning, Plan, PlanMode, PlanStatus,
    CURATOR_IDLE_FLOOR_MS,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod digest;
pub mod movement;
pub mod note;
pub mod synthesis;
pub fn request_round(state: &mut ControlLoop, eligible: bool, now: u64) -> Result<(), String> {
    if !eligible || state.mode != ControlLoopMode::Curator || state.owner_heartbeat() {
        return Err("Curator round is suspended or Curator mode is unavailable".into());
    }
    if state.running.is_some() || state.manual_run_requested {
        return Err("Curator round is already running or requested".into());
    }
    state.manual_run_requested = true;
    state.next_at = Some(now);
    Ok(())
}
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DueAction {
    Spawn,
    Nudge,
    Quiet,
}

pub fn due_action(activity: bool, actionable: bool, last_real: Option<u64>, now: u64) -> DueAction {
    if activity || last_real.is_some_and(|at| now.saturating_sub(at) >= CURATOR_IDLE_FLOOR_MS) {
        DueAction::Spawn
    } else if actionable {
        DueAction::Nudge
    } else {
        DueAction::Quiet
    }
}

pub fn eligible(plan: &Plan, enabled: bool, human_wait: bool) -> bool {
    EligibilityInput {
        team_bound: plan.team_id.is_some(),
        status: &plan.status,
        mode: &plan.mode,
        complete: super::heartbeat::is_plan_complete(plan),
        control_mode: &plan.control_loop.mode,
    }
    .eligible(enabled, human_wait)
}

pub struct EligibilityInput<'a> {
    pub team_bound: bool,
    pub status: &'a PlanStatus,
    pub mode: &'a PlanMode,
    pub complete: bool,
    pub control_mode: &'a ControlLoopMode,
}

impl EligibilityInput<'_> {
    pub fn eligible(&self, enabled: bool, human_wait: bool) -> bool {
        enabled
            && !human_wait
            && self.team_bound
            && *self.status == PlanStatus::Active
            && matches!(self.mode, PlanMode::Approved | PlanMode::Execution)
            && !self.complete
            && *self.control_mode == ControlLoopMode::Curator
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct ControlLoopUpdate {
    pub mode: ControlLoopMode,
    pub heartbeat_interval_secs: Option<u64>,
}

impl PlanStore {
    pub fn set_control_loop(
        &self,
        id: &str,
        update: ControlLoopUpdate,
        now: u64,
    ) -> Result<ControlLoop, PlanError> {
        if let Some(interval) = update.heartbeat_interval_secs {
            if update.mode != ControlLoopMode::Heartbeat
                || ![900, 1800, 2700, 3600, 5400, 7200].contains(&interval)
            {
                return Err(PlanError::Validation("heartbeat interval must be 15, 30, 45, 60, 90 or 120 minutes, in heartbeat mode".into()));
            }
        }
        self.edit_curator_state(id, now, |plan| {
            plan.control_loop.mode = update.mode;
            plan.control_loop.manual_run_requested = false;
            plan.control_loop.heartbeat_interval_secs = update.heartbeat_interval_secs;
            plan.control_loop.next_at = None;
            plan.control_loop.fallback_reason = None;
            Ok(plan.control_loop.clone())
        })
    }

    pub fn record_curator_round(&self, id: &str, round: CuratorRound) -> Result<(), PlanError> {
        self.edit_curator_round(id, round.ended_at, |plan| {
            plan.append_round(round);
            Ok(())
        })
    }

    pub fn finish_curator_round(
        &self,
        id: &str,
        number: u64,
        note: &note::CuratorNote,
        now: u64,
    ) -> Result<CuratorRound, PlanError> {
        self.edit_curator_round(id, now, |plan| {
            let run = plan
                .control_loop
                .running
                .as_ref()
                .filter(|r| r.number == number)
                .ok_or_else(|| PlanError::Validation("Curator round is no longer active".into()))?
                .clone();
            if note.ready.is_some() && !run.synthesis {
                return Err(PlanError::Validation(
                    "READY is only valid in a synthesis round".into(),
                ));
            }
            for (tag, lines) in [
                ("curator", &note.retro),
                ("curator", &note.learnings),
                ("curator:next", &note.next_checks),
            ] {
                for text in lines {
                    plan.heartbeat.learnings.push(HeartbeatLearning {
                        id: crate::plan::ids::short_id("learn"),
                        text: text.clone(),
                        ts: now,
                        tag: Some(tag.into()),
                    });
                }
            }
            let round = CuratorRound {
                run_id: run.run_id,
                tab_id: run.tab_id,
                number,
                started_at: run.started_at,
                ended_at: now,
                outcome: if plan.control_loop.mode == ControlLoopMode::Curator {
                    codeterm_types::plan::CuratorOutcome::Done
                } else {
                    codeterm_types::plan::CuratorOutcome::ModeChanged
                },
                summary: note.ready.as_ref().map_or_else(
                    || note.next_move.clone(),
                    |ready| format!("{}\nREADY: {ready}", note.next_move),
                ),
                synthesis: run.synthesis,
                move_line: note.next_move.clone(),
                retro: note.retro.clone(),
                learn: note.learnings.clone(),
                next_checks: note.next_checks.clone(),
                ready: note.ready.clone(),
                next_delay_secs: plan.control_loop.delay(Some(note.next_secs)),
                reason: note.reason.clone(),
                learnings: (note.retro.len() + note.learnings.len() + note.next_checks.len())
                    as u64,
                asked: note.asked,
                digest_hash: run.digest_hash,
                section_sizes: run.section_sizes,
                turns: run.turns,
                output_tokens: run.output_tokens,
                save_id: run.save_id,
                duration_ms: now.saturating_sub(run.started_at),
            };
            plan.control_loop.consecutive_failures = 0;
            plan.control_loop.fallback_reason = None;
            plan.control_loop.running = None;
            plan.control_loop.last_real_round_at = Some(now);
            plan.append_round(round.clone());
            Ok(round)
        })
    }
}

pub fn append_round(plan: &mut Plan, round: CuratorRound) {
    plan.control_loop.task_states = plan
        .tasks
        .iter()
        .map(|t| (t.id.clone(), t.status.key().to_string()))
        .collect();
    plan.control_loop
        .defer(round.ended_at, Some(round.next_delay_secs));
    let text = serde_json::to_string(&round).unwrap_or_default();
    plan.strides.push(crate::plan::Milestone {
        id: crate::plan::ids::short_id("ms"),
        from: Default::default(),
        kind: "curator_round".into(),
        text,
        ts: round.ended_at,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    });
    plan.control_loop.record(round);
}

pub fn section_sizes(sections: &[(String, String)]) -> BTreeMap<String, usize> {
    sections
        .iter()
        .map(|(name, body)| (name.clone(), body.len()))
        .collect()
}

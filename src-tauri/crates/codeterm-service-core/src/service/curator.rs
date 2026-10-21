use super::{AuthenticatedPlanCaller, ServiceContext};
use crate::cron::store::{commit, CronChange, CronCreateParams, CronJob, CronRunStatus, Prepared};
use crate::plan::{
    curator_delay, ControlLoop, CuratorGrant, CuratorRound, CuratorRun, Plan, PlanMode, PlanStatus,
};
use codeterm_plan::service_plan::curator::{self, digest, note, ControlLoopUpdate, DueAction};
use serde_json::json;

mod authority;
mod save;
mod skill;

pub struct CuratorGrantScope {
    pub blueprints: Vec<String>,
    pub plan_skills: Vec<String>,
    pub saves: Vec<String>,
}
mod digest_sources;
mod scheduling;
#[cfg(test)]
mod tests;
pub use authority::CuratorIdentity;

impl ServiceContext {
    pub(crate) fn curator_heartbeat_body(&self, plan: &Plan) -> String {
        let heartbeat = self.plan_store.prepend_heartbeat_for_plan(&plan.id, "");
        let actions = crate::service::plan::reminders::composition::action_groups(plan);
        let parked = crate::service::plan::reminders::parked_later_group_summary(plan);
        crate::service::plan::reminders::composition::compose(
            plan,
            &[&actions, &parked, &heartbeat],
        )
    }

    pub(crate) fn curator_turn_settled(&self, tab_id: &str) {
        if !self.is_curator_tab(tab_id) {
            return;
        }
        let service = self.clone();
        let tab_id = tab_id.to_string();
        let _ = crate::async_runtime::spawn_blocking(move || {
            let Some(plan) = service.plan_store.list_full().into_iter().find(|p| {
                p.control_loop
                    .running
                    .as_ref()
                    .is_some_and(|r| r.tab_id.as_deref() == Some(&tab_id))
            }) else {
                return;
            };
            let tokens = service
                .kpi
                .ledger()
                .and_then(|l| l.output_tokens_for_tab(&tab_id).ok().flatten());
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let result = service.plan_store.with_plan(&plan.id, now, |plan| {
                let Some(run) = plan.control_loop.running.as_mut() else {
                    return Ok(None);
                };
                run.turns += 1;
                run.output_tokens = tokens;
                if run.turns < 24 && tokens.is_none_or(|n| n < 64_000) {
                    return Ok(None);
                }
                let round = CuratorRound {
                    number: run.number,
                    started_at: run.started_at,
                    ended_at: now,
                    outcome: "partial".into(),
                    reason: if run.turns >= 24 { "turns" } else { "tokens" }.into(),
                    summary: "Curator round reached its resource cap".into(),
                    next_delay_secs: curator_delay(None),
                    turns: run.turns,
                    output_tokens: tokens,
                    duration_ms: now.saturating_sub(run.started_at),
                    ..Default::default()
                };
                plan.control_loop.running = None;
                plan.control_loop.last_real_round_at = Some(now);
                curator::append_round(plan, round.clone());
                Ok(Some(round))
            });
            if let Ok(Some(round)) = result {
                service.journal_curator(&plan.id, &round);
                let _ = service.close_tab_with_reason(
                    &tab_id,
                    crate::multiplexer::types::TabCloseReason::AgentRunEnded,
                );
            }
        });
    }

    pub fn control_loop_show(&self, plan_id: &str) -> Result<ControlLoop, String> {
        let plan = self.plan_store.get(plan_id).ok_or("plan not found")?;
        let mut state = plan.control_loop.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        state.refresh_counts(now);
        state.suspended_reason = self.curator_suspended_reason(&plan);
        if state.suspended_reason.is_some() {
            state.next_at = None;
        }
        Ok(state)
    }

    pub fn control_loop_set(
        &self,
        plan_id: &str,
        caller: &AuthenticatedPlanCaller,
        update: ControlLoopUpdate,
        now: u64,
    ) -> Result<ControlLoop, String> {
        self.authorize_plan_orchestrator(plan_id, caller)?;
        self.plan_store
            .set_control_loop(plan_id, update, now)
            .map_err(|e| e.to_string())?;
        self.reconcile_curators(now);
        self.ui.emit("plan-changed", json!({"plan_id":plan_id}));
        self.control_loop_show(plan_id)
    }

    fn curator_suspended_reason(&self, plan: &Plan) -> Option<String> {
        let reason = if !self.configuration().orchestration.plan_reminders {
            Some("reminders_disabled")
        } else if plan.mode == PlanMode::Paused {
            Some("paused")
        } else if plan.mode == PlanMode::Complete
            || plan.status == PlanStatus::Archived
            || crate::service::plan::heartbeat::is_plan_complete(plan)
        {
            Some("complete")
        } else if self.curator_human_wait(plan) {
            Some("waiting_on_human")
        } else {
            None
        };
        reason.map(str::to_string)
    }

    fn curator_human_wait(&self, plan: &Plan) -> bool {
        let team_tabs: Vec<_> = self
            .mux
            .read()
            .tabs
            .values()
            .filter(|tab| tab.team_id == plan.team_id)
            .map(|tab| tab.id.clone())
            .collect();
        self.notif.lock().list(false).iter().any(|n| {
            !n.resolved
                && n.level == Some(codeterm_notification_core::contract::NotifyLevel::ActionNeeded)
                && n.tab_id.as_ref().is_some_and(|tab| team_tabs.contains(tab))
                && n.answer
                    .as_ref()
                    .is_none_or(|answer| answer.answered.is_none())
        })
    }

    pub fn curator_finish(
        &self,
        caller: &AuthenticatedPlanCaller,
        text: &str,
        next_secs: u64,
        now: u64,
    ) -> Result<CuratorRound, String> {
        let identity = self.curator_identity(caller)?;
        let note = note::parse(text, next_secs)?;
        let asked = self
            .plan_store
            .get(&identity.plan_id)
            .and_then(|plan| plan.control_loop.running)
            .is_some_and(|run| run.asked);
        if asked == (note.asked == "none") {
            return Err("ASKED must match this round's message record".into());
        }
        let round = self
            .plan_store
            .finish_curator_round(&identity.plan_id, identity.number, &note, now)
            .map_err(|e| e.to_string())?;
        self.journal_curator(&identity.plan_id, &round);
        if let Some(cron) = self.extensions.try_get::<crate::CronState>() {
            let run = cron
                .lock()
                .get_tab_owner(&identity.tab_id)
                .map(|(job, run)| (job.to_string(), run.to_string()));
            if let Some((job, run)) = run {
                let _ = job;
                self.run_watcher.request_cron_completion(
                    self,
                    &identity.tab_id,
                    &run,
                    &round.summary,
                )?;
            }
        }
        self.ui
            .emit("plan-changed", json!({"plan_id":identity.plan_id}));
        Ok(round)
    }

    pub(crate) fn journal_curator(&self, plan: &str, round: &CuratorRound) {
        let team = self.plan_store.get(plan).and_then(|p| p.team_id);
        let event = json!({"kind":"curator","label":round.outcome,"reason":"curator_round","plan_id":plan,"team_id":team,"time":round.ended_at,"action":"cron","round":round.number,"next_delay_secs":round.next_delay_secs,"learnings":round.learnings});
        if let Err(error) =
            crate::service::supporter::runtime_with_notifications(self).heartbeat_event(event)
        {
            log::debug!("[curator] journal unavailable: {error}");
        }
        log::debug!(
            "[curator] plan={plan} round={}",
            serde_json::to_string(round).unwrap_or_default()
        );
    }
}

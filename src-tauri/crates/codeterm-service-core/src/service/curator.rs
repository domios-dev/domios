use super::{AuthenticatedPlanCaller, ServiceContext};
use crate::cron::store::{commit, CronChange, CronCreateParams, CronJob, CronRunStatus, Prepared};
use crate::plan::{
    curator_delay, ControlLoop, CuratorGrant, CuratorRound, CuratorRun, Plan, PlanMode, PlanStatus,
};
use codeterm_plan::service_plan::curator::{self, digest, note, ControlLoopUpdate, DueAction};
use codeterm_types::plan::control_loop::{
    CuratorFailure, CuratorSuspension, CURATOR_ELEVATED_LANES, CURATOR_HIGH_LANES,
    CURATOR_INITIAL_WINDOW_MS, CURATOR_MAX_OUTPUT_TOKENS, CURATOR_MAX_TURNS, CURATOR_TIMEOUT_MS,
    CURATOR_TIMEOUT_SECS,
};
use serde_json::json;

mod ask;
mod authority;
mod save;
mod skill;

pub struct CuratorGrantScope {
    pub blueprints: Vec<String>,
    pub plan_skills: Vec<String>,
    pub saves: Vec<String>,
}
mod completion;
mod digest_sources;
mod events;
mod manual;
mod movement_sources;
pub(crate) mod policy;
mod reconciliation;
mod scheduling;
pub(crate) use reconciliation::CuratorReconciliations;
#[cfg(test)]
mod notification_tests;
#[cfg(test)]
mod tests;
pub use authority::CuratorIdentity;
pub(crate) use events::curator_bootstrap_ids;
pub(crate) use events::CuratorTabs;

fn curator_envelope() -> crate::service::messenger::MessageEnvelope {
    crate::service::messenger::MessageEnvelope::codeterm_reminder("curator")
}

fn curator_started_envelope(number: u64) -> crate::service::messenger::MessageEnvelope {
    crate::service::messenger::MessageEnvelope::codeterm_reminder(format!(
        "curator:round_started:{number}"
    ))
}

impl ServiceContext {
    pub fn curator_now(&self) -> Result<u64, String> {
        self.extensions
            .try_get::<crate::CronState>()
            .map(|clock| clock.lock().now_ms())
            .ok_or_else(|| "Curator clock is unavailable".into())
    }

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
        drop(crate::async_runtime::spawn_blocking(move || {
            let plan_id = service
                .extensions
                .try_get::<events::CuratorTabs>()
                .and_then(|index| index.0.read().get(&tab_id).cloned());
            let Some(plan) = plan_id.and_then(|id| service.plan_store.get(&id)) else {
                return;
            };
            let tokens = service
                .kpi
                .ledger()
                .and_then(|l| l.output_tokens_for_tab(&tab_id).ok().flatten());
            let now = match service.curator_now() {
                Ok(now) => now,
                Err(error) => {
                    log::debug!("[curator] settle {tab_id}: {error}");
                    return;
                }
            };
            let result = service.plan_store.with_plan(&plan.id, now, |plan| {
                let Some(run) = plan
                    .control_loop
                    .running
                    .as_mut()
                    .filter(|run| run.tab_id.as_deref() == Some(&tab_id))
                else {
                    return Ok(None);
                };
                run.turns += 1;
                run.output_tokens = tokens;
                if run.turns < CURATOR_MAX_TURNS
                    && tokens.is_none_or(|n| n < CURATOR_MAX_OUTPUT_TOKENS)
                {
                    return Ok(None);
                }
                let round = CuratorRound {
                    run_id: run.run_id.clone(),
                    tab_id: run.tab_id.clone(),
                    number: run.number,
                    started_at: run.started_at,
                    ended_at: now,
                    outcome: codeterm_types::plan::CuratorOutcome::Partial,
                    reason: if run.turns >= CURATOR_MAX_TURNS {
                        "turns"
                    } else {
                        "tokens"
                    }
                    .into(),
                    summary: "Curator round reached its resource cap".into(),
                    next_delay_secs: curator_delay(None),
                    turns: run.turns,
                    output_tokens: tokens,
                    save_id: run.save_id.clone(),
                    synthesis: run.synthesis,
                    digest_hash: run.digest_hash.clone(),
                    section_sizes: run.section_sizes.clone(),
                    duration_ms: now.saturating_sub(run.started_at),
                    ..Default::default()
                };
                plan.control_loop.running = None;
                plan.control_loop.last_real_round_at = Some(now);
                curator::append_round(plan, round.clone());
                Ok(Some(round))
            });
            match result {
                Ok(Some(round)) => {
                    service.journal_curator(&plan.id, &round);
                    if let Err(error) = service.close_tab_with_reason(
                        &tab_id,
                        crate::multiplexer::types::TabCloseReason::AgentRunEnded,
                    ) {
                        log::debug!("[curator] close capped round {tab_id}: {error}");
                    }
                }
                Ok(None) => {}
                Err(error) => log::debug!("[curator] settle {tab_id}: {error}"),
            }
        }));
    }

    pub fn control_loop_show(&self, plan_id: &str) -> Result<ControlLoop, String> {
        let now = self.curator_now()?;
        self.reconcile_curator_id(plan_id, now);
        let plan = self.plan_store.get(plan_id).ok_or("plan not found")?;
        let mut state = plan.control_loop.clone();
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
        self.reconcile_curator_id(plan_id, now);
        self.ui.emit("plan-changed", json!({"plan_id":plan_id}));
        self.control_loop_show(plan_id)
    }

    fn curator_suspended_reason(&self, plan: &Plan) -> Option<CuratorSuspension> {
        if !self.configuration().orchestration.plan_reminders {
            Some(CuratorSuspension::RemindersDisabled)
        } else if plan.mode == PlanMode::Paused {
            Some(CuratorSuspension::Paused)
        } else if plan.mode == PlanMode::Complete
            || plan.status == PlanStatus::Archived
            || crate::service::plan::heartbeat::is_plan_complete(plan)
        {
            Some(CuratorSuspension::Complete)
        } else if self.curator_human_wait(plan) {
            Some(CuratorSuspension::WaitingOnHuman)
        } else {
            None
        }
    }

    fn curator_human_wait(&self, plan: &Plan) -> bool {
        let team_tabs: Vec<_> = self
            .mux
            .read()
            .all_tabs()
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
        let identity = self.authenticate_curator(caller)?;
        let run = self
            .plan_store
            .get(&identity.plan_id)
            .and_then(|plan| plan.control_loop.running)
            .ok_or("Curator round is no longer active")?;
        let note = note::parse_for_round(text, next_secs, run.synthesis)?;
        if run.asked == (note.asked == codeterm_types::plan::CuratorAsked::None) {
            return Err("ASKED must match this round's message record".into());
        }
        let round = self
            .plan_store
            .finish_curator_round(&identity.plan_id, identity.number, &note, now)
            .map_err(|e| e.to_string())?;
        self.journal_curator(&identity.plan_id, &round);
        let mut result_delivery_error = None;
        if let Some(owner) = self
            .plan_store
            .get(&identity.plan_id)
            .and_then(|plan| plan.owner.tab_id)
        {
            let body = format!(
                "Curator round {} {}. MOVE: {}",
                round.number, round.outcome, round.summary
            );
            result_delivery_error = self
                .deliver_cron_report(&owner, curator_envelope(), &body)
                .err();
        }
        if let Some(cron) = self.extensions.try_get::<crate::CronState>() {
            let run = cron
                .lock()
                .get_tab_owner(&identity.tab_id)
                .map(|(job, run)| (job.to_string(), run.to_string()));
            if let Some((_job, run)) = run {
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
        if let Some(error) = result_delivery_error {
            return Err(format!(
                "round finalized; Orchestrator result delivery failed: {error}"
            ));
        }
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

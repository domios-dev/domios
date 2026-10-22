use super::*;

impl ServiceContext {
    pub(crate) fn settle_curator_run(
        &self,
        plan_id: &str,
        tab: &str,
        run_id: &str,
        now: u64,
    ) -> Result<CuratorRound, String> {
        let (round, recorded) = self
            .plan_store
            .with_plan(plan_id, now, |plan| {
                if let Some(round) = plan
                    .control_loop
                    .last_round
                    .as_ref()
                    .filter(|round| round.run_id.as_deref() == Some(run_id))
                {
                    return Ok((round.clone(), false));
                }
                let run = plan
                    .control_loop
                    .running
                    .as_ref()
                    .filter(|run| {
                        run.tab_id.as_deref() == Some(tab) && run.run_id.as_deref() == Some(run_id)
                    })
                    .ok_or_else(|| {
                        crate::service::plan::PlanError::Validation(
                            "Curator run identity changed".into(),
                        )
                    })?;
                let deadline = now.saturating_sub(run.started_at) >= CURATOR_TIMEOUT_MS;
                let round = CuratorRound {
                    run_id: run.run_id.clone(),
                    tab_id: run.tab_id.clone(),
                    number: run.number,
                    started_at: run.started_at,
                    ended_at: now,
                    outcome: if deadline {
                        codeterm_types::plan::CuratorOutcome::Partial
                    } else {
                        codeterm_types::plan::CuratorOutcome::Failed
                    },
                    reason: if deadline {
                        "deadline"
                    } else {
                        "run_ended_without_finish"
                    }
                    .into(),
                    summary: if deadline {
                        "Curator deadline reached"
                    } else {
                        "Curator execution ended without finish"
                    }
                    .into(),
                    next_delay_secs: curator_delay(None),
                    synthesis: run.synthesis,
                    digest_hash: run.digest_hash.clone(),
                    section_sizes: run.section_sizes.clone(),
                    turns: run.turns,
                    output_tokens: run.output_tokens,
                    save_id: run.save_id.clone(),
                    duration_ms: now.saturating_sub(run.started_at),
                    ..Default::default()
                };
                plan.control_loop.running = None;
                plan.control_loop.last_real_round_at = Some(now);
                plan.control_loop.consecutive_failures += 1;
                if plan.control_loop.consecutive_failures >= 2 {
                    plan.control_loop.fallback_reason = Some(CuratorFailure::ConsecutiveFailures);
                }
                curator::append_round(plan, round.clone());
                Ok((round, true))
            })
            .map_err(|error| error.to_string())?;
        if recorded {
            self.journal_curator(plan_id, &round);
            self.ui.emit("plan-changed", json!({"plan_id":plan_id}));
        }
        Ok(round)
    }
}

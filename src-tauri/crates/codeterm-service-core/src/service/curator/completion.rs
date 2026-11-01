use super::*;

#[derive(Clone, Copy)]
enum RunEnd {
    ExecutionEnded,
    TabClosed,
}

impl ServiceContext {
    pub(crate) fn settle_curator_run(
        &self,
        plan_id: &str,
        tab: &str,
        run_id: &str,
        now: u64,
    ) -> Result<CuratorRound, String> {
        self.settle_curator_run_end(plan_id, tab, run_id, now, RunEnd::ExecutionEnded)
    }

    pub(crate) fn settle_closed_curator_tab(&self, tab: &str) -> Result<(), String> {
        let Some(index) = self.try_ext::<super::CuratorTabs>() else {
            return Ok(());
        };
        let indexed = index.0.read().get(tab).cloned();
        let plan_id = indexed.or_else(|| {
            let cron = self.try_ext::<crate::CronState>()?.lock();
            let (job, _) = cron.get_tab_owner(tab)?;
            cron.get(job).map(|job| job.task.clone())
        });
        let Some(plan_id) = plan_id else {
            return Ok(());
        };
        let plan = self.plan_store.get(&plan_id).ok_or("Curator plan absent")?;
        let round = if let Some(run) = plan
            .control_loop
            .running
            .filter(|run| run.tab_id.as_deref() == Some(tab))
        {
            let run_id = run
                .run_id
                .as_deref()
                .ok_or("Curator closed without a run identity")?;
            self.settle_curator_run_end(
                &plan_id,
                tab,
                run_id,
                self.curator_now()?,
                RunEnd::TabClosed,
            )?
        } else if let Some(round) = plan
            .control_loop
            .last_round
            .filter(|round| round.tab_id.as_deref() == Some(tab))
        {
            round
        } else {
            index.0.write().remove(tab);
            return Ok(());
        };
        let run_id = round
            .run_id
            .as_deref()
            .ok_or("Curator closed without a run identity")?;
        if let Some(cron) = self.try_ext::<crate::CronState>() {
            let job = { cron.lock().get_tab_owner(tab).map(|(job, _)| job.clone()) };
            if let Some(job) = job {
                let status = if matches!(
                    round.outcome,
                    codeterm_types::plan::CuratorOutcome::Done
                        | codeterm_types::plan::CuratorOutcome::ModeChanged
                ) {
                    CronRunStatus::Success
                } else {
                    CronRunStatus::Failed
                };
                crate::cron::store::finish_run_with_summary(
                    cron,
                    &job,
                    run_id,
                    status,
                    &round.outcome.to_string(),
                    None,
                )
                .map_err(|error| error.to_string())?;
                cron.lock().unregister_tab(tab);
                self.emit_cron_changed();
            }
        }
        let pty = self.agent_run_pty_map.lock().get(tab).cloned();
        if let Some(pty) = pty {
            self.run_watcher.mark_completed_externally(&pty);
        }
        index.0.write().remove(tab);
        Ok(())
    }

    fn settle_curator_run_end(
        &self,
        plan_id: &str,
        tab: &str,
        run_id: &str,
        now: u64,
        end: RunEnd,
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
                let closed = matches!(end, RunEnd::TabClosed);
                let deadline = !closed && now.saturating_sub(run.started_at) >= CURATOR_TIMEOUT_MS;
                let round = CuratorRound {
                    run_id: run.run_id.clone(),
                    tab_id: run.tab_id.clone(),
                    number: run.number,
                    started_at: run.started_at,
                    ended_at: now,
                    outcome: if closed || deadline {
                        codeterm_types::plan::CuratorOutcome::Partial
                    } else {
                        codeterm_types::plan::CuratorOutcome::Failed
                    },
                    reason: if closed {
                        "tab_closed"
                    } else if deadline {
                        "deadline"
                    } else {
                        "run_ended_without_finish"
                    }
                    .into(),
                    summary: if closed {
                        "Curator tab closed without finish"
                    } else if deadline {
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
                if !closed {
                    plan.control_loop.consecutive_failures += 1;
                    if plan.control_loop.consecutive_failures >= 2 {
                        plan.control_loop.fallback_reason =
                            Some(CuratorFailure::ConsecutiveFailures);
                    }
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

#[cfg(test)]
mod tests;

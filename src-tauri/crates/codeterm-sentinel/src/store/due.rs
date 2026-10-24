use super::*;

pub fn scheduled_stop_notices(jobs: &[Sentinel], changes: &[ScheduleChange]) -> Vec<StopNotice> {
    changes
        .iter()
        .filter_map(|change| match change {
            ScheduleChange::Stop { id, reason } => jobs
                .iter()
                .find(|job| job.id == *id)
                .map(|job| StopNotice::new(job, reason)),
            ScheduleChange::Advance { .. } => None,
        })
        .collect()
}

/// Expiry and budget stops, then due leases; registers each lease's ephemeral cancellation.
pub fn plan_due(
    jobs: &mut [Sentinel],
    active: &mut HashMap<String, Arc<Cancellation>>,
    wake_pending: &mut BTreeSet<String>,
    now: u64,
) -> (Vec<Lease>, Vec<ScheduleChange>, Vec<String>) {
    let mut due = Vec::new();
    let mut changes = Vec::new();
    let mut consumed_wakes = Vec::new();
    let session_spent: BTreeSet<_> = jobs
        .iter()
        .filter(|job| job.session)
        .filter(|job| {
            let usage = super::super::session::usage(jobs, job, now);
            usage.exchanges >= codeterm_blueprint_types::blueprint_terminal::SESSION_SAFETY_LIMIT
                || usage.actions
                    >= codeterm_blueprint_types::blueprint_terminal::SESSION_SAFETY_LIMIT
        })
        .map(|job| job.id.clone())
        .collect();
    for job in jobs.iter_mut() {
        if job.machine.stopped || !job.admission.permits_steps() {
            continue;
        }
        if job.expires_at.is_some_and(|at| now >= at) {
            wake_pending.remove(&job.id);
            job.machine.stopped = true;
            job.cleanup.sync(true, now);
            job.revision += 1;
            job.last_reason = Some("timeout".into());
            if let Some(cancelled) = active.get(&job.id) {
                cancelled.store(true, Ordering::SeqCst);
            }
            changes.push(ScheduleChange::Stop {
                id: job.id.clone(),
                reason: "timeout",
            });
            continue;
        }
        if job.machine.held.is_some() {
            continue;
        }
        let transitions_spent = transition_budget_spent(job) || session_spent.contains(&job.id);
        if !active.contains_key(&job.id)
            && (transitions_spent
                || job
                    .transitions
                    .iter()
                    .flat_map(|t| &t.handlers)
                    .filter(|r| r.claim_id.is_some())
                    .count()
                    .saturating_sub(job.cleanup.action_offset)
                    >= action_limit(job))
        {
            let reason = if job.session {
                "session_safety_limit"
            } else if transitions_spent {
                "transition_budget_exhausted"
            } else {
                "action_budget_exhausted"
            };
            job.machine.stopped = true;
            job.cleanup.sync(true, now);
            wake_pending.remove(&job.id);
            job.revision += 1;
            job.last_reason = Some(reason.into());
            changes.push(ScheduleChange::Stop {
                id: job.id.clone(),
                reason,
            });
            continue;
        }
        if job.machine.paused {
            continue;
        }
        if matches!(job.observe(), Some(TabBinding::CronRun(_))) && job.run.is_none() {
            continue;
        }
        if job.admission.continuation.is_none()
            && job.pipeline().is_some_and(|pipeline| {
                matches!(
                    &pipeline.trigger,
                    codeterm_blueprint_types::blueprint_pipeline::Trigger::Event { .. }
                )
            })
        {
            continue;
        }
        let input_trigger = job.pipeline().is_some_and(|pipeline| {
            matches!(
                &pipeline.trigger,
                codeterm_blueprint_types::blueprint_pipeline::Trigger::Input { .. }
            )
        });
        let text_event = job.terminal().is_some() || job.input().is_some();
        let due_now = if job.admission.continuation.is_some() || job.admission.baseline_pending {
            true
        } else if text_event {
            wake_pending.contains(&job.id)
                || (job.input().is_some()
                    && job.next_run
                        != codeterm_blueprint_types::sentinel_input::INPUT_DORMANT_DEADLINE_MS
                    && job.next_run <= now)
        } else if input_trigger {
            codeterm_blueprint_types::sentinel_input::input_due(job.next_run, now)
        } else {
            job.next_run <= now || wake_pending.contains(&job.id)
        };
        if due_now && !active.contains_key(&job.id) {
            if text_event || input_trigger {
                job.next_run = codeterm_blueprint_types::sentinel_input::INPUT_DORMANT_DEADLINE_MS;
            } else if job.next_run <= now {
                let missed = (now - job.next_run) / job.interval_ms;
                job.next_run = job
                    .next_run
                    .saturating_add((missed + 1).saturating_mul(job.interval_ms));
            } else {
                job.next_run = now.saturating_add(job.interval_ms);
            }
            if wake_pending.remove(&job.id) {
                consumed_wakes.push(job.id.clone());
            }
            let cancelled = Arc::new(Cancellation::new(false));
            active.insert(job.id.clone(), cancelled.clone());
            due.push(Lease {
                job: job.clone(),
                cancelled,
                event: None,
            });
            changes.push(ScheduleChange::Advance {
                id: job.id.clone(),
                next_run: job.next_run,
            });
        }
    }
    (due, changes, consumed_wakes)
}

pub fn transition_budget_spent(job: &Sentinel) -> bool {
    if job.session {
        return job.transitions.len()
            >= codeterm_blueprint_types::blueprint_terminal::SESSION_SAFETY_LIMIT;
    }
    if job.transitions.len() >= MAX_LIFETIME_TRANSITIONS {
        return true;
    }
    if matches!(job.observe(), Some(TabBinding::CronRun(_))) {
        job.run.is_some() && job.run_transitions >= job.max_transitions
    } else {
        job.transitions
            .len()
            .saturating_sub(job.cleanup.transition_offset)
            >= usize::from(job.max_transitions)
    }
}

#[cfg(test)]
mod tests;

pub fn action_limit(job: &Sentinel) -> usize {
    if job.session {
        codeterm_blueprint_types::blueprint_terminal::SESSION_SAFETY_LIMIT
    } else {
        usize::from(job.max_actions)
    }
}

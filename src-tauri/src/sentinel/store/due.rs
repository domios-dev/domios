use super::*;

pub(super) fn scheduled_stop_notices(
    jobs: &[Sentinel],
    changes: &[ScheduleChange],
) -> Vec<StopNotice> {
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
pub(super) fn plan_due(
    jobs: &mut [Sentinel],
    active: &mut HashMap<String, Arc<Cancellation>>,
    wake_pending: &mut BTreeSet<String>,
    now: u64,
) -> (Vec<Lease>, Vec<ScheduleChange>, Vec<String>) {
    let mut due = Vec::new();
    let mut changes = Vec::new();
    let mut consumed_wakes = Vec::new();
    for job in jobs.iter_mut() {
        if job.machine.stopped {
            continue;
        }
        if job.expires_at.is_some_and(|at| now >= at) {
            wake_pending.remove(&job.id);
            job.machine.stopped = true;
            job.revision += 1;
            job.last_reason = Some("timeout".into());
            if let Some(cancelled) = active.remove(&job.id) {
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
        let transitions_spent = transition_budget_spent(job);
        if !active.contains_key(&job.id)
            && (transitions_spent
                || job
                    .transitions
                    .iter()
                    .flat_map(|t| &t.handlers)
                    .filter(|r| r.claim_id.is_some())
                    .count()
                    >= usize::from(job.max_actions))
        {
            let reason = if transitions_spent {
                "transition_budget_exhausted"
            } else {
                "action_budget_exhausted"
            };
            job.machine.stopped = true;
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
        if matches!(job.observe.as_ref(), Some(TabBinding::CronRun(_))) && job.run.is_none() {
            continue;
        }
        if job.pipeline.as_ref().is_some_and(|pipeline| {
            matches!(
                &pipeline.trigger,
                codeterm_types::blueprint_pipeline::Trigger::Event { .. }
            )
        }) {
            continue;
        }
        if (job.next_run <= now || wake_pending.contains(&job.id)) && !active.contains_key(&job.id)
        {
            if job.next_run <= now {
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

pub(in crate::sentinel) fn transition_budget_spent(job: &Sentinel) -> bool {
    if job.transitions.len() >= MAX_LIFETIME_TRANSITIONS {
        return true;
    }
    if matches!(job.observe.as_ref(), Some(TabBinding::CronRun(_))) {
        job.run.is_some() && job.run_transitions >= job.max_transitions
    } else {
        job.transitions.len() >= usize::from(job.max_transitions)
    }
}

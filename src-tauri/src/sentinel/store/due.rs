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
    let session_spent: BTreeSet<_> = jobs
        .iter()
        .filter(|job| job.session)
        .filter(|job| {
            let usage = super::super::session::usage(jobs, job, now);
            usage.exchanges >= codeterm_types::blueprint_terminal::SESSION_SAFETY_LIMIT
                || usage.actions >= codeterm_types::blueprint_terminal::SESSION_SAFETY_LIMIT
        })
        .map(|job| job.id.clone())
        .collect();
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
        let transitions_spent = transition_budget_spent(job) || session_spent.contains(&job.id);
        if !active.contains_key(&job.id)
            && (transitions_spent
                || job
                    .transitions
                    .iter()
                    .flat_map(|t| &t.handlers)
                    .filter(|r| r.claim_id.is_some())
                    .count()
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
        if job.pipeline().is_some_and(|pipeline| {
            matches!(
                &pipeline.trigger,
                codeterm_types::blueprint_pipeline::Trigger::Event { .. }
            )
        }) {
            continue;
        }
        let input_trigger = job.pipeline().is_some_and(|pipeline| {
            matches!(
                &pipeline.trigger,
                codeterm_types::blueprint_pipeline::Trigger::Input { .. }
            )
        });
        let text_event = job.terminal().is_some() || job.input().is_some();
        let due_now = if text_event {
            wake_pending.contains(&job.id)
                || (job.input().is_some()
                    && job.next_run != codeterm_types::sentinel_input::INPUT_DORMANT_DEADLINE_MS
                    && job.next_run <= now)
        } else if input_trigger {
            codeterm_types::sentinel_input::input_due(job.next_run, now)
        } else {
            job.next_run <= now || wake_pending.contains(&job.id)
        };
        if due_now && !active.contains_key(&job.id) {
            if text_event || input_trigger {
                job.next_run = codeterm_types::sentinel_input::INPUT_DORMANT_DEADLINE_MS;
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

pub(in crate::sentinel) fn transition_budget_spent(job: &Sentinel) -> bool {
    if job.session {
        return job.transitions.len() >= codeterm_types::blueprint_terminal::SESSION_SAFETY_LIMIT;
    }
    if job.transitions.len() >= MAX_LIFETIME_TRANSITIONS {
        return true;
    }
    if matches!(job.observe(), Some(TabBinding::CronRun(_))) {
        job.run.is_some() && job.run_transitions >= job.max_transitions
    } else {
        job.transitions.len() >= usize::from(job.max_transitions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_types::blueprint_pipeline::{
        BlueprintResult, BlueprintSpec, IoType, Step, StepKind, Trigger,
    };
    use codeterm_types::sentinel_input::{DataRef, MediaKind};

    #[test]
    fn blueprint_input_due_is_once_per_activation_and_amendment_without_interval_runs() {
        let temp = tempfile::tempdir().unwrap();
        let clock = crate::cron::clock::MockClock::starting_at(1_000);
        let store = Store::new(temp.path().join("input.json"), clock);
        let mut job = store
            .prepare_pipeline(
                BlueprintSpec {
                    name: "one-shot-input".into(),
                    owner: codeterm_types::blueprint_pipeline::BlueprintOwner::Tab,
                    trigger: Trigger::Input {
                        kind: MediaKind::Text,
                        source: DataRef::Text("hello".into()),
                    },
                    steps: vec![Step {
                        name: "filter".into(),
                        input: IoType::Text,
                        kind: StepKind::Filter {
                            pattern: "hello".into(),
                            bucket: None,
                        },
                        on: Default::default(),
                    }],
                    result: BlueprintResult::default(),
                },
                Owner {
                    creator_tab_id: "creator".into(),
                    team_id: None,
                    workspace_id: "workspace".into(),
                    cwd: "/tmp".into(),
                },
                true,
            )
            .unwrap();
        job.next_run = 1_000;
        let id = job.id.clone();
        let mut jobs = vec![job];
        let mut active = HashMap::new();
        let mut wake_pending = BTreeSet::new();

        assert_eq!(
            plan_due(&mut jobs, &mut active, &mut wake_pending, 1_000)
                .0
                .len(),
            1
        );
        active.remove(&id);
        assert!(plan_due(&mut jobs, &mut active, &mut wake_pending, 21_000)
            .0
            .is_empty());

        jobs[0].next_run = 21_000;
        assert_eq!(
            plan_due(&mut jobs, &mut active, &mut wake_pending, 21_000)
                .0
                .len(),
            1
        );
        active.remove(&id);
        assert!(plan_due(&mut jobs, &mut active, &mut wake_pending, 41_000)
            .0
            .is_empty());
    }
}

pub(in crate::sentinel) fn action_limit(job: &Sentinel) -> usize {
    if job.session {
        codeterm_types::blueprint_terminal::SESSION_SAFETY_LIMIT
    } else {
        usize::from(job.max_actions)
    }
}

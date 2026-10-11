use super::*;

pub(crate) fn advance(
    store: &Arc<Mutex<Store>>,
    lease: &Lease,
    output: &probe::Output,
    classify: impl FnOnce() -> super::super::provenance::Classification,
    mut claim: impl FnMut(
        &Sentinel,
        &str,
        &super::super::handlers::Handler,
    ) -> Result<Option<super::super::handlers::ResultRecord>, String>,
    mut execute: impl FnMut(
        &Sentinel,
        &str,
        &super::super::handlers::Handler,
        &machine::Verdict,
    ) -> Result<super::super::handlers::Effect, String>,
    mut write_journal: impl FnMut(&Sentinel, &machine::Outcome, Stage) -> std::io::Result<()>,
) {
    use super::super::handlers::Status;
    if lease.cancelled.load(Ordering::SeqCst) {
        return;
    }
    let current = store.lock().get(&lease.job.id).cloned();
    if !current.is_some_and(|job| {
        job.same_owner(&lease.job)
            && job.revision == lease.job.revision
            && job.machine.last_hash == lease.job.machine.last_hash
            && job.machine.last_bucket == lease.job.machine.last_bucket
            && !job.machine.stopped
            && !job.machine.paused
    }) {
        return;
    }
    let mut job = lease.job.clone();
    if job.terminal().is_some() || job.input().is_some() {
        job.machine.last_hash = None;
        job.machine.last_bucket = None;
    }
    let scope = Scope::from(&job);
    let outcome = super::super::progress::evaluate_probe(
        &mut job.machine,
        output,
        scope,
        |machine, scope| {
            machine::evaluate_with_other_rearm(
                machine,
                &output.bytes,
                &job.buckets,
                &job.routes,
                &job.rearm_on_change,
                &job.on_other,
                job.until.as_deref(),
                scope,
                classify,
            )
        },
    );
    let now = store.lock().clock.now_ms();
    job.progress.record_outcome(now, &outcome);
    job.last_reason = Some(outcome.reason.into());
    let transition_bucket = match outcome.reason {
        "transition" | "rearm_on_change" => outcome
            .verdict
            .as_ref()
            .map(|verdict| verdict.bucket.as_str()),
        "other" | "low_confidence" if !outcome.handlers.is_empty() => Some("other"),
        _ => None,
    };
    let committed = if let Some(bucket) = transition_bucket {
        storage::commit(store, |current| {
            current.stage_accept_decision(&job, bucket, &outcome.evidence)
        })
    } else {
        storage::commit(store, |current| current.stage_finish(&job))
    };
    if !matches!(committed, Ok(true)) {
        return;
    }
    if write_journal(&job, &outcome, Stage::Decision).is_err() {
        let _ = storage::commit(store, |current| {
            current.stage_skip_remaining(
                &job.id,
                &outcome.evidence.evaluation_id,
                &outcome.handlers,
            )
        });
        return;
    }
    for (index, handler) in outcome.handlers.iter().enumerate() {
        let transition = &outcome.evidence.evaluation_id;
        let permitted = !lease.cancelled.load(Ordering::SeqCst)
            && store.lock().action_permitted(&job, transition);
        if !permitted {
            let _ = storage::commit(store, |current| {
                current.stage_skip_remaining(&job.id, transition, &outcome.handlers[index..])
            });
            return;
        }
        let accepted = match claim(&job, transition, handler) {
            Ok(Some(claim)) => claim,
            Err(error) if error == super::super::handlers::ESCALATION_RATE_LIMITED => {
                let mut limited = outcome.clone();
                limited.reason = "escalation_rate_limited";
                limited.route = Some(handler.action.clone());
                let _ = write_journal(&job, &limited, Stage::Skip);
                if storage::commit(store, |current| {
                    current.stage_skip_remaining(&job.id, transition, std::slice::from_ref(handler))
                })
                .is_err()
                {
                    return;
                }
                continue;
            }
            _ => {
                let _ = storage::commit(store, |current| {
                    current.stage_skip_remaining(&job.id, transition, &outcome.handlers[index..])
                });
                return;
            }
        };
        let mut handler_outcome = outcome.clone();
        handler_outcome.route = Some(handler.action.clone());
        if let Err(error) = write_journal(&job, &handler_outcome, Stage::RouteClaim) {
            if !journal_refusal_prevented_effect(&error) {
                log::error!("[blueprint] route claim journal outcome is uncertain: {error}");
                let uncertain = super::super::handlers::Effect {
                    status: Status::Unconfirmed,
                    execution_id: None,
                    command_result: None,
                    reason: Some("route_claim_journal_unconfirmed".into()),
                };
                if let Err(persist_error) = storage::commit(store, |current| {
                    current.stage_finish_handler_result(&job.id, transition, &accepted, uncertain)
                }) {
                    log::error!(
                        "[blueprint] uncertain route claim persistence failed: {persist_error}"
                    );
                }
                return;
            }
            let failure = super::super::handlers::Effect {
                status: Status::Failed,
                execution_id: None,
                command_result: None,
                reason: Some("journal_route_claim_failed".into()),
            };
            if let Err(error) = storage::commit(store, |current| {
                current.stage_finish_handler_result(&job.id, transition, &accepted, failure)
            }) {
                log::error!("[blueprint] failed route claim result persistence failed: {error}");
            }
            let _ = write_journal(&job, &handler_outcome, Stage::RouteFailed);
            let _ = storage::commit(store, |current| {
                current.stage_skip_remaining(&job.id, transition, &outcome.handlers[index + 1..])
            });
            return;
        }
        if lease.cancelled.load(Ordering::SeqCst)
            || !store.lock().action_permitted(&job, transition)
        {
            let failure = super::super::handlers::Effect {
                status: Status::Unconfirmed,
                execution_id: None,
                command_result: None,
                reason: Some("route_claim_fenced".into()),
            };
            let _ = storage::commit(store, |current| {
                current.stage_finish_handler_result(&job.id, transition, &accepted, failure)
            });
            let _ = write_journal(&job, &handler_outcome, Stage::RouteFailed);
            let _ = storage::commit(store, |current| {
                current.stage_skip_remaining(&job.id, transition, &outcome.handlers[index + 1..])
            });
            return;
        }
        let Some(verdict) = outcome.verdict.as_ref() else {
            let failure = super::super::handlers::Effect {
                status: Status::Failed,
                execution_id: None,
                command_result: None,
                reason: Some("route_verdict_missing".into()),
            };
            let _ = storage::commit(store, |current| {
                current.stage_finish_handler_result(&job.id, transition, &accepted, failure)
            });
            let _ = write_journal(&job, &handler_outcome, Stage::RouteFailed);
            return;
        };
        let result = execute(&job, transition, handler, verdict);
        let stage = if matches!(
            &result,
            Ok(super::super::handlers::Effect {
                status: Status::Completed | Status::Noop,
                ..
            })
        ) {
            Stage::RouteCompleted
        } else {
            Stage::RouteFailed
        };
        let effect = result.unwrap_or_else(|_| super::super::handlers::Effect {
            status: Status::Failed,
            execution_id: None,
            command_result: None,
            reason: Some("handler_execution_failed".into()),
        });
        let unsafe_continuation = effect
            .command_result
            .as_ref()
            .is_some_and(|result| !result.resources_confirmed);
        if let Err(error) = storage::commit(store, |current| {
            current.stage_finish_handler_result(&job.id, transition, &accepted, effect)
        }) {
            log::error!("[blueprint] handler result persistence failed: {error}");
            return;
        }
        let _ = write_journal(&job, &handler_outcome, stage);
        if unsafe_continuation {
            let _ = storage::commit(store, |current| {
                current.stage_stop(&job.id, "command_resources_unconfirmed")
            });
            let _ = storage::commit(store, |current| {
                current.stage_skip_remaining(&job.id, transition, &outcome.handlers[index + 1..])
            });
            return;
        }
    }
}

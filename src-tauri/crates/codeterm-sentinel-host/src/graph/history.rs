use crate::{handlers::Transition, store::Sentinel};

pub(super) const MAX_TRANSITIONS: usize = 64;

pub(super) fn project(
    job: &Sentinel,
    remaining: &mut usize,
) -> Vec<codeterm_sentinel::graph_contracts::Transition> {
    use codeterm_sentinel::graph_contracts::{Transition as GraphTransition, TransitionHandler};
    job.transitions
        .iter()
        .rev()
        .take(MAX_TRANSITIONS)
        .take_while(|transition| {
            let cost = 1 + transition.handlers.len().min(64);
            if cost > *remaining {
                return false;
            }
            *remaining -= cost;
            true
        })
        .map(|transition| GraphTransition {
            causal_run: transition.causal_run.clone(),
            id: transition.id.clone(),
            bucket: transition.bucket.clone(),
            accepted_at: transition.accepted_at,
            routes_current: job
                .amendments
                .iter()
                .all(|amendment| amendment.at < transition.accepted_at),
            pending_handlers: pending_handlers(job, transition),
            handlers: transition
                .handlers
                .iter()
                .take(64)
                .map(|handler| TransitionHandler {
                    handler_id: handler.handler_id.clone(),
                    result: super::record(handler),
                })
                .collect(),
            decision: super::detail::decision_projection(transition.decision.as_ref()),
        })
        .collect()
}

fn pending_handlers(job: &Sentinel, transition: &Transition) -> bool {
    if job
        .amendments
        .iter()
        .any(|amendment| amendment.at >= transition.accepted_at)
    {
        return false;
    }
    let declared = job
        .routes
        .get(&transition.bucket)
        .map(Vec::as_slice)
        .unwrap_or_else(|| {
            if transition.bucket == codeterm_service_core::storage::OTHER_BUCKET {
                &job.on_other
            } else {
                &[]
            }
        });
    declared.iter().any(|handler| {
        !transition
            .handlers
            .iter()
            .any(|result| result.handler_id == handler.id)
    })
}

#[cfg(test)]
mod tests;

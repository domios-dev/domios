use std::collections::{BTreeMap, BTreeSet};

use super::super::store::Sentinel;

#[cfg(test)]
mod tests;

pub(super) fn project(
    nodes: &[codeterm_sentinel::graph_contracts::Node],
    jobs: &[Sentinel],
    now: u64,
    live: &BTreeSet<String>,
) -> Vec<codeterm_sentinel::graph_contracts::Run> {
    use codeterm_sentinel::graph_contracts::{Run, RunState, RunTransition};
    let mut groups: BTreeMap<String, Run> = BTreeMap::new();
    for node in nodes {
        let Some(history) = &node.history else {
            continue;
        };
        for transition in history.iter().rev() {
            let Some(context) = &transition.causal_run else {
                continue;
            };
            let group = groups.entry(context.id.clone()).or_insert_with(|| Run {
                id: context.id.clone(),
                root_node: context.root_node.clone(),
                root_transition: context.root_transition.clone(),
                started_at: context.started_at,
                state: RunState::Completed,
                incomplete: false,
                transitions: Vec::new(),
            });
            group.transitions.push(RunTransition {
                node: node.identity.clone(),
                transition_id: transition.id.clone(),
                parent: context.parent.clone(),
                conversation: node.conversation.clone(),
            });
            if transition.pending_handlers
                || transition
                    .handlers
                    .iter()
                    .any(|handler| handler.result.status.as_deref() == Some("unconfirmed"))
            {
                if live.contains(&node.identity.id) {
                    group.state = RunState::Running;
                } else if group.state != RunState::Running {
                    group.state = RunState::Unconfirmed;
                }
            }
        }
    }
    let waiting: BTreeSet<_> = jobs
        .iter()
        .filter(|job| {
            let identity = super::identity(job);
            nodes.iter().any(|node| node.identity == identity)
                && !job.machine.stopped
                && !job.machine.paused
                && job.admission.permits_steps()
                && job.expires_at.is_none_or(|at| at > now)
        })
        .filter_map(|job| job.causal_run.as_ref().map(|run| run.id.as_str()))
        .collect();
    for group in groups.values_mut() {
        let root_retained = group.transitions.iter().any(|entry| {
            entry.node.id == group.root_node && entry.transition_id == group.root_transition
        });
        let parents_retained = group.transitions.iter().all(|entry| {
            entry.parent.as_ref().is_none_or(|parent| {
                group.transitions.iter().any(|candidate| {
                    candidate.node.id == parent.node && candidate.transition_id == parent.transition
                })
            })
        });
        group.incomplete = !root_retained || !parents_retained;
        if waiting.contains(group.id.as_str()) {
            group.state = RunState::Running;
        }
    }
    let mut runs: Vec<_> = groups.into_values().collect();
    runs.sort_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    runs
}

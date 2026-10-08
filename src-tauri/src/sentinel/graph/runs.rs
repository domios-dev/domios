use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::super::store::Sentinel;

#[cfg(test)]
mod tests;

pub(super) fn project(
    nodes: &[Value],
    jobs: &[Sentinel],
    now: u64,
    live: &BTreeSet<String>,
) -> Vec<Value> {
    let mut groups: BTreeMap<String, Value> = BTreeMap::new();
    for node in nodes {
        let Some(history) = node["history"].as_array() else {
            continue;
        };
        for transition in history.iter().rev() {
            let Some(id) = transition["causal_run"]["id"].as_str() else {
                continue;
            };
            let context = &transition["causal_run"];
            let group = groups.entry(id.into()).or_insert_with(|| {
                json!({"id":id,"root_node":context["root_node"],
                    "root_transition":context["root_transition"],"started_at":context["started_at"],
                    "state":"completed","incomplete":false,"transitions":[]})
            });
            if let Some(entries) = group["transitions"].as_array_mut() {
                entries.push(json!({
                "node":node["identity"],"transition_id":transition["id"],"parent":context["parent"],"conversation":node["conversation"]
            }));
            }
            if transition["pending_handlers"] == true
                || transition["handlers"].as_array().is_some_and(|handlers| {
                    handlers
                        .iter()
                        .any(|handler| handler["status"] == "unconfirmed")
                })
            {
                if node["identity"]["id"]
                    .as_str()
                    .is_some_and(|id| live.contains(id))
                {
                    group["state"] = json!("running");
                } else if group["state"] != "running" {
                    group["state"] = json!("unconfirmed");
                }
            }
        }
    }
    let waiting: BTreeSet<_> = jobs
        .iter()
        .filter(|job| {
            let identity = json!(super::identity(job));
            nodes.iter().any(|node| node["identity"] == identity)
                && !job.machine.stopped
                && !job.machine.paused
                && job.expires_at.is_none_or(|at| at > now)
        })
        .filter_map(|job| job.causal_run.as_ref().map(|run| run.id.as_str()))
        .collect();
    for group in groups.values_mut() {
        let Some(transitions) = group["transitions"].as_array() else {
            continue;
        };
        let root_retained = transitions.iter().any(|entry| {
            entry["node"]["id"] == group["root_node"]
                && entry["transition_id"] == group["root_transition"]
        });
        let parents_retained = transitions.iter().all(|entry| {
            entry["parent"].is_null()
                || transitions.iter().any(|candidate| {
                    candidate["node"]["id"] == entry["parent"]["node"]
                        && candidate["transition_id"] == entry["parent"]["transition"]
                })
        });
        group["incomplete"] = json!(!root_retained || !parents_retained);
        if group["id"].as_str().is_some_and(|id| waiting.contains(id)) {
            group["state"] = json!("running");
        }
    }
    let mut runs: Vec<_> = groups.into_values().collect();
    runs.sort_by(|a, b| {
        b["started_at"]
            .as_u64()
            .cmp(&a["started_at"].as_u64())
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    runs
}

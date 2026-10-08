use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::super::store::Sentinel;

#[cfg(test)]
mod tests;

pub(super) fn project(nodes: &[Value], jobs: &[Sentinel], now: u64) -> Vec<Value> {
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
                "node":node["identity"],"transition_id":transition["id"],"parent":context["parent"]
            }));
            }
            if transition["pending_handlers"] == true
                || transition["handlers"].as_array().is_some_and(|handlers| {
                    handlers
                        .iter()
                        .any(|handler| handler["status"] == "unconfirmed")
                })
            {
                group["state"] = json!("running");
            }
        }
    }
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
        if jobs.iter().any(|job| {
            nodes
                .iter()
                .any(|node| node["identity"] == json!(super::identity(job)))
                && job
                    .causal_run
                    .as_ref()
                    .is_some_and(|run| group["id"] == run.id)
                && !job.machine.stopped
                && !job.machine.paused
                && job.expires_at.is_none_or(|at| at > now)
        }) {
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

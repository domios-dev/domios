use serde_json::{json, Value};

use crate::sentinel::store::Sentinel;

pub(super) const MAX_TRANSITIONS: usize = 64;

pub(super) fn project(job: &Sentinel, remaining: &mut usize) -> Vec<Value> {
    job.transitions.iter().rev().take(MAX_TRANSITIONS).take_while(|transition| {
        let cost = 1 + transition.handlers.len().min(64);
        if cost > *remaining { return false; }
        *remaining -= cost;
        true
    }).map(|transition| {
        json!({"id":transition.id,"bucket":transition.bucket,"accepted_at":transition.accepted_at,"routes_current":job.amendments.iter().all(|amendment| amendment.at < transition.accepted_at),
            "handlers":transition.handlers.iter().take(64).map(|handler| {
                let mut result = super::record(handler);
                result["handler_id"] = json!(handler.handler_id);
                result
            }).collect::<Vec<_>>()})
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sentinel::handlers::Transition;

    #[test]
    fn retained_history_is_bounded_newest_first_without_private_output() {
        let mut job: Sentinel = serde_json::from_value(json!({
            "id":"node","workspace_id":"workspace","team_id":null,"creator_tab_id":"creator",
            "cwd":"private-directory","probe":"private-code","question":"Ready?","buckets":["ready"],
            "routes":{},"matches":{},"interval_ms":10000,"next_run":0,"expires_at":null,
            "until":null,"last_reason":null,
            "machine":{"paused":false,"stopped":false,"last_hash":null,"last_bucket":null}
        })).unwrap();
        job.transitions = (0..70)
            .map(|index| Transition {
                id: format!("event-{index}"),
                bucket: "ready".into(),
                accepted_at: index,
                terminal: false,
                handlers: vec![],
                decision: None,
            })
            .collect();
        let history = project(&job, &mut 4096);
        assert_eq!(history.len(), MAX_TRANSITIONS);
        assert_eq!(history[0]["id"], "event-69");
        assert_eq!(history[63]["id"], "event-6");
        assert_eq!(project(&job, &mut 2).len(), 2);
        assert!(!json!(history).to_string().contains("private-"));
    }
}

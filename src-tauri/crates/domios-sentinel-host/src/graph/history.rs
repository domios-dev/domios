use serde_json::{json, Value};

use crate::{handlers::Transition, store::Sentinel};

pub(super) const MAX_TRANSITIONS: usize = 64;

pub(super) fn project(job: &Sentinel, remaining: &mut usize) -> Vec<Value> {
    job.transitions.iter().rev().take(MAX_TRANSITIONS).take_while(|transition| {
        let cost = 1 + transition.handlers.len().min(64);
        if cost > *remaining { return false; }
        *remaining -= cost;
        true
    }).map(|transition| {
        let mut result = json!({"causal_run":transition.causal_run,"id":transition.id,"bucket":transition.bucket,"accepted_at":transition.accepted_at,"routes_current":job.amendments.iter().all(|amendment| amendment.at < transition.accepted_at),
            "pending_handlers":pending_handlers(job, transition),
            "handlers":transition.handlers.iter().take(64).map(|handler| {
                let mut result = super::record(handler);
                result["handler_id"] = json!(handler.handler_id);
                result
            }).collect::<Vec<_>>()});
        super::detail::project_decision(&mut result, transition.decision.as_ref());
        result
    }).collect()
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
mod tests {
    use super::*;
    use crate::handlers::Transition;

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
                causal_run: None,
                id: format!("event-{index}"),
                bucket: "ready".into(),
                accepted_at: index,
                terminal: false,
                handlers: vec![],
                decision: Some(crate::provenance::Decision {
                    decided_by: crate::provenance::DecidedBy::Adapter,
                    adapter_id: Some("fixture-adapter".into()),
                    model: Some("fixture-model".into()),
                    confidence: Some(0.87),
                }),
            })
            .collect();
        let history = project(&job, &mut 4096);
        assert_eq!(history.len(), MAX_TRANSITIONS);
        assert_eq!(history[0]["id"], "event-69");
        assert_eq!(history[63]["id"], "event-6");
        assert_eq!(history[0]["decided_by"], "adapter");
        assert_eq!(history[0]["adapter_id"], "fixture-adapter");
        assert_eq!(history[0]["model"], "fixture-model");
        assert_eq!(history[0]["confidence"], 0.87);
        assert_eq!(project(&job, &mut 2).len(), 2);
        assert!(!json!(history).to_string().contains("private-"));
    }
}

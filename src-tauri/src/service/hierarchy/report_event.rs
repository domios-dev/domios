//! Broadcast payload for one accepted report. Exposure only: it reads the
//! outcome the submission already decided and never decides anything itself.

use serde_json::{json, Value};

use crate::agent::reports::AgentReport;
use crate::service::RejectedTaskTransition;

/// Task-transition truth of one accepted report: `None` when the report
/// resolved no plan task, so a consumer can tell "no task" from "task refused".
pub(super) fn task_report_outcome(
    persisted_task: Option<&(String, String)>,
    rejected: Option<&RejectedTaskTransition>,
) -> Option<Value> {
    if let Some(rejection) = rejected {
        return Some(json!({
            "persisted": false,
            "transition_rejected": true,
            "plan_id": rejection.plan_id,
            "task_id": rejection.task_id,
            "cause": rejection.cause,
        }));
    }
    persisted_task.map(|(plan_id, task_id)| {
        json!({
            "persisted": true,
            "transition_rejected": false,
            "plan_id": plan_id,
            "task_id": task_id,
            "cause": Value::Null,
        })
    })
}

pub(super) fn agent_report_event(
    target_tab_id: &str,
    from_tab_id: &str,
    from_title: &str,
    report: &AgentReport,
    task_report: Option<Value>,
) -> Value {
    json!({
        "target_tab_id": target_tab_id,
        "from_tab_id": from_tab_id,
        "from_title": from_title,
        "message": report.message,
        "report": report,
        "task_report": task_report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::reports::ReportStore;

    fn report() -> AgentReport {
        ReportStore::new(4).add("orchestrator", "worker", "Worker", "lane merged")
    }

    #[test]
    fn no_resolved_task_reports_no_transition_outcome() {
        assert!(task_report_outcome(None, None).is_none());
    }

    #[test]
    fn persisted_task_is_reported_as_persisted() {
        let persisted = ("pl_one".to_string(), "task_one".to_string());
        let outcome = task_report_outcome(Some(&persisted), None).expect("outcome");
        assert_eq!(outcome["persisted"], json!(true));
        assert_eq!(outcome["transition_rejected"], json!(false));
        assert_eq!(outcome["plan_id"], json!("pl_one"));
        assert_eq!(outcome["task_id"], json!("task_one"));
    }

    #[test]
    fn rejected_transition_wins_and_carries_its_cause() {
        let persisted = ("pl_one".to_string(), "task_one".to_string());
        let rejection = RejectedTaskTransition {
            plan_id: Some("pl_one".to_string()),
            task_id: Some("task_one".to_string()),
            cause: "missing caller session id".to_string(),
        };
        let outcome = task_report_outcome(Some(&persisted), Some(&rejection)).expect("outcome");
        assert_eq!(outcome["persisted"], json!(false));
        assert_eq!(outcome["transition_rejected"], json!(true));
        assert_eq!(outcome["cause"], json!("missing caller session id"));
    }

    /// The exact delivered-but-refused submission the report card must keep
    /// visible on both sides.
    #[test]
    fn delivered_report_with_refused_transition_keeps_both_truths() {
        let rejection = RejectedTaskTransition {
            plan_id: Some("pl_two".to_string()),
            task_id: Some("task_two".to_string()),
            cause: "missing caller session id".to_string(),
        };
        let mut report = report();
        report.id = "79c177b3-0d8d-43f3-a16f-7e9b9412a294".to_string();
        let event = agent_report_event(
            "orchestrator",
            "worker",
            "Worker",
            &report,
            task_report_outcome(None, Some(&rejection)),
        );

        assert_eq!(event["target_tab_id"], json!("orchestrator"));
        assert_eq!(event["from_tab_id"], json!("worker"));
        assert_eq!(
            event["report"]["id"],
            json!("79c177b3-0d8d-43f3-a16f-7e9b9412a294")
        );
        assert_eq!(event["message"], json!("lane merged"));
        assert_eq!(event["task_report"]["persisted"], json!(false));
        assert_eq!(event["task_report"]["transition_rejected"], json!(true));
        assert_eq!(
            event["task_report"]["cause"],
            json!("missing caller session id")
        );
    }

    #[test]
    fn event_without_a_task_carries_an_explicit_null_outcome() {
        let event = agent_report_event("orchestrator", "worker", "Worker", &report(), None);
        assert!(event["task_report"].is_null());
        assert!(!event["report"]["id"]
            .as_str()
            .unwrap_or_default()
            .is_empty());
    }
}

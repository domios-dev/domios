//! Broadcast payload for one accepted report. Exposure only: it reads the
//! outcome the submission already decided and never decides anything itself.

use serde_json::{json, Value};

use crate::agent::reports::AgentReport;
use crate::service::RejectedTaskTransition;
use codeterm_types::agent::ReportOutcomeClass;

/// Task-transition truth of one accepted report, including the source-owned
/// class when the report was delivered without a task transition.
pub(super) fn task_report_outcome(
    persisted_task: Option<&(String, String)>,
    rejected: Option<&RejectedTaskTransition>,
    report_outcome: Option<ReportOutcomeClass>,
) -> Option<Value> {
    if let Some(rejection) = rejected {
        let mut value = json!({
            "persisted": false,
            "transition_rejected": true,
            "plan_id": rejection.plan_id,
            "task_id": rejection.task_id,
            "cause": rejection.cause,
        });
        if let Some(outcome) = report_outcome.or(rejection.report_outcome) {
            value["report_outcome"] = json!(outcome);
        }
        return Some(value);
    }
    if let Some((plan_id, task_id)) = persisted_task {
        let mut value = json!({
            "persisted": true,
            "transition_rejected": false,
            "plan_id": plan_id,
            "task_id": task_id,
            "cause": Value::Null,
        });
        if let Some(outcome) = report_outcome {
            value["report_outcome"] = json!(outcome);
        }
        return Some(value);
    }
    report_outcome.map(|outcome| {
        json!({
            "persisted": false,
            "transition_rejected": false,
            "plan_id": Value::Null,
            "task_id": Value::Null,
            "cause": Value::Null,
            "report_outcome": outcome,
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
    // The submission decides the class; a consumer must not have to dig it out
    // of the nested transition to learn what the producer already knows.
    let outcome = match report.report_outcome {
        Some(class) => json!(class),
        None => task_report
            .as_ref()
            .and_then(|value| value.get("report_outcome"))
            .cloned()
            .unwrap_or(Value::Null),
    };
    json!({
        "target_tab_id": target_tab_id,
        "from_tab_id": from_tab_id,
        "from_title": from_title,
        "message": report.message,
        "report": report,
        "task_report": task_report,
        "report_outcome": outcome,
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
        assert!(task_report_outcome(None, None, None).is_none());
    }

    #[test]
    fn persisted_task_is_reported_as_persisted() {
        let persisted = ("pl_one".to_string(), "task_one".to_string());
        let outcome = task_report_outcome(Some(&persisted), None, None).expect("outcome");
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
            report_outcome: Some(ReportOutcomeClass::StaleBindingConflict),
        };
        let outcome =
            task_report_outcome(Some(&persisted), Some(&rejection), None).expect("outcome");
        assert_eq!(outcome["persisted"], json!(false));
        assert_eq!(outcome["transition_rejected"], json!(true));
        assert_eq!(outcome["cause"], json!("missing caller session id"));
        assert_eq!(outcome["report_outcome"], json!("stale_binding_conflict"));
    }

    /// The exact delivered-but-refused submission the report card must keep
    /// visible on both sides.
    #[test]
    fn delivered_report_with_refused_transition_keeps_both_truths() {
        let rejection = RejectedTaskTransition {
            plan_id: Some("pl_two".to_string()),
            task_id: Some("task_two".to_string()),
            cause: "missing caller session id".to_string(),
            report_outcome: Some(ReportOutcomeClass::StaleBindingConflict),
        };
        let mut report = report();
        report.id = "79c177b3-0d8d-43f3-a16f-7e9b9412a294".to_string();
        let event = agent_report_event(
            "orchestrator",
            "worker",
            "Worker",
            &report,
            task_report_outcome(
                None,
                Some(&rejection),
                Some(ReportOutcomeClass::StaleBindingConflict),
            ),
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
        assert_eq!(event["report_outcome"], json!("stale_binding_conflict"));
    }

    #[test]
    fn event_without_a_task_carries_an_explicit_null_outcome() {
        let event = agent_report_event("orchestrator", "worker", "Worker", &report(), None);
        assert!(event["task_report"].is_null());
        assert!(event["report_outcome"].is_null());
        assert!(!event["report"]["id"]
            .as_str()
            .unwrap_or_default()
            .is_empty());
    }

    #[test]
    fn structural_and_unattached_classes_are_reported_without_a_transition() {
        for class in [
            ReportOutcomeClass::StructurallyNotAttachable,
            ReportOutcomeClass::DeliveredUnattached,
        ] {
            let outcome = task_report_outcome(None, None, Some(class)).expect("outcome");
            assert_eq!(outcome["persisted"], json!(false));
            assert_eq!(outcome["transition_rejected"], json!(false));
            assert_eq!(outcome["report_outcome"], json!(class));
        }
    }
}

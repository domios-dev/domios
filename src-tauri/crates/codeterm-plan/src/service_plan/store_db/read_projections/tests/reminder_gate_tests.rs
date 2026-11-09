use super::*;
use crate::plan::{
    DecisionGate, ReviewEvidence, ReviewVerdict, TaskExecutionReportSnapshot, TaskStatus,
};
use crate::service_plan::state::task_projection;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn hash(value: &impl serde::Serialize) -> String {
    format!("{:x}", Sha256::digest(serde_json::to_vec(value).unwrap()))
}

fn projection(plan: &Plan) -> BTreeMap<String, String> {
    let mut out = BTreeMap::from([("mode".into(), format!("{:?}:{:?}", plan.mode, plan.status))]);
    for task in &plan.tasks {
        out.insert(
            format!("task:{}", task.id),
            hash(&(
                &task.status,
                &task.status_label,
                &task.pause_reason,
                &task.interruption,
                &task.review_stage,
                &task.qa_state,
                &task.assigned_tab_id,
                &task_projection(plan, task).dependency_waits,
            )),
        );
        out.insert(format!("review:{}", task.id), hash(&task.review_evidence));
        for attempt in &task.execution_attempts {
            out.insert(
                format!("attempt:{}", attempt.id),
                hash(&attempt.interruption),
            );
            if let Some(report) = &attempt.report {
                out.insert(format!("report:{}", report.id), hash(report));
            }
        }
        for stride in &task.milestones {
            out.insert(format!("stride:{}", stride.id), hash(stride));
        }
    }
    for stride in &plan.strides {
        out.insert(format!("stride:{}", stride.id), hash(stride));
    }
    for gate in &plan.gates {
        out.insert(format!("gate:{}", gate.id), hash(gate));
    }
    out
}

fn excerpt(text: &str) -> String {
    text.chars().take(512).collect()
}

fn context(plan: &Plan) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::from([(
        "mode".into(),
        json!({"mode":plan.mode,"status":plan.status}),
    )]);
    for task in &plan.tasks {
        let projected = task_projection(plan, task);
        out.insert(format!("task:{}",task.id),json!({"task":task.id,"status":task.status,
            "status_label":task.status_label.as_deref().map(excerpt),"pause_reason":task.pause_reason.as_deref().map(excerpt),
            "assigned_tab_id":task.assigned_tab_id,"presentation":projected.presentation,
            "dependency_waits":projected.dependency_waits.iter().map(|wait|json!({"task_id":wait.task_id,"status":wait.status,"missing":wait.missing})).collect::<Vec<_>>(),
            "held":crate::plan::reconcile::active_review_hold_note(task).is_some()}));
        for attempt in &task.execution_attempts {
            if let Some(report) = &attempt.report {
                out.insert(format!("report:{}",report.id),json!({"task":task.id,"status":report.status,"sha":report.sha,"test_result":report.test_result.as_deref().map(excerpt)}));
            }
        }
    }
    for stride in plan
        .strides
        .iter()
        .chain(plan.tasks.iter().flat_map(|task| &task.milestones))
    {
        out.insert(
            format!("stride:{}", stride.id),
            json!({"kind":stride.kind,"text":excerpt(&stride.text)}),
        );
    }
    for gate in &plan.gates {
        out.insert(format!("gate:{}",gate.id),json!({"status":gate.status,"answer":gate.answer.as_ref().map(|answer|excerpt(&answer.text))}));
    }
    out
}

#[test]
fn reminder_owner_composition_matches_the_held_plan_oracle_for_directives_and_modes() {
    let directory = tempfile::tempdir().unwrap();
    let base = super::curator_notice_tests::decisions(directory.path());
    for mode in [
        crate::plan::PlanMode::Planning,
        crate::plan::PlanMode::Execution,
        crate::plan::PlanMode::Complete,
    ] {
        let mut plan = base.clone();
        plan.mode = mode;
        let database = fixtures::database(directory.path(), &plan);
        let snapshot = database.reminder_read_snapshot(excerpt).unwrap();
        let read = snapshot.composition(&plan.id).unwrap().unwrap();
        for directive in ["directive pending", "\n\n", &"overflow\n".repeat(1000)] {
            let digests = vec!["digest pending".into()];
            let selection = Some(["first".to_string(), "second".to_string()]);
            let plain = plan.mode != crate::plan::PlanMode::Execution;
            let heartbeat = if plain {
                String::new()
            } else {
                crate::service_plan::heartbeat::view::HeartbeatView::from_plan_at(&plan, 1000)
                    .prepend("", &digests, selection.as_ref().map(|ids| ids.as_slice()))
            };
            let expected = crate::service_plan::reminders::composition::compose(
                &plan,
                &[
                    &crate::service_plan::reminders::composition::action_groups(&plan),
                    directive,
                    &crate::service_plan::reminders::parked_later_group_summary(&plan),
                    &heartbeat,
                ],
            );
            assert_eq!(
                read.compose_owner(
                    directive,
                    &digests,
                    selection.as_ref().map(|ids| ids.as_slice()),
                    plain,
                    1000
                )
                .unwrap(),
                expected
            );
        }
    }
}

#[test]
fn reminder_gate_fingerprints_and_context_preserve_original_order_with_bounded_payload_reads() {
    let mut baseline = None;
    for extra in [0, 32768] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), 1, 1);
        let payload = format!("escaped \" text\n{}", "界".repeat(512 + extra));
        plan.tasks[0].status = TaskStatus::InReview;
        plan.tasks[0].status_changed_at = 100;
        plan.tasks[0].status_label = Some(payload.clone());
        plan.tasks[0].depends_on = vec![plan.tasks[1].id.clone(), "missing".into()];
        plan.tasks[0].review_evidence.push(ReviewEvidence {
            id: "review".into(),
            from: Default::default(),
            verdict: ReviewVerdict::Hold,
            note: payload.clone(),
            at: 500,
            changed_executable_surfaces: false,
            distinct_oracle: false,
        });
        plan.tasks[1].status = TaskStatus::Blocked;
        plan.tasks[1].pause_reason = Some(payload.clone());
        plan.tasks[1].name = payload.clone();
        plan.tasks[0].execution_attempts[0].interruption.reason = Some(payload.clone());
        plan.tasks[0].execution_attempts[0].report = Some(TaskExecutionReportSnapshot {
            id: "report".into(),
            message: Some(payload.clone()),
            status: Some("in_review".into()),
            sha: Some("sha".into()),
            test_result: Some(payload.clone()),
            ..Default::default()
        });
        plan.strides[0].text = payload.clone();
        let mut task_stride = plan.strides[0].clone();
        task_stride.text = format!("task {payload}");
        plan.tasks[0].milestones.push(task_stride);
        let gate:DecisionGate=serde_json::from_value(json!({"id":"gate","question":payload,"context":payload,"asked_at":1,"status":"answered","answer":{"text":payload,"at":2,"by_tab_id":null}})).unwrap();
        plan.gates.push(gate);
        let expected = projection(&plan);
        let expected_context = context(&plan)
            .into_iter()
            .take(32)
            .collect::<BTreeMap<_, _>>();
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        let reference = database
            .reminder_read_snapshot(excerpt)
            .unwrap()
            .plan(&plan.id)
            .unwrap()
            .unwrap()
            .with_gate_facts()
            .unwrap();
        assert_eq!(
            reference.tasks[1].blocked_fingerprint,
            Some(hash(&(
                "task_blocked",
                plan.tasks[1].pause_reason.as_deref().unwrap().trim()
            )))
        );
        assert_eq!(
            reference.tasks[0].attempts[0].interruption_fingerprint,
            Some(hash(&("machine_hold", payload.as_str())))
        );
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let snapshot = database.reminder_read_snapshot(excerpt).unwrap();
        let read = snapshot
            .admission(&plan.id)
            .unwrap()
            .unwrap()
            .with_gate_facts()
            .unwrap();
        assert_eq!(read.projection, expected);
        assert!(
            read.tasks.is_empty(),
            "owner reads carry no redundant task presentation or attempt metadata"
        );
        let blockers = read.blocker_facts(Some(&plan.tasks[1].id)).unwrap();
        assert!(blockers.iter().any(|blocker| blocker.identity
            == hash(&(
                "task_blocked",
                plan.tasks[1].pause_reason.as_deref().unwrap().trim()
            ))));
        let (selected, truncated) = read.delta_context(&read.projection, 32).unwrap();
        assert_eq!(selected, expected_context);
        assert!(truncated);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(&read, &selected)).unwrap().len();
        assert_eq!(serialized, 0);
        assert!(metrics.materialized_bytes <= response + 4096);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumers=reminder_gate_fingerprints,reminder_gate_context,reminder_blocked_notice tasks={} evidence_payload_growth={extra} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &baseline {
            assert_eq!(&measured, previous);
        }
        baseline = Some(measured);
    }
}

#[test]
fn worker_reminder_evidence_ignores_unrelated_attempt_and_stride_growth() {
    let mut baseline = None;
    for (history, attempts) in [(0, 1), (2048, 1), (0, 2048)] {
        let directory = tempfile::tempdir().unwrap();
        let plan = fixtures::fixture(directory.path(), history, attempts);
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let snapshot = database.reminder_read_snapshot(excerpt).unwrap();
        let read = snapshot
            .admission(&plan.id)
            .unwrap()
            .unwrap()
            .with_worker_gate_facts(&plan.tasks[0].id)
            .unwrap();
        assert_eq!(read.tasks[0].attempts.len(), 1);
        assert_eq!(
            read.tasks.len(),
            1,
            "only the scoped worker task is returned"
        );
        assert!(read.projection.keys().all(|key| key == "mode"
            || key == &format!("task:{}", plan.tasks[0].id)
            || key == &format!("review:{}", plan.tasks[0].id)
            || key == "attempt:current"));
        let blockers = read.blocker_facts(Some(&plan.tasks[0].id)).unwrap();
        assert!(blockers.is_empty());
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(serialized, 0);
        assert!(metrics.materialized_bytes <= response + 4096);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumers=worker_reminder_scoped_gate tasks={} old_strides={history} unrelated_attempts={attempts} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &baseline {
            assert_eq!(&measured, previous);
        }
        baseline = Some(measured);
    }
}

#[test]
fn owner_gate_rows_preserve_hard_fact_order_without_closed_attempt_metadata() {
    use crate::service_plan::ReminderOwnerRowRead;
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 1, 1);
    plan.tasks.truncate(5);
    plan.tasks[0].status = TaskStatus::InProgress;
    plan.tasks[1].status = TaskStatus::Done;
    plan.tasks[2].status = TaskStatus::Blocked;
    plan.tasks[1].execution_attempts[0].interruption.reason = Some("closed hold".into());
    plan.tasks[1].execution_attempts[0].report = Some(TaskExecutionReportSnapshot {
        id: "closed-report".into(),
        ..Default::default()
    });
    let database = fixtures::database(directory.path(), &plan);
    let snapshot = database.reminder_read_snapshot(excerpt).unwrap();
    let reference = snapshot
        .plan(&plan.id)
        .unwrap()
        .unwrap()
        .with_gate_facts()
        .unwrap();
    let read = snapshot
        .admission(&plan.id)
        .unwrap()
        .unwrap()
        .with_gate_facts()
        .unwrap();
    assert_eq!(read.projection, reference.projection);
    for delta in [BTreeMap::new(), read.projection.clone()] {
        let mut expected = Vec::new();
        for task in &reference.tasks {
            if delta.contains_key(&format!("task:{}", task.id)) {
                expected.push((
                    match task.status {
                        TaskStatus::Done | TaskStatus::Cancelled => "task_closed",
                        TaskStatus::Blocked => "task_blocked",
                        _ => "task_state",
                    }
                    .into(),
                    task.id.clone(),
                ));
            }
            if delta.contains_key(&format!("review:{}", task.id)) {
                if let Some(review) = task
                    .last_review
                    .as_ref()
                    .filter(|review| review.verdict == ReviewVerdict::Bounce)
                {
                    expected.push(("failed_gate".into(), review.id.clone()));
                }
            }
            for attempt in &task.attempts {
                if attempt.interruption_fingerprint.is_some()
                    && (!attempt.closed || delta.contains_key(&format!("attempt:{}", attempt.id)))
                {
                    expected.push((
                        "incident".into(),
                        format!("dispatch_or_worker_failure:{}", attempt.id),
                    ));
                }
                if let Some(report) = &attempt.report_id {
                    if delta.contains_key(&format!("report:{report}")) {
                        expected.push(("new_report".into(), report.clone()));
                    }
                }
            }
            if !task.status.counts_as_open_work() {
                continue;
            }
            if task.status == TaskStatus::Pending
                && task.assigned_tab_id.is_none()
                && task.projection.needs_orchestrator_action()
            {
                expected.push(("incident".into(), format!("dispatch_required:{}", task.id)));
            }
            if task.attempts.iter().any(|attempt| !attempt.closed) {
                expected.push(("worker".into(), task.id.clone()));
            }
            if task.status == TaskStatus::InProgress
                && task.attempts.iter().all(|attempt| attempt.closed)
            {
                expected.push(("incident".into(), format!("dispatch_missing:{}", task.id)));
            }
            if task.handoff_required {
                expected.push(("incident".into(), format!("task_interruption:{}", task.id)));
            }
        }
        let actual = read
            .owner_rows(&delta)
            .unwrap()
            .into_iter()
            .map(|row| match row {
                ReminderOwnerRowRead::Wake { event, id } => (event, id),
                ReminderOwnerRowRead::Worker { task } => {
                    assert!(task.attempts.iter().all(|attempt| !attempt.closed));
                    ("worker".into(), task.id)
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        assert_eq!(
            read.delta_context(&delta, 32).unwrap(),
            reference.delta_context(&delta, 32).unwrap()
        );
    }
}

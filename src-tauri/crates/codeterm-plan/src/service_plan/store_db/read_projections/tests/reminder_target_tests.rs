use super::*;
use crate::plan::{ControlLoopMode, PlanMode, TaskStatus};

#[test]
fn reminder_targets_are_output_sized_with_unassigned_task_history_and_payload_growth() {
    let mut baseline = None;
    for (extra_tasks, history, attempts, body) in [
        (0, 0, 1, 0),
        (0, 2048, 1, 0),
        (0, 0, 2048, 0),
        (0, 0, 1, 32768),
        (600, 0, 1, 0),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), history, attempts);
        plan.mode = PlanMode::Execution;
        plan.control_loop.mode = ControlLoopMode::Heartbeat;
        plan.owner.tab_id = Some("owner".into());
        plan.tasks[0].status = TaskStatus::InProgress;
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[1].name.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"y".repeat(body));
        for i in 0..extra_tasks {
            let mut task = plan.tasks[2].clone();
            task.id = format!("extra-{i}");
            task.handle = None;
            plan.tasks.push(task);
        }
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        let reference = database
            .reminder_read_snapshot(str::to_string)
            .unwrap()
            .candidate_reference(None)
            .unwrap();
        let reference_bytes = serde_json::to_vec(&reference).unwrap().len();
        assert!(reference_bytes > 200_000);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let snapshot = database.reminder_read_snapshot(str::to_string).unwrap();
        let targets = snapshot.candidate_targets(None).unwrap();
        let bindings = snapshot.worker_bindings("worker", None).unwrap();
        let popup = snapshot.candidate_targets(Some("owner")).unwrap();
        let cooldown = snapshot
            .admission(&plan.id)
            .unwrap()
            .unwrap()
            .cooldown_key()
            .unwrap();
        assert_eq!(cooldown, reference[0].state_key_digest);
        assert_eq!(targets.len(), 1);
        assert!(targets[0].owner_eligible());
        assert_eq!(
            targets[0].retained_scope_tabs.as_ref().unwrap(),
            &["worker"]
        );
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].task_id, plan.tasks[0].id);
        assert_eq!(
            popup[0].popup.as_ref().unwrap().labels,
            vec![crate::service_plan::reminders::composition::task_label(
                &plan.tasks[0]
            )]
        );
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(&targets, &bindings, &popup, &cooldown))
            .unwrap()
            .len();
        assert!(response < 4096);
        assert!(metrics.materialized_bytes < 4096);
        assert!(decoded < 4096);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumers=reminder_scheduler,reminder_scope_retention,reminder_worker_resolution,reminder_popup,reminder_cooldown tasks={} old_strides={history} unrelated_attempts={attempts} extra_body={body} reference_response_bytes={reference_bytes} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,plan.tasks.len(),metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &baseline {
            assert_eq!(&measured, previous);
        }
        baseline = Some(measured);
    }
}

#[test]
fn reminder_targets_skip_gate_history_until_admission_and_keep_sql_failures_loud() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    plan.mode = PlanMode::Execution;
    plan.tasks[0].assigned_tab_id = Some("worker".into());
    let database = fixtures::database(directory.path(), &plan);
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_task_attempts")
        .unwrap();
    let snapshot = database.reminder_read_snapshot(str::to_string).unwrap();
    assert_eq!(snapshot.candidate_targets(None).unwrap().len(), 1);
    assert_eq!(snapshot.worker_bindings("worker", None).unwrap().len(), 1);
    assert!(snapshot
        .admission(&plan.id)
        .unwrap()
        .unwrap()
        .with_gate_facts()
        .is_err());
    drop(snapshot);
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_tasks")
        .unwrap();
    let snapshot = database.reminder_read_snapshot(str::to_string).unwrap();
    assert!(snapshot.candidate_targets(None).is_err());
    assert!(snapshot.worker_bindings("worker", None).is_err());
}

#[test]
fn scoped_worker_candidate_ids_preserve_the_previous_eligibility_and_ambiguity_order() {
    for case in 0..8 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), 0, 1);
        plan.tasks.truncate(6);
        plan.mode = PlanMode::Execution;
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[1].assigned_tab_id = Some("worker".into());
        match case {
            1 => plan.tasks[0].depends_on = vec!["missing".into()],
            2 => {
                plan.tasks[0].status = TaskStatus::Blocked;
                plan.tasks[0].depends_on = vec!["missing".into()]
            }
            3 => {
                plan.tasks[0].group = Some("later".into());
                plan.tasks[1].group = Some("first".into());
                plan.task_groups=serde_json::from_value(json!([{"key":"first","label":"First","color":null},{"key":"later","label":"Later","color":null}])).unwrap()
            }
            4 => plan.tasks[0].interruption.handoff_required = true,
            5 => plan.tasks[2].parent_task_id = Some(plan.tasks[0].id.clone()),
            6 => plan.tasks[0].assigned_tab_id = Some(" worker ".into()),
            7 => plan.control_loop.mode = ControlLoopMode::Off,
            _ => {}
        }
        let database = fixtures::database(directory.path(), &plan);
        let snapshot = database.reminder_read_snapshot(str::to_string).unwrap();
        let reference = snapshot.candidate_reference(None).unwrap();
        let expected = reference
            .iter()
            .filter(|plan| {
                plan.control_loop
                    .active_execution(&plan.status, &plan.mode, plan.complete())
            })
            .flat_map(|plan| {
                plan.tasks
                    .iter()
                    .filter(|task| {
                        task.assigned_tab_id.as_deref().map(str::trim) == Some("worker")
                            && task
                                .projection
                                .presentation
                                .worker_reminder_eligible(&task.status)
                    })
                    .map(move |task| crate::service_plan::ReminderWorkerBindingRead {
                        plan_id: plan.id.clone(),
                        task_id: task.id.clone(),
                    })
            })
            .take(2)
            .collect::<Vec<_>>();
        assert_eq!(
            snapshot.worker_bindings("worker", None).unwrap(),
            expected,
            "case {case}"
        );
        assert_eq!(
            snapshot.worker_bindings("worker", Some(&plan.id)).unwrap(),
            expected
        );
        assert!(snapshot
            .worker_bindings("worker", Some("missing"))
            .unwrap()
            .is_empty());
    }
}

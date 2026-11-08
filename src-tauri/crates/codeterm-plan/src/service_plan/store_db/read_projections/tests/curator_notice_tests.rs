use super::*;
use crate::plan::{PlanMode, PlanOrphan, QaState, ReviewStage, TaskStatus};
use crate::service_plan::{heartbeat, reminders};

fn reference(plan: &Plan, now: u64) -> String {
    let heartbeat = heartbeat::view::HeartbeatView::from_plan_at(plan, now).prepend("", &[], None);
    reminders::composition::compose(
        plan,
        &[
            &reminders::composition::action_groups(plan),
            &reminders::parked_later_group_summary(plan),
            &heartbeat,
        ],
    )
}

pub(super) fn decisions(root: &std::path::Path) -> Plan {
    let mut plan = Plan::new(
        "Typed notice α".into(),
        root.display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.mode = PlanMode::Execution;
    plan.team_id = Some("team".into());
    for (id, status) in [
        ("pending", TaskStatus::Pending),
        ("review", TaskStatus::InReview),
        ("queued", TaskStatus::InReview),
        ("running", TaskStatus::InReview),
        ("blocked", TaskStatus::Blocked),
        ("waiting", TaskStatus::Pending),
        ("missing", TaskStatus::Pending),
        ("parent", TaskStatus::Pending),
        ("child", TaskStatus::Pending),
        ("handoff", TaskStatus::Paused),
        ("later", TaskStatus::Pending),
        ("closed", TaskStatus::Done),
        ("held", TaskStatus::InReview),
        ("Review", TaskStatus::Pending),
        ("dispatch", TaskStatus::Pending),
        ("-", TaskStatus::Pending),
    ] {
        plan.tasks
            .push(crate::service_plan::load_audit::task(id, status, None));
    }
    plan.tasks[0].key = Some("COD-42".into());
    plan.tasks[0].name = "First α β".into();
    plan.tasks[0].group = Some("current".into());
    plan.tasks[2].review_stage = ReviewStage::Qa;
    plan.tasks[2].qa_state = Some(QaState::Queued);
    plan.tasks[3].review_stage = ReviewStage::Qa;
    plan.tasks[3].qa_state = Some(QaState::Running);
    plan.tasks[4].depends_on = vec!["absent".into()];
    plan.tasks[5].depends_on = vec!["pending".into()];
    plan.tasks[6].depends_on = vec!["absent".into()];
    plan.tasks[8].parent_task_id = Some("parent".into());
    plan.tasks[9].interruption.handoff_required = true;
    plan.tasks[10].group = Some("later".into());
    plan.tasks[11].group = Some("undeclared".into());
    plan.tasks[11].status_changed_at = 200;
    plan.tasks[12].status_changed_at = u64::MAX - 1;
    plan.tasks[12].review_evidence = serde_json::from_value(
        json!([{"id":"held","from":{},"verdict":"hold","note":"kept","at":u64::MAX}]),
    )
    .unwrap();
    plan.tasks[13].name = "heading label".into();
    plan.tasks[14].name = "verb label".into();
    plan.tasks[15].name = String::new();
    plan.tasks[1].execution_attempts = (0..2)
        .map(|index| crate::service_plan::load_audit::attempt(&format!("review-{index}"), "worker"))
        .collect();
    plan.tasks[3].execution_attempts = (0..2)
        .map(|index| {
            crate::service_plan::load_audit::attempt(&format!("qa-running-{index}"), "worker")
        })
        .collect();
    plan.task_groups=serde_json::from_value(json!([
        {"key":"current","label":"Current","color":null},{"key":"later","label":"Later","color":null},
        {"key":"current","label":"Duplicate declaration","color":null}
    ])).unwrap();
    plan.loops = serde_json::from_value(
        json!([{"id":"pending","target":"pending","status":"running","count":1,"budget":2}]),
    )
    .unwrap();
    plan.heartbeat.learnings=serde_json::from_value(json!([
        {"id":"owner-rule","text":"Keep   the owner's contract","tag":"owner","ts":1},
        {"id":"first","text":"first","ts":2},{"id":"second","text":"second","ts":3},
        {"id":"third","text":"third","ts":4},{"id":"fourth","text":"fourth","ts":5},{"id":"empty","text":"  ","ts":6}
    ])).unwrap();
    plan.gates=serde_json::from_value(json!([{"id":"gate","question":"α".repeat(120),"asked_at":100,"status":"escalated","blocking_task_ids":["pending"],"routed_to_tab_id":"owner"}])).unwrap();
    plan
}

#[test]
fn curator_notice_uses_shared_decisions_labels_and_composition_for_all_modes() {
    let directory = tempfile::tempdir().unwrap();
    let base = decisions(directory.path());
    for (index, mode) in [
        PlanMode::Planning,
        PlanMode::Approved,
        PlanMode::Execution,
        PlanMode::Paused,
        PlanMode::Complete,
    ]
    .into_iter()
    .enumerate()
    {
        let case = tempfile::tempdir().unwrap();
        let mut plan = base.clone();
        plan.mode = mode;
        let database = fixtures::database(case.path(), &plan);
        let actual = database
            .curator_notice_projection(&plan.id, 10_000)
            .unwrap();
        assert_eq!(actual.body, reference(&plan, 10_000), "mode case {index}");
        assert_eq!(actual.owner_tab_id, plan.owner.tab_id);
        assert!(actual.body.len() <= reminders::composition::MAX_REMINDER_BYTES);
        let choices = [None, Some(vec!["fourth".to_string(), "first".to_string()])];
        for selection in &choices {
            for digests in [
                vec!["opaque digest".into(), "pending".into(), " ".into()],
                vec!["opaque digest ".repeat(10); 64],
            ] {
                let heartbeat = heartbeat::view::HeartbeatView::from_plan_at(&plan, 10_000)
                    .prepend("", &digests, selection.as_deref());
                let expected = reminders::composition::compose(
                    &plan,
                    &[
                        &reminders::composition::action_groups(&plan),
                        &reminders::parked_later_group_summary(&plan),
                        &heartbeat,
                    ],
                );
                let connection = database.connection.lock();
                let scoped = super::super::super::curator_notice_queries::bounded_notice(
                    &connection,
                    &plan.id,
                    10_000,
                    selection.as_deref(),
                    &digests,
                )
                .unwrap();
                assert_eq!(scoped.body, expected);
                assert!(scoped.body.len() <= reminders::composition::MAX_REMINDER_BYTES);
            }
        }
    }
    for complete in [false, true] {
        let case = tempfile::tempdir().unwrap();
        let mut plan = base.clone();
        if complete {
            for task in &mut plan.tasks {
                task.status = TaskStatus::Done;
            }
        } else {
            plan.team_id = None;
            plan.orphaned = Some(PlanOrphan {
                team_id: "old-team".into(),
                reason: "closed".into(),
                unfinished_task_ids: vec![],
                at: 1,
            });
        }
        let database = fixtures::database(case.path(), &plan);
        assert_eq!(
            database
                .curator_notice_projection(&plan.id, 10_000)
                .unwrap()
                .body,
            reference(&plan, 10_000)
        );
    }
}

#[test]
fn curator_notice_materialization_is_bounded_by_output_not_task_history_or_body_size() {
    for sparse in [false, true] {
        let mut prior = None;
        for (strides, attempts, body, selected_history, rounds) in [
            (0, 1, 0, 0, 0),
            (2048, 1, 0, 0, 0),
            (0, 2048, 0, 0, 0),
            (0, 1, 32768, 0, 0),
            (0, 1, 0, 2048, 0),
            (0, 1, 0, 0, 2048),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let mut plan = fixtures::fixture(directory.path(), strides, attempts);
            plan.mode = PlanMode::Execution;
            if sparse {
                for task in plan.tasks.iter_mut().skip(1) {
                    task.status = TaskStatus::Done;
                }
            }
            plan.tasks[1].body.push_str(&"x".repeat(body));
            plan.owner.title = Some("x".repeat(body));
            for index in 0..selected_history {
                let mut attempt =
                    crate::service_plan::load_audit::attempt(&format!("history-{index}"), "old");
                attempt.closed_at = Some(1);
                plan.tasks[0].execution_attempts.push(attempt);
                plan.tasks[0].milestones.push(serde_json::from_value(json!({"id":format!("history-{index}"),"from":{},"kind":"note","text":"unselected history","ts":1})).unwrap());
            }
            plan.strides.extend(fixtures::round_strides(
                (0..rounds)
                    .map(|number| crate::plan::CuratorRound {
                        number,
                        ..Default::default()
                    })
                    .collect(),
            ));
            let database = fixtures::database(directory.path(), &plan);
            let store = PlanStore::open(database.path()).unwrap();
            fixtures::audit(&database);
            crate::service_plan::store_sync::take_decoded_row_bytes();
            database.load_plan(&plan.id).unwrap().unwrap();
            let baseline = crate::service_plan::store_sync::take_decoded_row_bytes();
            let expected = reference(&plan, 10_000);
            take_metrics();
            crate::service_plan::store_rows::take_serialized_row_bytes();
            let actual = store.curator_notice_projection(&plan.id, 10_000).unwrap();
            let metrics = take_metrics();
            let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
            let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
            assert_eq!(actual.body, expected);
            assert_eq!(actual.owner_tab_id, plan.owner.tab_id);
            assert!(metrics.materialized_bytes < 16 * 1024);
            assert!(decoded < 4096);
            assert_eq!(serialized, 0);
            assert!(store.plan_loads_for_test().is_empty());
            assert!(fixtures::audit_counts(&database).is_empty());
            println!("fixture={} consumer=curator_notice tasks={} base_bytes={} sparse={sparse} old_strides={strides} unrelated_attempts={attempts} extra_body={body} selected_history={selected_history} rounds={rounds} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes,actual.body.len());
            let measured = (metrics, decoded, actual.body.len());
            if let Some(prior) = &prior {
                assert_eq!(&measured, prior);
            }
            prior = Some(measured);
        }
    }
}

#[test]
fn curator_notice_owner_body_and_decisions_share_one_snapshot_and_errors_stay_loud() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("owner-a"),
    );
    plan.mode = PlanMode::Execution;
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "task_one",
        TaskStatus::Pending,
        None,
    )];
    plan.loops = serde_json::from_value(
        json!([{"id":"A","target":"fixed","status":"running","count":1,"budget":1}]),
    )
    .unwrap();
    plan.heartbeat.learnings =
        serde_json::from_value(json!([{"id":"rule","text":"A","tag":"owner","ts":1}])).unwrap();
    let database = fixtures::database(directory.path(), &plan);
    let id = plan.id.clone();
    let path = database.path();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        for index in 0..16 {
            let (title, owner) = if index % 2 == 0 {
                ("B", "owner-b")
            } else {
                ("A", "owner-a")
            };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plans SET header_json=json_set(header_json,'$.title',?2,'$.owner.tab_id',?3,'$.loops[0].id',?2) WHERE id=?1",params![id,title,owner]).unwrap();
            tx.execute("UPDATE plan_heartbeat SET row_json=json_set(row_json,'$.learnings[0].text',?2) WHERE plan_id=?1",params![id,title]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let notice = database
            .curator_notice_projection(&plan.id, 10_000)
            .unwrap();
        assert_eq!(
            notice.body.contains("(B)"),
            notice.owner_tab_id.as_deref() == Some("owner-b")
        );
        let heartbeat = database
            .heartbeat_projection(&plan.id, 10_000, None)
            .unwrap()
            .prepend("fixed", &[]);
        assert_eq!(
            heartbeat.contains("loop B nearly exhausted"),
            heartbeat.contains("- B\n")
        );
        let shown = database
            .heartbeat_show_projection(&plan.id, 10_000)
            .unwrap();
        assert_eq!(
            shown.reinjection.contains("loop B nearly exhausted"),
            shown
                .heartbeat
                .learnings
                .iter()
                .any(|learning| learning.text == "B")
        );
        assert_eq!(
            shown.reinjection.contains("- B\n"),
            shown
                .heartbeat
                .learnings
                .iter()
                .any(|learning| learning.text == "B")
        );
        barrier.wait();
    }
    writer.join().unwrap();
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_tasks SET row_json=json_remove(row_json,'$.name') WHERE plan_id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(database
        .curator_notice_projection(&plan.id, 10_000)
        .is_err());
    assert!(matches!(
        database.curator_notice_projection("missing", 10_000),
        Err(PlanError::NotFound)
    ));
}

use super::*;
use crate::service_plan::{TeamEventCursor, TeamPlanRead};

fn stored_id(
    ordinal: usize,
    event: &crate::service_plan::state::mission_control::PlanEvent,
) -> String {
    format!(
        "stored-{ordinal}-{}-{}-{}",
        event.kind,
        event.ts,
        event.task_id.as_deref().unwrap_or_default()
    )
}

fn lifecycle_id(ordinal: usize, event: &crate::plan::PlanLifecycleEvent) -> String {
    if event.id.is_empty() {
        format!("lifecycle-{ordinal}-{}-{}", event.kind, event.at)
    } else {
        event.id.clone()
    }
}

fn expected_page(plan: &Plan, limit: usize, cursor: Option<&TeamEventCursor>) -> TeamPlanRead {
    let mut input = TeamPlanRead::from(plan);
    let after = |at: u64, id: &str| {
        cursor.is_none_or(|cursor| {
            at < cursor.ts
                || (at == cursor.ts
                    && (plan.id > cursor.plan_id
                        || (plan.id == cursor.plan_id && id > cursor.event_id.as_str())))
        })
    };
    input
        .lifecycle_events
        .retain(|(ordinal, event)| after(event.at, &lifecycle_id(*ordinal, event)));
    input.lifecycle_events.sort_by(|(a, left), (b, right)| {
        right
            .at
            .cmp(&left.at)
            .then_with(|| lifecycle_id(*a, left).cmp(&lifecycle_id(*b, right)))
    });
    input.lifecycle_events.truncate(limit + 1);
    input
        .stored_events
        .retain(|(ordinal, event)| after(event.ts, &stored_id(*ordinal, event)));
    input.stored_events.sort_by(|(a, left), (b, right)| {
        right
            .ts
            .cmp(&left.ts)
            .then_with(|| stored_id(*a, left).cmp(&stored_id(*b, right)))
    });
    input.stored_events.truncate(limit + 1);
    input
}

fn team_metadata(plan: &mut Plan) {
    plan.team_id = Some("selected".into());
    plan.goal = Some(crate::plan::Goal {
        vector: "Requested vector".into(),
        vision: "Unrequested vision".into(),
        constraints: Vec::new(),
        postulates: Vec::new(),
    });
    plan.lifecycle_events = serde_json::from_value(json!([
        {"id":"","kind":"opened","at":40,"author":{"tab_id":"owner"}},
        {"id":"same","kind":"activated","at":50,"author":{"tab_id":"owner"}},
        {"id":"highest","kind":"archived","at":u64::MAX,"author":{"tab_id":"owner"}}
    ]))
    .unwrap();
}

#[test]
fn team_sql_preserves_compact_summaries_event_ordinals_cursor_and_selected_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    plan.tasks.truncate(3);
    team_metadata(&mut plan);
    plan.tasks[0].milestones = serde_json::from_value(json!([
        {"id":"a","kind":"worker_report","text":"First","from":{"tab_id":"worker"},"ts":45},
        {"id":"b","kind":"worker_report","text":"Second","from":{"tab_id":"worker"},"ts":46}
    ]))
    .unwrap();
    let database = fixtures::database(directory.path(), &plan);
    let mut foreign = plan.clone();
    foreign.id = "foreign".into();
    foreign.team_id = Some("other".into());
    database.persist_plan_rows(&foreign, None).unwrap();
    let store = PlanStore::open(database.path()).unwrap();
    let actual = store
        .team_mission_control_projection("selected", 2, None)
        .unwrap();
    assert_eq!(actual, vec![expected_page(&plan, 2, None)]);
    assert_eq!(
        actual[0].metadata.goal.as_ref().unwrap().vector,
        "Requested vector"
    );
    assert!(actual[0]
        .stored_events
        .iter()
        .any(|(_, event)| event.kind == "worker_report" && event.count == 2));
    for cursor in [
        TeamEventCursor {
            ts: 50,
            plan_id: plan.id.clone(),
            event_id: "same".into(),
        },
        TeamEventCursor {
            ts: 45,
            plan_id: plan.id.clone(),
            event_id: "stored-0".into(),
        },
        TeamEventCursor {
            ts: u64::MAX,
            plan_id: plan.id.clone(),
            event_id: "highest".into(),
        },
    ] {
        assert_eq!(
            store
                .team_mission_control_projection("selected", 2, Some(&cursor))
                .unwrap(),
            vec![expected_page(&plan, 2, Some(&cursor))]
        );
    }
    assert!(store
        .team_mission_control_projection("absent", 2, None)
        .unwrap()
        .is_empty());
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.goal.vector',42) WHERE id=?1",
            [&foreign.id],
        )
        .unwrap();
    assert!(store
        .team_mission_control_projection("selected", 2, None)
        .is_ok());
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.goal.vector',42) WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(store
        .team_mission_control_projection("selected", 2, None)
        .is_err());
    assert!(store.plan_loads_for_test().is_empty());
}

#[test]
fn team_sql_filters_before_materializing_unrequested_history_and_bodies() {
    let mut prior = None;
    for (strides, attempts, extra, history, rounds) in [
        (0, 1, 0, 0, 0),
        (2048, 1, 0, 0, 0),
        (0, 2048, 0, 0, 0),
        (0, 1, 32768, 0, 0),
        (0, 1, 0, 2048, 0),
        (0, 1, 0, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        team_metadata(&mut plan);
        for number in 0..205 {
            plan.strides.push(serde_json::from_value(json!({"id":format!("visible-{number}"),"kind":"decision","text":format!("Line {number}\n{}","x".repeat(extra)),"from":{"tab_id":"display"},"ts":1000+number})).unwrap());
        }
        plan.goal
            .as_mut()
            .unwrap()
            .vision
            .push_str(&"x".repeat(extra));
        plan.tasks[0].body.push_str(&"x".repeat(extra));
        for number in 0..history {
            plan.tasks[0].milestones.push(serde_json::from_value(json!({"id":format!("old-{number}"),"kind":"note","text":"unselected old milestone","from":{},"ts":1})).unwrap());
        }
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                // Old round rows sit below the selected event window, as header history did.
                .map(|number| crate::plan::CuratorRound {
                    number,
                    ended_at: 1,
                    ..Default::default()
                })
                .collect(),
        ));
        let expected = expected_page(&plan, 2, None);
        let database = fixtures::database(directory.path(), &plan);
        let mut foreign = plan.clone();
        foreign.id = "foreign".into();
        foreign.team_id = Some("other".into());
        foreign.goal.as_mut().unwrap().vector = "x".repeat(extra);
        database.persist_plan_rows(&foreign, None).unwrap();
        let store = PlanStore::open(database.path()).unwrap();
        fixtures::audit(&database);
        crate::service_plan::store_sync::take_decoded_row_bytes();
        database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = crate::service_plan::store_sync::take_decoded_row_bytes();
        take_metrics();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let actual = store
            .team_mission_control_projection("selected", 2, None)
            .unwrap();
        assert_eq!(actual, vec![expected]);
        assert!(store.plan_loads_for_test().is_empty());
        let response = serde_json::to_vec(&json!([
            actual[0].metadata,
            actual[0].task_counts,
            actual[0].lifecycle_events,
            actual[0].stored_events
        ]))
        .unwrap()
        .len();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 4);
        assert!(metrics.materialized_bytes < 8192);
        assert!(decoded < 8192);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=team_board tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} unselected_target_history={history} rounds={rounds} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn team_summary_and_events_share_a_committed_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    plan.tasks.truncate(1);
    team_metadata(&mut plan);
    plan.title = "A".into();
    plan.lifecycle_events.truncate(1);
    plan.lifecycle_events[0].kind = "A".into();
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let path = database.path();
    let id = plan.id.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..16 {
            let name = if index % 2 == 0 { "B" } else { "A" };
            let transaction = connection.transaction().unwrap();
            transaction.execute("UPDATE plans SET title=?2,header_json=json_set(header_json,'$.title',?2,'$.lifecycle_events[0].kind',?2) WHERE id=?1",params![id,name]).unwrap();
            writer_barrier.wait();
            transaction.commit().unwrap();
            writer_barrier.wait();
        }
    });
    let mut coherent = true;
    for _ in 0..16 {
        barrier.wait();
        let result = store.team_mission_control_projection("selected", 2, None);
        barrier.wait();
        coherent &= result.is_ok_and(|plans| {
            plans.len() == 1
                && plans[0].lifecycle_events.len() == 1
                && plans[0].metadata.title == plans[0].lifecycle_events[0].1.kind
        });
    }
    writer.join().unwrap();
    assert!(
        coherent,
        "Team metadata and lifecycle event must share one commit"
    );
}

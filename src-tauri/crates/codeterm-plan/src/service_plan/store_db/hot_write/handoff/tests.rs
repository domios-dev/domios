use super::super::tests::fixtures::{database, fixture};
use super::*;
use crate::service_plan::handoff::HandoffResumeSend;
use serde_json::json;
use std::sync::{Arc, Barrier};

fn element() -> CanvasElement {
    CanvasElement {
        id: "handoff".into(),
        kind: "handoff".into(),
        canvas: Default::default(),
        links: vec![],
        content: json!({"seq":1,"reseed_state":"new_sent","resume_prompt":"resume","reseed_pending":{
            "state":"new_sent","tab_id":"owner","source_session_id":"old","resume_prompt":"resume"
        }}),
    }
}

#[test]
fn handoff_writes_one_element_independent_of_unrelated_history_and_roll_back() {
    let mut previous = None;
    for (strides, attempts) in [(0, 0), (2048, 0), (0, 2048)] {
        let dir = tempfile::tempdir().unwrap();
        let mut plan = fixture(dir.path(), strides, attempts);
        plan.elements.push(element());
        let db = database(dir.path(), &plan);
        let (_, receipt) = db
            .edit_handoff_row(&plan.id, Some("1"), None, false, 20, |rows| {
                assert_eq!(rows.elements.len(), 1);
                rows.elements[0].content["reseed_state"] = json!("session_seen");
                Ok(())
            })
            .unwrap();
        assert_eq!(
            (
                receipt.element_rows,
                receipt.task_rows,
                receipt.attempts,
                receipt.strides
            ),
            (1, 0, 0, 0)
        );
        let measured = (
            receipt.decoded_bytes,
            receipt.serialized_bytes,
            receipt.hashed_bytes,
            receipt.element_rows,
            receipt.header_rows,
        );
        if let Some(previous) = previous {
            assert_eq!(previous, measured);
        }
        previous = Some(measured);
        let before = db.load_plan(&plan.id).unwrap().unwrap();
        db.connection.lock().execute_batch("CREATE TRIGGER refuse_runtime_header BEFORE UPDATE ON plans BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").unwrap();
        assert!(db
            .edit_handoff_row(&plan.id, Some("handoff"), None, false, 21, |rows| {
                rows.elements[0].content["reseed_state"] = json!("resume_sent");
                Ok(())
            })
            .is_err());
        assert_eq!(db.load_plan(&plan.id).unwrap().unwrap(), before);
    }
}

#[test]
fn handoff_resume_is_claimed_once_without_whole_plan_loads() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(PlanStore::new_for_test(dir.path()));
    let mut plan = crate::plan::Plan::new(
        "handoff".into(),
        dir.path().display().to_string(),
        Default::default(),
    );
    plan.elements.push(element());
    store.save(&mut plan, 1).unwrap();
    store.reset_plan_loads_for_test();
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .observe_handoff_session_start("owner", "new", 20)
                    .unwrap()
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<Option<HandoffResumeSend>> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_some()).count(), 1);
    store
        .mark_handoff_resume_sent(&plan.id, "handoff", 21)
        .unwrap();
    store.ack_handoff_resume(&plan.id, "handoff", 22).unwrap();
    assert!(store.plan_loads_for_test().is_empty());
}

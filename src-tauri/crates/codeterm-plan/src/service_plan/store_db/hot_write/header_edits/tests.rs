use super::*;
use crate::plan::{EffortLinkRel, Plan, TaskStatus};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::{load_audit, PlanStore};

#[test]
fn header_family_rows_and_bytes_ignore_unrelated_growth() {
    let mut prior = None;
    for (strides, attempts, body_bytes) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"u".repeat(body_bytes));
        let database = fixtures::database(root.path(), &plan);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&database);
        drop(database);
        let store = PlanStore::open(root.path().join("plans.db")).unwrap();
        fixtures::audit(&store.database);
        load_audit::start(&store);
        let mut work = Vec::new();
        store
            .set_planner_ref(
                &plan.id,
                Some(PlannerRef::Tab {
                    tab_id: "planner".into(),
                    session_id: None,
                    title: None,
                }),
                20,
            )
            .unwrap();
        work.push(super::super::take_work());
        store.set_mode(&plan.id, PlanMode::Paused, 21).unwrap();
        work.push(super::super::take_work());
        store.set_mode(&plan.id, PlanMode::Planning, 22).unwrap();
        work.push(super::super::take_work());
        store
            .set_planning_state(
                &plan.id,
                PlanningState::Approved,
                PlanningAuthor::User,
                Some("approved"),
                23,
            )
            .unwrap();
        work.push(super::super::take_work());
        store
            .set_kinds(
                &plan.id,
                vec![MilestoneKind {
                    key: "decision".into(),
                    label: "Decision".into(),
                    color: None,
                }],
                24,
            )
            .unwrap();
        work.push(super::super::take_work());
        store
            .set_task_groups(
                &plan.id,
                vec![TaskGroup {
                    key: "group".into(),
                    label: "Group".into(),
                    color: None,
                }],
                25,
            )
            .unwrap();
        work.push(super::super::take_work());
        store
            .set_tag_facets(
                &plan.id,
                vec![TagFacet {
                    key: "work".into(),
                    ..Default::default()
                }],
                26,
            )
            .unwrap();
        work.push(super::super::take_work());
        store.link_plan(&plan.id, "linked", 27).unwrap();
        work.push(super::super::take_work());
        store.unlink_plan(&plan.id, "linked", 28).unwrap();
        work.push(super::super::take_work());
        store
            .link_effort(&plan.id, "predecessor", EffortLinkRel::Follows, 29)
            .unwrap();
        work.push(super::super::take_work());
        store
            .transfer(&plan.id, OrchestratorRef::from_tab("next"), "handover", 30)
            .unwrap();
        work.push(super::super::take_work());
        assert!(store
            .refresh_owner_session(&plan.id, "next", Some("session"), Some("title"), 31)
            .unwrap());
        work.push(super::super::take_work());
        println!("fixture={} tasks={} base_content_bytes={} operation=header_family strides={strides} unrelated_attempts={attempts} unrelated_body_bytes={body_bytes} changed_rows={:?} work_task_header_attempt_milestone_stride_decoded_serialized_projected_hashed={work:?}", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, fixtures::audit_counts(&store.database));
        assert!(store.plan_loads_for_test().is_empty());
        for entry in &work {
            assert_eq!((entry[0], entry[2], entry[3], entry[4]), (0, 0, 0, 0));
            assert!(entry[5] < 1024 && entry[6] < 1024);
        }
        if let Some(prior) = prior {
            assert_eq!(prior, work);
        }
        prior = Some(work);
        crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(
            &store.database,
        );
        drop(store);
        let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
        let stored = reopened.load_plan(&plan.id).unwrap().unwrap();
        assert_eq!(stored.tasks, plan.tasks);
        assert_eq!(stored.strides, plan.strides);
        assert_eq!(stored.owner.tab_id.as_deref(), Some("next"));
        assert_eq!(stored.owner.session_id.as_deref(), Some("session"));
        assert_eq!(stored.owner_history.len(), 1);
        assert_eq!(stored.owner_history[0].from, plan.owner);
        assert_eq!(stored.planning_state, PlanningState::Approved);
        assert_eq!(stored.mode, PlanMode::Approved);
        assert!(stored.paused_from_mode.is_none());
        assert_eq!(stored.task_groups[0].key, "group");
        assert_eq!(stored.tag_facets[0].key, "work");
        assert!(stored.links.is_empty());
        assert_eq!(stored.lineage[0].to, "predecessor");
    }
}

#[test]
fn header_owner_denial_and_failed_multi_field_commit_preserve_durable_authority() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "header".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.tasks
        .push(load_audit::task("task", TaskStatus::Pending, None));
    drop(fixtures::database(root.path(), &plan));
    let store = PlanStore::open(root.path().join("plans.db")).unwrap();
    assert!(!store
        .refresh_owner_session(&plan.id, "foreign", Some("session"), None, 20)
        .unwrap());
    assert_eq!(store.database.load_plan(&plan.id).unwrap().unwrap(), plan);
    store.database.pending_changes.lock().take(&plan.id);
    store.database.connection.lock().execute_batch("CREATE TRIGGER reject_header_revision BEFORE UPDATE ON plans WHEN json_extract(NEW.header_json,'$._revision') != coalesce(json_extract(OLD.header_json,'$._revision'),0) BEGIN SELECT RAISE(ABORT,'injected header commit failure'); END;").unwrap();
    assert!(store
        .transfer(
            &plan.id,
            OrchestratorRef::from_tab("foreign"),
            "rejected",
            21
        )
        .unwrap_err()
        .to_string()
        .contains("injected header commit failure"));
    assert!(store.take_committed_change(&plan.id).is_none());
    crate::service_plan::store_db::hot_write::tests::fixtures::remove_observers(&store.database);
    drop(store);
    let reopened = PlanDatabase::open(root.path().join("plans.db")).unwrap();
    assert_eq!(reopened.load_plan(&plan.id).unwrap().unwrap(), plan);
}

#[test]
fn header_appends_preserve_existing_history_without_decoding_it() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "header".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.owner_history = (0..2048)
        .map(|index| TransferRecord {
            from: OrchestratorRef::from_tab("previous"),
            to: OrchestratorRef::from_tab("owner"),
            reason: format!("history {index}"),
            ts: index,
        })
        .collect();
    let database = fixtures::database(root.path(), &plan);
    let (_, receipt) = database
        .change_header(
            &plan.id,
            HeaderChange::Owner {
                to: OrchestratorRef::from_tab("next"),
                reason: "handover".into(),
            },
            3000,
        )
        .unwrap();
    println!("operation=owner_transfer unrelated_owner_history=2048 receipt={receipt:?}");
    assert!(receipt.decoded_bytes < 1024 && receipt.serialized_bytes < 1024);
    let stored = database.load_plan(&plan.id).unwrap().unwrap();
    assert_eq!(&stored.owner_history[..2048], plan.owner_history.as_slice());
    assert_eq!(
        stored.owner_history[2048].to.tab_id.as_deref(),
        Some("next")
    );
}

use super::*;
use crate::plan::{OrchestratorRef, Plan};
use crate::service_plan::store_db::hot_write::tests::fixtures;
use crate::service_plan::PlanStore;
use std::sync::{Arc, Barrier};

mod agent_transition_tests;
mod attachment_charter_tests;
mod auxiliary_tests;
mod binding_context_tests;
mod binding_tests;
mod bundle_tests;
mod chat_header_tests;
mod close_inventory_tests;
mod curator_digest_tests;
mod curator_lifecycle_tests;
mod curator_notice_tests;
mod curator_schedule_tests;
mod document_reader_tests;
mod export_tests;
mod handoff_reader_tests;
mod header_tests;
mod heartbeat_display_tests;
mod heartbeat_owner_tests;
mod heartbeat_projection_tests;
mod hierarchy_routing_tests;
mod hierarchy_tests;
mod index_filter_tests;
mod lane_reader_tests;
mod lane_team_state_tests;
mod launch_restore_tests;
mod notice_tests;
mod qa_tests;
mod reference_resolver_tests;
mod related_tests;
mod reminder_digest_tests;
mod reminder_gate_tests;
mod reminder_target_tests;
mod reminder_tests;
mod role_binding_tests;
mod runtime_identity_tests;
mod search_tests;
mod section_link_tests;
mod selector_error_tests;
mod status_tests;
mod tab_tests;
mod task_key_reader_tests;
mod team_close_tests;
mod team_mutation_tests;
mod transfer_authority_tests;
mod worker_read_tests;

fn sections() -> Vec<Section> {
    serde_json::from_value(json!([
        {"id":"sec_a","title":"Root","body":"root prose","subsections":[
            {"id":"sec_b","handle":"nested","title":"Nested","body":"selected prose","subsections":[
                {"id":"sec_c","title":"Leaf","body":"leaf prose"}
            ]},
            {"id":"sec_d","title":"Sibling","body":"sibling prose"}
        ]},
        {"id":"sec_e","title":"Other","body":"other prose"}
    ]))
    .unwrap()
}

#[test]
fn section_selection_preserves_tree_order_and_subtree_output() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Outline".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.sections = sections();
    let database = fixtures::database(directory.path(), &plan);
    let outline = database.section_outline(&plan.id).unwrap();
    assert_eq!(
        outline,
        json!({"id":plan.id,"title":"Outline","sections":[
            {"id":"sec_a","title":"Root","subsections":[
                {"id":"sec_b","title":"Nested","subsections":[{"id":"sec_c","title":"Leaf","subsections":[]}]},
                {"id":"sec_d","title":"Sibling","subsections":[]}
            ]}, {"id":"sec_e","title":"Other","subsections":[]}
        ]})
    );
    assert_eq!(
        database.selected_section(&plan.id, "nested").unwrap(),
        plan.sections[0].subsections[0]
    );
    assert_eq!(
        database.selected_section(&plan.id, "sec_b").unwrap(),
        plan.sections[0].subsections[0]
    );
    assert!(matches!(
        database.selected_section(&plan.id, "absent"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.section_outline("absent"),
        Err(PlanError::NotFound)
    ));
}

#[test]
fn section_and_reference_reads_do_not_materialize_unrelated_body_or_history() {
    let mut previous = None;
    for (strides, attempts, extra_body) in [(0, 0, 0), (2048, 0, 0), (0, 2048, 0), (0, 0, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.sections = sections();
        plan.sections[1].body = "x".repeat(extra_body);
        plan.tasks[1].body.push_str(&"x".repeat(extra_body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::store_sync::take_decoded_row_bytes();
        let baseline = database.load_plan(&plan.id).unwrap().unwrap();
        let baseline_decoded = super::super::super::store_sync::take_decoded_row_bytes();
        let baseline_serialized = serde_json::to_vec(&baseline).unwrap().len();
        assert!(baseline_decoded > 31 * 1024 * 1024);
        println!("baseline=whole_plan fixture={} strides={strides} unrelated_attempts={attempts} unrelated_extra_body={extra_body} decoded_json_bytes={baseline_decoded} response_bytes={baseline_serialized}", fixtures::SEED);
        drop(baseline);
        take_metrics();
        super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::store_rows::take_serialized_row_bytes();
        let outline = database.section_outline(&plan.id).unwrap();
        let selected = database.selected_section(&plan.id, "nested").unwrap();
        let references = database
            .resolve_task_reference(Some(&plan.id), "task_0000")
            .unwrap();
        let section_refs = database.scoped_section_references(&plan.id).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::store_rows::take_serialized_row_bytes();
        let response_bytes = serde_json::to_vec(&(outline, selected)).unwrap().len();
        assert_eq!(references.len(), 1);
        assert_eq!(section_refs.len(), 5);
        assert_eq!(metrics.queries, 5);
        assert!(metrics.materialized_bytes < 100_000);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} tasks={} base_bytes={} strides={strides} unrelated_attempts={attempts} unrelated_extra_body={extra_body} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, metrics.queries, metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn scoped_resolvers_keep_case_precedence_ambiguity_and_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let mut plan = Plan::new(
        "References".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.sections = sections();
    let mut first =
        crate::service_plan::load_audit::task("task_alpha", crate::plan::TaskStatus::Pending, None);
    first.handle = Some("named".into());
    first.key = Some("COD-1".into());
    let mut second = first.clone();
    second.id = "task_alpine".into();
    second.handle = Some("COD-1".into());
    second.key = Some("COD-2".into());
    plan.tasks = vec![first, second];
    store.save(&mut plan, 1).unwrap();
    crate::service_plan::load_audit::start(&store);
    assert_eq!(
        store
            .resolve_task_ref(Some(&plan.id), "NAMED")
            .unwrap()
            .task_id,
        "task_alpha"
    );
    assert!(store.resolve_task_ref(Some(&plan.id), "task_al").is_err());
    assert!(store.resolve_task_ref(Some(&plan.id), "cod-1").is_err());
    assert_eq!(
        store
            .task_display_key(&plan.id, "task_alpha")
            .unwrap()
            .as_deref(),
        Some("COD-1")
    );
    assert!(store
        .task_display_key(&plan.id, "task_al")
        .unwrap()
        .is_none());
    assert!(store.task_display_key(&plan.id, "cod-1").unwrap().is_none());
    assert_eq!(
        store
            .resolve_section_ref(Some(&plan.id), "nested")
            .unwrap()
            .section_id,
        "sec_b"
    );
    assert!(store.plan_loads_for_test().is_empty());
    store.delete(&plan.id).unwrap();
    assert!(store.resolve_task_ref(Some(&plan.id), "NAMED").is_err());
    assert!(store.show_section(&plan.id, "nested").is_err());
}

#[test]
fn section_outline_reads_one_committed_snapshot_during_concurrent_commits() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.sections = serde_json::from_value(json!([{"id":"sec_a","title":"A"}])).unwrap();
    let database = Arc::new(fixtures::database(directory.path(), &plan));
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let path = database.path();
    let id = plan.id.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..32 {
            let title = if index % 2 == 0 { "B" } else { "A" };
            let transaction = connection.transaction().unwrap();
            transaction
                .execute(
                    "UPDATE plans SET title = ?2 WHERE id = ?1",
                    params![id, title],
                )
                .unwrap();
            transaction.execute("UPDATE plan_sections SET row_json = json_set(row_json, '$.title', ?2) WHERE plan_id = ?1", params![id, title]).unwrap();
            writer_barrier.wait();
            transaction.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..32 {
        barrier.wait();
        let outline = database.section_outline(&plan.id).unwrap();
        assert_eq!(outline["title"], outline["sections"][0]["title"]);
        barrier.wait();
    }
    writer.join().unwrap();
    assert_eq!(database.section_outline(&plan.id).unwrap()["title"], "A");
}

#[test]
fn task_detail_and_list_preserve_filters_graph_context_and_history_windows() {
    use crate::plan::TaskStatus;
    use crate::service_plan::task_queries::{TaskListEmptyReason, TaskListFilter};
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let mut plan = Plan::new(
        "Tasks".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.sections = sections();
    let mut selected =
        crate::service_plan::load_audit::task("task_selected", TaskStatus::Pending, Some("worker"));
    selected.body = "selected body".into();
    selected.lane.branch = Some("feature/selected".into());
    selected.lane.workspace_id = Some("workspace".into());
    selected.parent_task_id = Some("task_parent".into());
    selected.depends_on = vec!["task_done".into(), "task_missing".into()];
    selected.section_ids = vec!["sec_b".into()];
    selected.milestones = (0..75)
        .map(|i| {
            serde_json::from_value(json!({"id":format!("m{i}"),"kind":"note","ts":i,"from":{}}))
                .unwrap()
        })
        .collect();
    selected.execution_attempts = (0..20)
        .map(|i| crate::service_plan::load_audit::attempt(&format!("a{i}"), "old"))
        .collect();
    let mut sibling =
        crate::service_plan::load_audit::task("task_sibling", TaskStatus::Blocked, None);
    sibling.parent_task_id = Some("task_parent".into());
    plan.tasks = vec![
        crate::service_plan::load_audit::task("task_parent", TaskStatus::InProgress, None),
        selected,
        sibling,
        crate::service_plan::load_audit::task("task_done", TaskStatus::Done, None),
    ];
    plan.tasks[1].interruption.owner_epoch = 7;
    plan.tasks[1].interruption.owner_tab_id = Some("worker".into());
    store.save(&mut plan, 1).unwrap();
    crate::service_plan::load_audit::start(&store);
    let context = store
        .database
        .task_detail_projection("task_selected")
        .expect("SQL task context decodes")
        .unwrap();
    assert_eq!(context.task.interruption.owner_epoch, 7);
    let detail = store.show_task_window("task_selected").unwrap();
    assert_eq!(detail.task.body, "selected body");
    assert_eq!(detail.parent.unwrap().id, "task_parent");
    assert_eq!(
        detail
            .siblings
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        vec!["task_sibling"]
    );
    assert_eq!(detail.sections[0].title, "Nested");
    assert_eq!(detail.dependency_waits.len(), 1);
    assert!(detail.dependency_waits[0].missing);
    assert_eq!(detail.dependency_waits[0].task_id, "task_missing");
    assert_eq!(
        detail.presentation,
        crate::service_plan::state::TaskPresentation::WaitingOnDependencies
    );
    assert_eq!(detail.task.milestones.len(), 50);
    assert_eq!(detail.task.milestones.first().unwrap().id, "m25");
    assert_eq!(detail.task.milestones.last().unwrap().id, "m74");
    assert_eq!(detail.task.execution_attempts.len(), 8);
    assert_eq!(detail.task.execution_attempts.first().unwrap().id, "a12");
    let authority_detail = store.inspect_task_for_test("task_selected").unwrap();
    assert_eq!(authority_detail.task.interruption.owner_epoch, 7);
    assert_eq!(
        authority_detail.task.interruption.owner_tab_id.as_deref(),
        Some("worker")
    );
    assert_eq!(authority_detail.task.execution_attempts.len(), 20);
    assert_eq!(authority_detail.task.milestones.len(), 75);
    crate::service_plan::load_audit::start(&store);
    let mine = store
        .list_tasks_with_reason(TaskListFilter {
            plan_id: Some(plan.id.clone()),
            mine_tabs: ["worker".into()].into(),
            active_only: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(mine.tasks.len(), 1);
    assert_eq!(mine.tasks[0].task.id, "task_selected");
    assert_eq!(mine.tasks[0].section_titles, vec!["Nested"]);
    assert_eq!(
        mine.tasks[0].task.lane.branch.as_deref(),
        Some("feature/selected")
    );
    assert_eq!(
        mine.tasks[0].task.lane.workspace_id.as_deref(),
        Some("workspace")
    );
    let inactive = store
        .list_tasks_with_reason(TaskListFilter {
            plan_id: Some(plan.id.clone()),
            active_only: true,
            status: Some(TaskStatus::Done),
            ..Default::default()
        })
        .unwrap();
    assert!(matches!(
        inactive.empty_reason,
        Some(TaskListEmptyReason::AllTasksInactive { hidden: 1 })
    ));
    assert!(store.plan_loads_for_test().is_empty());
    store.database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.interruption',json('{\"handoff_required\":false}')) WHERE task_id='task_selected'",[]).unwrap();
    assert!(matches!(
        store.show_task_window("task_selected"),
        Err(PlanError::Quarantined { .. })
    ));
    let listed = store
        .list_tasks_with_reason(TaskListFilter {
            plan_id: Some(plan.id.clone()),
            ..Default::default()
        })
        .expect("list does not read interruption authority fields");
    assert_eq!(listed.tasks.len(), 4);
    assert!(listed
        .tasks
        .iter()
        .any(|summary| summary.task.id == "task_selected"));
    assert!(store.plan_loads_for_test().is_empty());
    store.database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',NULL) WHERE task_id='task_selected'",[]).unwrap();
    assert!(matches!(
        store.list_tasks(TaskListFilter {
            plan_id: Some(plan.id.clone()),
            ..Default::default()
        }),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn selected_task_list_detail_and_chat_materialization_stays_bounded() {
    use crate::service_plan::task_queries::TaskListFilter;
    let mut previous = std::collections::HashMap::new();
    for (strides, attempts, extra_body, selected_body, selected_attempts, selected_milestones) in [
        (0, 0, 0, 0, 0, 0),
        (2048, 0, 0, 0, 0, 0),
        (0, 2048, 0, 0, 0, 0),
        (0, 0, 32768, 0, 0, 0),
        (0, 0, 0, 32768, 0, 0),
        (0, 0, 0, 0, 2048, 0),
        (0, 0, 0, 0, 0, 2048),
    ] {
        let list_only = selected_body > 0 || selected_attempts > 0 || selected_milestones > 0;
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].assigned_tab_id = Some("selected".into());
        plan.tasks[1].body.push_str(&"x".repeat(extra_body));
        plan.tasks[0].body.push_str(&"x".repeat(selected_body));
        for index in 0..selected_attempts {
            let mut attempt =
                crate::service_plan::load_audit::attempt(&format!("selected-{index}"), "old");
            attempt.closed_at = Some(1);
            plan.tasks[0].execution_attempts.push(attempt);
        }
        plan.tasks[0].milestones.extend((0..selected_milestones).map(|index|
            serde_json::from_value::<crate::plan::Milestone>(json!({"id":format!("selected-{index}"),"from":{},"kind":"note","text":"unselected milestone","ts":1})).unwrap()
        ));
        let database = fixtures::database(directory.path(), &plan);
        let store = PlanStore::open(database.path()).unwrap();
        fixtures::audit(&database);
        for consumer in ["task_detail", "task_list", "chat"] {
            if list_only && consumer != "task_list" {
                continue;
            }
            take_metrics();
            super::super::super::store_sync::take_decoded_row_bytes();
            super::super::super::store_rows::take_serialized_row_bytes();
            let response = match consumer {
                "task_detail" => {
                    serde_json::to_value(store.show_task_window("task_0000").unwrap()).unwrap()
                }
                "task_list" => serde_json::to_value(
                    store
                        .list_tasks_with_reason(TaskListFilter {
                            plan_id: Some(plan.id.clone()),
                            mine_tabs: ["selected".into()].into(),
                            ..Default::default()
                        })
                        .unwrap(),
                )
                .unwrap(),
                _ => serde_json::to_value(
                    store.chat_snapshot(None, None, Some("task_0000")).unwrap(),
                )
                .unwrap(),
            };
            let metrics = take_metrics();
            let decoded = super::super::super::store_sync::take_decoded_row_bytes();
            let serialized = super::super::super::store_rows::take_serialized_row_bytes();
            let response_bytes = serde_json::to_vec(&response).unwrap().len();
            assert!(metrics.materialized_bytes < 1024 * 1024);
            if consumer == "task_list" {
                assert!(metrics.materialized_bytes < 8192);
                assert!(decoded < 8192);
                assert_eq!(metrics.queries, 1);
            }
            assert_eq!(serialized, 0);
            assert!(fixtures::audit_counts(&database).is_empty());
            println!("fixture={} consumer={consumer} tasks={} base_bytes={} strides={strides} unrelated_attempts={attempts} extra_body={extra_body} selected_body={selected_body} selected_attempts={selected_attempts} selected_milestones={selected_milestones} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
            let measured = (metrics, decoded, response_bytes);
            if let Some(prior) = previous.get(consumer) {
                assert_eq!(&measured, prior);
            } else {
                previous.insert(consumer, measured);
            }
        }
        assert!(store.plan_loads_for_test().is_empty());
    }
}

#[test]
fn task_detail_and_chat_keep_committed_snapshots_and_unicode_search() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let mut task =
        crate::service_plan::load_audit::task("task_one", crate::plan::TaskStatus::Pending, None);
    task.body = "A".into();
    task.name = "ÄTHER".into();
    task.lane.branch = Some("A".into());
    plan.tasks = vec![task];
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
    let chat = store.chat_snapshot(None, None, Some("äther")).unwrap();
    assert_eq!(chat[0]["tasks"][0]["id"], "task_one");
    assert_eq!(chat[0]["tasks"][0]["body"], "");
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
            let text = if index % 2 == 0 { "B" } else { "A" };
            let transaction = connection.transaction().unwrap();
            transaction
                .execute("UPDATE plans SET title=?2 WHERE id=?1", params![id, text])
                .unwrap();
            transaction.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.body',?2,'$.lane.branch',?2) WHERE plan_id=?1",params![id,text]).unwrap();
            writer_barrier.wait();
            transaction.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let detail = store.show_task_window("task_one").unwrap();
        assert_eq!(detail.plan_title, detail.task.body);
        let list = store
            .list_tasks(crate::service_plan::task_queries::TaskListFilter {
                plan_id: Some(plan.id.clone()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            list[0].task.lane.branch.as_deref(),
            Some(list[0].plan_title.as_str())
        );
        barrier.wait();
    }
    writer.join().unwrap();
}

mod task_authority_tests;

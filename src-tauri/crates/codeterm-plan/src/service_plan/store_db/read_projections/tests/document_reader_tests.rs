use super::*;
mod manifest_tag_tests;
mod regression_cleanup_tests;
mod retrospective_tests;

fn decorate(plan: &mut Plan) {
    plan.sections = sections();
    plan.loops = serde_json::from_value(json!([
        {"id":"selected","target":"task_0000","budget":5,"count":2,"status":"running","stagnation":0},
        {"id":"unselected","target":"task_0001","budget":5,"count":1,"status":"running","stagnation":0}
    ])).unwrap();
}

#[test]
fn document_markdown_and_loop_reads_preserve_output_and_selection_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut first = Plan::new(
        "First".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    first.id = "pl_first".into();
    first.updated_at = 2;
    decorate(&mut first);
    let database = fixtures::database(directory.path(), &first);
    let store = PlanStore::open(database.path()).unwrap();
    assert_eq!(
        store.spec_markdown(&first.id).unwrap(),
        crate::service_plan::body_files::spec_markdown(&first)
    );
    assert_eq!(store.loop_status("selected").unwrap(), first.loops[0]);
    assert_eq!(
        store.loop_plan_id("selected").unwrap(),
        Some(first.id.clone())
    );
    let metadata = store.document_metadata(&first.id).unwrap();
    assert_eq!(metadata.id, first.id);
    assert_eq!(metadata.project_path, first.project_path);
    assert!(store.loop_plan_id("missing").unwrap().is_none());
    assert!(matches!(
        store.loop_status("missing"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        store.spec_markdown("missing"),
        Err(PlanError::NotFound)
    ));
    let mut second = first.clone();
    second.id = "pl_second".into();
    second.updated_at = 3;
    second.loops[0].budget = 9;
    database.persist_plan_rows(&second, None).unwrap();
    assert_eq!(
        store.loop_plan_id("selected").unwrap(),
        Some(second.id.clone())
    );
    assert_eq!(store.loop_status("selected").unwrap().budget, 9);
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.loops[0].budget','invalid') WHERE id=?1",[&second.id]).unwrap();
    assert!(matches!(
        store.loop_status("selected"),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(store.plan_loads_for_test().is_empty());
}

#[test]
fn document_and_loop_reads_ignore_task_bodies_attempts_strides_and_unselected_loops() {
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
        decorate(&mut plan);
        plan.tasks[0].body.push_str(&"x".repeat(extra));
        plan.loops[1].deviation = Some("x".repeat(extra));
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| crate::plan::CuratorRound {
                    number,
                    ended_at: number,
                    ..Default::default()
                })
                .collect(),
        ));
        for index in 0..history {
            let mut attempt =
                crate::service_plan::load_audit::attempt(&format!("closed-{index}"), "other");
            attempt.closed_at = Some(1);
            plan.tasks[0].execution_attempts.push(attempt);
        }
        let database = fixtures::database(directory.path(), &plan);
        let store = PlanStore::open(database.path()).unwrap();
        fixtures::audit(&database);
        crate::service_plan::store_sync::take_decoded_row_bytes();
        database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = crate::service_plan::store_sync::take_decoded_row_bytes();
        take_metrics();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let markdown = store.spec_markdown(&plan.id).unwrap();
        let loop_spec = store.loop_status("selected").unwrap();
        let loop_plan = store.loop_plan_id("selected").unwrap();
        let metadata = store.document_metadata(&plan.id).unwrap();
        assert_eq!(
            markdown,
            crate::service_plan::body_files::spec_markdown(&plan)
        );
        assert_eq!(loop_spec, plan.loops[0]);
        assert_eq!(metadata.status, plan.status);
        assert_eq!(metadata.canonical_project_path, plan.canonical_project_path);
        assert!(store.plan_loads_for_test().is_empty());
        let response_bytes = serde_json::to_vec(&json!([markdown, loop_spec, loop_plan]))
            .unwrap()
            .len();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 5);
        assert!(metrics.materialized_bytes < 8192);
        assert!(decoded < 8192);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=document_loop tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} unselected_target_history={history} rounds={rounds} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

fn coverage_spec(plan: &mut Plan) -> crate::plan::Spec {
    use crate::plan::{Spec, SpecLink, TaskStatus};
    plan.sections = sections();
    plan.tasks[0].section_ids = vec!["sec_a".into(), "sec_a".into()];
    plan.tasks[0].status = TaskStatus::Done;
    plan.tasks[1].section_ids = vec!["sec_b".into()];
    plan.tasks[1].status = TaskStatus::InProgress;
    let mut spec = Spec::new("Coverage".into(), plan.project_path.clone());
    spec.id = "sp_coverage".into();
    spec.sections = serde_json::from_value(json!([
        {"id":"req","title":"Requirement","body":"spec body","subsections":[
            {"id":"unplanned","title":"Unplanned","body":"unrequested"}
        ]}
    ]))
    .unwrap();
    plan.spec_refs = vec![spec.id.clone()];
    plan.spec_links = ["sec_a", "sec_a", "sec_missing"]
        .into_iter()
        .map(|id| SpecLink {
            spec_id: spec.id.clone(),
            spec_section_id: "req".into(),
            plan_section_id: id.into(),
        })
        .collect();
    spec
}

#[test]
fn coverage_and_spec_links_preserve_duplicates_nested_order_archives_and_selected_decode() {
    use crate::plan::coverage::{
        compute_coverage_report, compute_spec_coverage_report, PlanCoverageInput,
    };
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    let spec = coverage_spec(&mut plan);
    let mut archived = plan.clone();
    archived.id = "pl_archived".into();
    archived.status = crate::plan::PlanStatus::Archived;
    archived.updated_at = plan.updated_at + 1;
    archived.spec_links.truncate(1);
    let database = fixtures::database(directory.path(), &plan);
    database.persist_spec(&spec).unwrap();
    database.persist_plan_rows(&archived, None).unwrap();
    let store = PlanStore::open(database.path()).unwrap();
    assert_eq!(
        store.coverage_report(&plan.id).unwrap().unwrap(),
        compute_coverage_report(&plan)
    );
    assert!(store.coverage_report(&archived.id).unwrap().is_none());
    assert!(store.coverage_report("missing").unwrap().is_none());
    assert!(store.spec_coverage_report("missing").unwrap().is_none());
    assert_eq!(
        store.spec_coverage_report(&spec.id).unwrap().unwrap(),
        compute_spec_coverage_report(
            &spec,
            &[
                PlanCoverageInput::from(&archived),
                PlanCoverageInput::from(&plan)
            ]
        )
    );
    let expected = archived
        .spec_links
        .iter()
        .map(|link| (archived.id.clone(), link.clone()))
        .chain(
            plan.spec_links
                .iter()
                .map(|link| (plan.id.clone(), link.clone())),
        )
        .collect::<Vec<_>>();
    assert_eq!(store.list_spec_links(&spec.id).unwrap(), expected);
    assert!(store.list_spec_links("missing").unwrap().is_empty());
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.status','invalid') WHERE plan_id=?1 AND task_id=?2",params![plan.id,plan.tasks[2].id]).unwrap();
    assert!(store.spec_coverage_report(&spec.id).unwrap().is_some());
    assert!(matches!(
        store.coverage_report(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(store.plan_loads_for_test().is_empty());
}

#[test]
fn coverage_and_spec_links_ignore_bodies_attempts_strides_history_and_rounds() {
    use crate::plan::coverage::{
        compute_coverage_report, compute_spec_coverage_report, PlanCoverageInput,
    };
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
        let mut spec = coverage_spec(&mut plan);
        plan.tasks[0].body.push_str(&"x".repeat(extra));
        plan.sections[0].body.push_str(&"x".repeat(extra));
        spec.sections[0].body.push_str(&"x".repeat(extra));
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| crate::plan::CuratorRound {
                    number,
                    ended_at: number,
                    ..Default::default()
                })
                .collect(),
        ));
        for index in 0..history {
            plan.tasks[0].milestones.push(serde_json::from_value(json!({"id":format!("history-{index}"),"from":{},"kind":"note","text":"unrequested history","ts":1})).unwrap());
        }
        let database = fixtures::database(directory.path(), &plan);
        database.persist_spec(&spec).unwrap();
        let store = PlanStore::open(database.path()).unwrap();
        fixtures::audit(&database);
        crate::service_plan::store_sync::take_decoded_row_bytes();
        database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = crate::service_plan::store_sync::take_decoded_row_bytes();
        take_metrics();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let coverage = store.coverage_report(&plan.id).unwrap().unwrap();
        let spec_coverage = store.spec_coverage_report(&spec.id).unwrap().unwrap();
        let links = store.list_spec_links(&spec.id).unwrap();
        assert_eq!(coverage, compute_coverage_report(&plan));
        assert_eq!(
            spec_coverage,
            compute_spec_coverage_report(&spec, &[PlanCoverageInput::from(&plan)])
        );
        assert!(store.plan_loads_for_test().is_empty());
        let response_bytes = serde_json::to_vec(&json!([coverage, spec_coverage, links]))
            .unwrap()
            .len();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 7);
        assert!(metrics.materialized_bytes < 262144);
        assert!(decoded < 262144);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=coverage_spec_link tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} unselected_target_history={history} rounds={rounds} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn spec_coverage_reads_section_metadata_and_task_status_in_one_commit() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    let mut spec = coverage_spec(&mut plan);
    spec.sections[0].title = "A".into();
    let database = fixtures::database(directory.path(), &plan);
    database.persist_spec(&spec).unwrap();
    let store = PlanStore::open(database.path()).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let path = database.path();
    let plan_id = plan.id.clone();
    let task_id = plan.tasks[0].id.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..16 {
            let (title, status) = if index % 2 == 0 {
                ("B", "pending")
            } else {
                ("A", "done")
            };
            let tx = connection.transaction().unwrap();
            tx.execute(
                "UPDATE specs SET row_json=json_set(row_json,'$.sections[0].title',?2) WHERE id=?1",
                params![spec.id, title],
            )
            .unwrap();
            tx.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.status',?3) WHERE plan_id=?1 AND task_id=?2",params![plan_id,task_id,status]).unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let report = store.spec_coverage_report("sp_coverage").unwrap().unwrap();
        assert_eq!(
            report.sections[0].status,
            if report.sections[0].title == "A" {
                "done"
            } else {
                "pending"
            }
        );
        barrier.wait();
    }
    writer.join().unwrap();
}

fn gate_goal_plan(plan: &mut Plan) {
    plan.planner = Some(crate::plan::PlannerRef::Tab {
        tab_id: "planner".into(),
        session_id: Some("planner-session".into()),
        title: Some("Planner".into()),
    });
    plan.goal = Some(crate::plan::Goal {
        vector: "Ship selected readers".into(),
        vision: "Requested goal prose".into(),
        constraints: vec!["Keep authority".into()],
        postulates: vec!["SQL is authoritative".into()],
    });
    plan.tasks[0].kind = "planning".into();
    plan.tasks[0].handle = Some(String::new());
    plan.tasks[0].assigned_tab_id = Some("worker".into());
    plan.tasks[1].kind = "planning".into();
    plan.tasks[1].assigned_tab_id = Some("other".into());
    plan.planner_requests = serde_json::from_value(json!([
        {"request_id":"chosen","payload":"selected request","state":"queued","created_at":1},
        {"request_id":"old","payload":"unselected request","state":"completed","created_at":1}
    ]))
    .unwrap();
    plan.planner_inbox = serde_json::from_value(json!([
        {"request_id":"chosen","phase":"delivery_uncertain","tab_id":"planner","session_id":"planner-session","binding_nonce":"exact-nonce","transport":"pty_agent","updated_at":1},
        {"request_id":"old","phase":"planner_acked","updated_at":1}
    ])).unwrap();
}

#[test]
fn gate_goal_and_planner_nudge_select_requested_fields_and_keep_routing_decisions() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    gate_goal_plan(&mut plan);
    let database = fixtures::database(directory.path(), &plan);
    let store = PlanStore::open(database.path()).unwrap();
    let mut read = store
        .planner_gate_context(&plan.id, Some("chosen"))
        .unwrap()
        .unwrap();
    assert_eq!(read.owner, plan.owner);
    assert_eq!(read.planner, plan.planner);
    assert_eq!(read.request.as_ref(), plan.planner_requests.first());
    assert_eq!(read.delivery.as_ref(), plan.planner_inbox.first());
    assert_eq!(read.owner_for_gate(Some("planner")), Some("owner"));
    assert_eq!(read.owner_for_gate(Some("worker")), None);
    assert_eq!(read.owner_for_gate(None), None);
    read.owner.tab_id = None;
    assert_eq!(read.owner_for_gate(Some("planner")), None);
    assert!(store
        .planner_gate_context("missing", None)
        .unwrap()
        .is_none());
    assert!(store
        .planner_gate_context(&plan.id, Some("missing"))
        .unwrap()
        .unwrap()
        .request
        .is_none());
    assert_eq!(store.goal_document(&plan.id).unwrap(), plan.goal);
    assert!(matches!(
        store.goal_document("missing"),
        Err(PlanError::NotFound)
    ));
    for caller in [None, Some("worker"), Some("other")] {
        assert_eq!(
            store.planner_nudge_projection(&plan.id, caller).unwrap(),
            crate::service_plan::hints::planner_ownership_nudge(&plan, caller)
        );
    }
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.planner_requests[1].payload',42,'$.goal',NULL) WHERE id=?1",[&plan.id]).unwrap();
    assert!(store
        .planner_gate_context(&plan.id, Some("chosen"))
        .unwrap()
        .is_some());
    assert!(store.goal_document(&plan.id).unwrap().is_none());
    assert!(matches!(
        store.planner_gate_context(&plan.id, Some("old")),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(store.plan_loads_for_test().is_empty());
}

#[test]
fn gate_goal_and_nudge_reads_ignore_unselected_payloads_task_bodies_and_history() {
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
        gate_goal_plan(&mut plan);
        plan.tasks[0].body.push_str(&"x".repeat(extra));
        plan.tasks[42].name.push_str(&"x".repeat(extra));
        plan.planner_requests[1]
            .payload
            .push_str(&"x".repeat(extra));
        for number in 0..history {
            let mut request = plan.planner_requests[1].clone();
            request.request_id = format!("unselected-{number}");
            plan.planner_requests.push(request);
            let mut delivery = plan.planner_inbox[1].clone();
            delivery.request_id = format!("unselected-{number}");
            plan.planner_inbox.push(delivery);
        }
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| crate::plan::CuratorRound {
                    number,
                    ended_at: number,
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
        take_metrics();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let initial = store.planner_gate_context(&plan.id, None).unwrap().unwrap();
        let selected = store
            .planner_gate_context(&plan.id, Some("chosen"))
            .unwrap()
            .unwrap();
        let goal = store.goal_document(&plan.id).unwrap();
        let nudge = store
            .planner_nudge_projection(&plan.id, Some("worker"))
            .unwrap();
        assert!(initial.request.is_none());
        assert!(initial.delivery.is_none());
        assert_eq!(selected.request.as_ref(), plan.planner_requests.first());
        assert_eq!(selected.delivery.as_ref(), plan.planner_inbox.first());
        assert_eq!(goal, plan.goal);
        assert_eq!(
            nudge,
            crate::service_plan::hints::planner_ownership_nudge(&plan, Some("worker"))
        );
        assert!(store.plan_loads_for_test().is_empty());
        let response_bytes = serde_json::to_vec(&json!([
            initial.owner,
            initial.planner,
            selected.request,
            selected.delivery,
            goal,
            nudge
        ]))
        .unwrap()
        .len();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 4);
        assert!(metrics.materialized_bytes < 16384);
        assert!(decoded < 16384);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=gate_goal_nudge tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} unselected_target_history={history} rounds={rounds} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn index_fixture_capture() {
    use sha2::{Digest, Sha256};
    let directory = tempfile::tempdir().unwrap();
    crate::service_plan::paths::mark_test_project_root(directory.path());
    let mut plan = fixtures::fixture(directory.path(), 0, 1);
    decorate(&mut plan);
    plan.goal = Some(crate::plan::Goal {
        vector: "Capture index".into(),
        vision: "Goal prose".into(),
        constraints: Vec::new(),
        postulates: Vec::new(),
    });
    plan.tasks.truncate(3);
    for task in &mut plan.tasks {
        task.body.clear();
    }
    plan.tasks[0].milestones=serde_json::from_value(json!([
        {"id":"capture-milestone","from":{},"kind":"decision","text":"long notes ".repeat(30),"ts":1}
    ])).unwrap();
    plan.elements = serde_json::from_value(json!([
        {"id":"capture-note","kind":"note","content":"note prose ".repeat(30)},
        {"id":"capture-handoff","kind":"handoff","content":{"seq":7}}
    ]))
    .unwrap();
    crate::service_plan::body_files::persist_bodies(&plan).unwrap();
    let bytes = std::fs::read(crate::service_plan::body_files::index_path(&plan).unwrap()).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let content = String::from_utf8(bytes.clone()).unwrap();
    assert!(content.contains("capture-note"));
    assert!(content.contains("capture-handoff"));
    assert!(content.contains("capture-milestone"));
    println!(
        "fixture={} artifact=INDEX.md bytes={} sha256={} content={}",
        fixtures::SEED,
        bytes.len(),
        hash,
        serde_json::to_string(&content).unwrap()
    );
}

mod team_reader_tests;

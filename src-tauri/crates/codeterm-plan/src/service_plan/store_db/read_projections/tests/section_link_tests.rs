use super::*;
use crate::plan::TaskStatus;
use crate::service_plan::section_links::{
    resolve_selectors, section_link_preview, suggest_section,
};

fn decorate(plan: &mut Plan) {
    plan.sections=serde_json::from_value(json!([
        {"id":"sec_root","title":"Root","handle":"overview","tags":["intro"],"body":"unneeded prose","subsections":[
            {"id":"sec_api","title":"API","handle":"network","tags":["http"],"body":"nested prose"}
        ]},
        {"id":"sec_ui","title":"User Interface","handle":"ui","tags":["screen"]},
        {"id":"sec_duplicate","title":"API"}
    ])).unwrap();
    for task in plan.tasks.iter_mut().skip(3) {
        task.section_ids = vec!["sec_root".into()]
    }
    plan.tasks[0].group = Some("http".into());
    plan.tasks[0].status = TaskStatus::Done;
    plan.tasks[1].handle = Some("screen".into());
    plan.tasks[2].name = "API surfaces".into();
}

fn response(database: &super::super::super::PlanDatabase, full: &Plan) -> serde_json::Value {
    let selectors = ["HTTP", "ui", "INTRO", "api", "unknown"].map(str::to_string);
    let references = database.section_link_projection(&full.id, false).unwrap();
    assert!(references.tasks.is_empty());
    let ids = resolve_selectors(&references, &selectors);
    assert_eq!(ids, resolve_selectors(full, &selectors));
    assert_eq!(ids, vec!["sec_api", "sec_ui", "sec_root"]);
    let suggestion = suggest_section(&references, &full.tasks[0]);
    assert_eq!(suggestion, suggest_section(full, &full.tasks[0]));
    let preview = section_link_preview(&database.section_link_projection(&full.id, true).unwrap());
    assert_eq!(preview, section_link_preview(full));
    assert_eq!(preview.unlinked_count, 3);
    assert_eq!(preview.suggestions.len(), 2);
    json!([ids, suggestion, preview])
}

#[test]
fn section_link_metadata_preserves_nested_keys_ambiguity_terminal_tasks_and_preview_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Links".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = (0..4)
        .map(|i| crate::service_plan::load_audit::task(&format!("t{i}"), TaskStatus::Pending, None))
        .collect();
    decorate(&mut plan);
    let database = fixtures::database(directory.path(), &plan);
    response(&database, &plan);
    assert!(matches!(
        database.section_link_projection("absent", true),
        Err(PlanError::NotFound)
    ));
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',NULL) WHERE plan_id=?1 AND task_id='t0'",[&plan.id]).unwrap();
    assert!(matches!(
        database.section_link_projection(&plan.id, true),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(database.section_link_projection(&plan.id, false).is_ok());
    database.connection.lock().execute("UPDATE plan_sections SET row_json=json_set(row_json,'$.tags',NULL) WHERE plan_id=?1 AND position=0",[&plan.id]).unwrap();
    assert!(matches!(
        database.section_link_projection(&plan.id, false),
        Err(PlanError::Quarantined { .. })
    ));
}

#[test]
fn section_link_reads_ignore_linked_task_metadata_and_all_unrelated_prose_and_history() {
    let mut prior = None;
    for (strides, attempts, extra) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        decorate(&mut plan);
        plan.tasks[3].body.push_str(&"x".repeat(extra));
        plan.tasks[3].name.push_str(&"x".repeat(extra));
        plan.sections[0].body.push_str(&"x".repeat(extra));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        let full = database.load_plan(&plan.id).unwrap().unwrap();
        let baseline = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let output = response(&database, &full);
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        let response_bytes = serde_json::to_vec(&output).unwrap().len();
        assert_eq!(metrics.queries, 5);
        assert!(metrics.materialized_bytes < 8192);
        assert!(decoded < 8192);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=section_links tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra} baseline_decoded_json_bytes={baseline} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response_bytes} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response_bytes);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior)
        }
        prior = Some(measured);
    }
}

#[test]
fn section_link_preview_keeps_selected_task_and_section_labels_in_one_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Links".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.sections = serde_json::from_value(json!([{"id":"sec_a","title":"A"}])).unwrap();
    let mut task = crate::service_plan::load_audit::task("task", TaskStatus::Pending, None);
    task.name = "A".into();
    task.group = Some("sec_a".into());
    plan.tasks = vec![task];
    let database = fixtures::database(directory.path(), &plan);
    let path = database.path();
    let id = plan.id.clone();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for index in 0..16 {
            let title = if index % 2 == 0 { "B" } else { "A" };
            let tx = connection.transaction().unwrap();
            tx.execute("UPDATE plan_sections SET row_json=json_set(row_json,'$.title',?2) WHERE plan_id=?1",params![id,title]).unwrap();
            tx.execute(
                "UPDATE plan_tasks SET row_json=json_set(row_json,'$.name',?2) WHERE plan_id=?1",
                params![id, title],
            )
            .unwrap();
            writer_barrier.wait();
            tx.commit().unwrap();
            writer_barrier.wait();
        }
    });
    for _ in 0..16 {
        barrier.wait();
        let projected = database.section_link_projection(&plan.id, true).unwrap();
        let preview = section_link_preview(&projected);
        assert_eq!(
            preview.suggestions[0].task_name,
            preview.suggestions[0].section_title
        );
        barrier.wait();
    }
    writer.join().unwrap();
}

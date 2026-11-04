use super::*;
use crate::service_plan::store_db::hot_write::tests::fixtures;

#[test]
fn list_windows_search_status_and_cursor_remain_scoped_read_only() {
    let root = tempfile::tempdir().unwrap();
    let mut plan = fixtures::fixture(root.path(), 0, 0);
    plan.tasks[1100].name = "Äther needle".into();
    plan.tasks[1100].status = crate::plan::TaskStatus::Blocked;
    plan.sections = serde_json::from_value(json!([
        {"id":"b","title":"Second","body":"Unselected section prose"},
        {"id":"a","title":"First"}, {"id":"c","title":"Third"}
    ]))
    .unwrap();
    plan.tasks[1100].section_ids = vec!["c".into(), "b".into(), "a".into()];
    let database = fixtures::database(root.path(), &plan);
    fixtures::audit(&database);
    let first = database
        .ui_plan_list_window(&plan.id, &Default::default())
        .unwrap();
    assert_eq!(first["tasks"].as_array().unwrap().len(), 25);
    assert_eq!(first["task_window"]["total"], 1162);
    let cursor = &first["task_window"]["next_cursor"];
    let second = database
        .ui_plan_list_window(
            &plan.id,
            &ListWindowQuery {
                after_position: cursor["position"].as_i64(),
                after_task_id: cursor["task_id"].as_str().map(str::to_string),
                ..Default::default()
            },
        )
        .unwrap();
    let ids: std::collections::HashSet<_> = first["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["id"].clone().to_string())
        .collect();
    assert!(second["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|task| !ids.contains(&task["id"].to_string())));
    let found = database
        .ui_plan_list_window(
            &plan.id,
            &ListWindowQuery {
                search: Some("äTHER".into()),
                status: Some("blocked".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(found["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(found["tasks"][0]["id"], "task_1100");
    let section_phrase = database
        .ui_plan_list_window(
            &plan.id,
            &ListWindowQuery {
                search: Some("First Second".into()),
                status: Some("blocked".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(section_phrase["tasks"][0]["id"], "task_1100");
    assert_eq!(section_phrase["task_window"]["total"], 1);
    let exact = database
        .ui_plan_list_window(
            &plan.id,
            &ListWindowQuery {
                task_ref: Some("fixture-1100".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(exact["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(exact["tasks"][0]["id"], "task_1100");
    assert!(database
        .ui_plan_list_window("missing", &Default::default())
        .is_err());
    assert!(database
        .ui_plan_list_window(
            &plan.id,
            &ListWindowQuery {
                after_position: Some(1),
                ..Default::default()
            }
        )
        .is_err());
    assert!(fixtures::audit_counts(&database).is_empty());
}

#[test]
fn board_list_and_chat_materialize_windows_instead_of_whole_task_and_history_collections() {
    for (extra_tasks, strides, attempts) in [(0, 0, 0), (0, 2048, 2048), (1162, 2048, 2048)] {
        let root = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(root.path(), strides, attempts);
        let unrelated = std::mem::take(&mut plan.tasks[1].execution_attempts);
        plan.tasks.last_mut().unwrap().execution_attempts = unrelated;
        for index in 0..extra_tasks {
            let mut task = plan.tasks[10].clone();
            task.id = format!("extra-{index}");
            task.handle = None;
            plan.tasks.push(task);
        }
        let database = fixtures::database(root.path(), &plan);
        fixtures::audit(&database);
        for surface in ["list", "chat", "board-state", "board-summary"] {
            super::super::read_projections::take_metrics();
            crate::service_plan::store_sync::take_decoded_row_bytes();
            crate::service_plan::store_rows::take_serialized_row_bytes();
            let value = match surface {
                "list" => database
                    .ui_plan_list_window(&plan.id, &Default::default())
                    .unwrap(),
                "chat" => json!(database
                    .chat_projection(None, None, None, Some(&plan.id), None)
                    .unwrap()),
                _ => {
                    database
                        .board_state_projection(&plan.id, 100, 0, surface == "board-state")
                        .unwrap()
                        .value
                }
            };
            let bytes = serde_json::to_vec(&value).unwrap().len();
            let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
            let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
            let metrics = super::super::read_projections::take_metrics();
            assert!(bytes < 65536, "{surface} bytes={bytes}");
            assert!(decoded < 65536, "{surface} decoded={decoded}");
            assert!(
                metrics.materialized_bytes < 65536,
                "{surface} materialized={}",
                metrics.materialized_bytes
            );
            assert_eq!(serialized, 0);
            if surface == "list" {
                assert_eq!(value["tasks"].as_array().unwrap().len(), 25);
            }
            if surface == "chat" {
                assert_eq!(value[0]["tasks"].as_array().unwrap().len(), 20);
            }
            if surface == "board-state" {
                assert!(
                    value["mission_control"]["event_log"]["entries"]
                        .as_array()
                        .unwrap()
                        .len()
                        <= 50
                );
                assert!(value.get("recent_strides").is_none());
            }
            assert!(fixtures::audit_counts(&database).is_empty());
            println!("fixture={} base_bytes={} tasks={} extra_tasks={extra_tasks} strides={strides} unrelated_attempts={attempts} surface={surface} payload_bytes={bytes} decoded_json_bytes={decoded} serialized_row_bytes={serialized} materialized_text_bytes={} changed_rows=0",fixtures::SEED,fixtures::CONTENT_BYTES,plan.tasks.len(),metrics.materialized_bytes);
        }
    }
}

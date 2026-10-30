use super::*;
use crate::plan::OrchestratorRef;
use crate::service_plan::PlanStore;
use std::sync::{mpsc, Barrier};
use std::time::Duration;

fn share_queue(queue: Arc<IndexQueue>) {
    TEST_INDEX_QUEUE.with(|slot| *slot.borrow_mut() = queue);
}

fn seed_racing_mutation(action: &str) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let store = Arc::new(PlanStore::new_for_test(root));
    let queue = index_queue();
    *queue.state.lock() = IndexQueueState::default();
    let mut plan = Plan::new(
        "Before".into(),
        root.display().to_string(),
        OrchestratorRef::default(),
    );
    store.save(&mut plan, 1).unwrap();
    let mut spec = Spec::new("Retained spec".into(), root.display().to_string());
    spec.sections = serde_json::from_value(serde_json::json!([
        {"id": "sec_seed", "title": "Before section", "body": "Before body"}
    ]))
    .unwrap();
    store.save_spec(&mut spec, 1).unwrap();
    assert!(!project_index_path(root).exists());

    let release = Arc::new(Barrier::new(2));
    let (snapshot_tx, snapshot_rx) = mpsc::channel();
    let seed_store = store.clone();
    let seed_queue = queue.clone();
    let seed_release = release.clone();
    let seeder = std::thread::spawn(move || {
        share_queue(seed_queue);
        seed_store.seed_missing_search_indexes_observed(|_| {
            snapshot_tx.send(()).unwrap();
            seed_release.wait();
        });
    });
    snapshot_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let admitted_before = store.mutation_writer.state.lock().next_ticket;
    let mutation_store = store.clone();
    let mutation_queue = queue.clone();
    let action = action.to_string();
    let mutation = std::thread::spawn(move || {
        share_queue(mutation_queue);
        match action.as_str() {
            "update" => {
                mutation_store
                    .update_plan(&plan.id, Some("After"), None, None, 2)
                    .unwrap();
            }
            "delete" => mutation_store.delete(&plan.id).unwrap(),
            "create" => {
                let mut created = Plan::new(
                    "Created".into(),
                    plan.project_path.clone(),
                    OrchestratorRef::default(),
                );
                mutation_store.save(&mut created, 2).unwrap();
            }
            "spec_section" => {
                mutation_store
                    .update_spec_section(
                        &spec.id,
                        "sec_seed",
                        None,
                        Some("After section"),
                        &[],
                        None,
                        None,
                        2,
                    )
                    .unwrap();
            }
            "spec_add" => {
                mutation_store
                    .add_spec_section(
                        &spec.id,
                        "New section",
                        "New body",
                        None,
                        &[],
                        None,
                        None,
                        2,
                    )
                    .unwrap();
            }
            "plan_task" => {
                mutation_store
                    .add_task(
                        &plan.id,
                        crate::service_plan::NewTask {
                            name: "New task".into(),
                            ..Default::default()
                        },
                        2,
                    )
                    .unwrap();
            }
            "spec_delete" => mutation_store.delete_spec(&spec.id, 2).unwrap(),
            "from_spec" => {
                mutation_store
                    .create_plan_from_spec_for_team(
                        &spec.id,
                        "Derived".into(),
                        OrchestratorRef::default(),
                        "derived-team".into(),
                        vec![],
                        2,
                    )
                    .unwrap();
            }
            "batch" => {
                use crate::service_plan::batch::{BatchOp, BatchRequest};
                mutation_store
                    .apply_batch(
                        &plan.id,
                        BatchRequest {
                            key: None,
                            ops: vec![
                                BatchOp::SetSpecSectionBody {
                                    spec: spec.id.clone(),
                                    section: "sec_seed".into(),
                                    body: "Batch body".into(),
                                },
                                BatchOp::CreatePlanFromSpec {
                                    client_id: None,
                                    spec: spec.id,
                                    title: "Batch derived".into(),
                                },
                            ],
                        },
                        2,
                    )
                    .unwrap();
            }
            "loaded_plan" => {
                let mut loaded = plan;
                loaded.title = "Loaded commit".into();
                loaded.updated_at = 2;
                mutation_store.commit_loaded_plan(loaded).unwrap();
            }
            _ => unreachable!(),
        }
    });
    {
        let mut state = store.mutation_writer.state.lock();
        while state.next_ticket == admitted_before {
            assert!(!store
                .mutation_writer
                .ready
                .wait_for(&mut state, Duration::from_secs(5))
                .timed_out());
        }
        assert!(
            state.serving_ticket < admitted_before,
            "the mutation waits behind the seed snapshot"
        );
    }
    release.wait();
    seeder.join().unwrap();
    mutation.join().unwrap();
    flush_index_queue_for_test(root);

    let scope = project_scope(root);
    let expected = store
        .database
        .load_plans_for_test()
        .iter()
        .flat_map(|plan| plan_units(plan, &scope))
        .chain(
            store
                .database
                .load_specs()
                .unwrap()
                .iter()
                .flat_map(|spec| spec_units(spec, &scope)),
        )
        .map(|unit| (unit.id.clone(), unit_signature(&unit)))
        .collect::<HashMap<_, _>>();
    let connection = open_index(root).unwrap();
    assert_eq!(
        indexed_units_like(&connection, "planunit_%").unwrap(),
        expected
    );
}

#[test]
fn seed_racing_update_persists_current_sql_rows() {
    seed_racing_mutation("update");
}

#[test]
fn seed_racing_delete_persists_current_sql_rows() {
    seed_racing_mutation("delete");
}

#[test]
fn seed_racing_create_persists_current_sql_rows() {
    seed_racing_mutation("create");
}

#[test]
fn seed_racing_public_spec_section_update_persists_current_sql_rows() {
    seed_racing_mutation("spec_section");
}

#[test]
fn seed_racing_public_spec_delete_persists_current_sql_rows() {
    seed_racing_mutation("spec_delete");
}

#[test]
fn seed_racing_create_plan_from_spec_loaded_commit_persists_current_sql_rows() {
    seed_racing_mutation("from_spec");
}

#[test]
fn seed_racing_batch_loaded_commits_persists_current_sql_rows() {
    seed_racing_mutation("batch");
}

#[test]
fn seed_racing_loaded_plan_commit_persists_current_sql_rows() {
    seed_racing_mutation("loaded_plan");
}

#[test]
fn seed_racing_public_spec_section_create_persists_current_sql_rows() {
    seed_racing_mutation("spec_add");
}

#[test]
fn seed_racing_public_task_create_persists_current_sql_rows() {
    seed_racing_mutation("plan_task");
}

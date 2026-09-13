use super::ports::{TaskKeyPort, TaskKeyScope};
use super::resolve::ResolveError;
use super::task_queries::TaskListFilter;
use super::{NewTask, PlanStore};
use crate::plan::task_key::{self, TaskRefMatch, WorkspaceTaskKeys};
use crate::plan::{OrchestratorRef, Task};
use parking_lot::Mutex;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

/// Stands in for one workspace record: the durable key state lives here, not in the plan.
struct WorkspacePort {
    name: String,
    taken: HashSet<String>,
    keys: Mutex<WorkspaceTaskKeys>,
}

impl WorkspacePort {
    fn bind(store: &PlanStore, name: &str, keys: WorkspaceTaskKeys) -> Arc<Self> {
        let port = Arc::new(Self {
            name: name.into(),
            taken: HashSet::new(),
            keys: Mutex::new(keys),
        });
        store.bind_task_key_port(port.clone());
        port
    }
}

impl TaskKeyPort for WorkspacePort {
    fn issue_task_keys(
        &self,
        _scope: &TaskKeyScope,
        count: usize,
        high_water: &HashMap<String, u64>,
    ) -> Vec<String> {
        let mut keys = self.keys.lock();
        (0..count)
            .map(|_| keys.issue(&self.name, &self.taken, high_water))
            .collect()
    }
}

fn new_plan(store: &PlanStore, dir: &Path) -> String {
    store
        .create_plan(
            "Keys".into(),
            dir.display().to_string(),
            OrchestratorRef::from_tab("orch"),
            vec![],
            1,
        )
        .unwrap()
        .id
}

fn add(store: &PlanStore, plan_id: &str, name: &str) -> String {
    store
        .add_task(
            plan_id,
            NewTask {
                name: name.into(),
                kind: "task".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap()
}

fn stored_key(store: &PlanStore, plan_id: &str, task_id: &str) -> Option<String> {
    store
        .get(plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap()
        .key
}

fn legacy_task(id: &str) -> Task {
    serde_json::from_value(json!({
        "id": id,
        "name": "Legacy",
        "type": "task",
        "model": null,
        "model_rationale": null,
        "test_plan": null,
        "correctness": null,
        "status": "pending",
        "status_label": null,
        "intent": null,
        "assigned_tab_id": null,
        "peer": null
    }))
    .unwrap()
}

#[test]
fn task_key_contract_prefix_derivation_is_deterministic_and_collision_free() {
    let none = HashSet::new();
    assert_eq!(task_key::assign_prefix("CodeTerm", &none), "COD");
    assert_eq!(
        task_key::assign_prefix("CodeTerm", &none),
        task_key::assign_prefix("CodeTerm", &none)
    );
    assert_eq!(task_key::assign_prefix("code term dev", &none), "CTD");
    assert_eq!(task_key::assign_prefix("x", &none), "XX");
    assert_eq!(task_key::assign_prefix("---", &none), "WS");

    let taken: HashSet<String> = ["COD".to_string()].into();
    assert_eq!(task_key::assign_prefix("Codex", &taken), "CO");
    assert_eq!(
        task_key::assign_prefix("Codex", &taken),
        task_key::assign_prefix("Codex", &taken)
    );

    for name in ["CodeTerm", "a", "Web App 2 Mobile Extra", "---"] {
        for candidate in task_key::prefix_candidates(name) {
            assert!((2..=4).contains(&candidate.len()), "{name}: {candidate}");
            assert!(
                candidate
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
                "{candidate}"
            );
        }
    }
    let crowded: HashSet<String> = task_key::prefix_candidates("CodeTerm")
        .into_iter()
        .collect();
    let fallback = task_key::assign_prefix("CodeTerm", &crowded);
    assert!(!crowded.contains(&fallback), "{fallback}");
    assert!(task_key::parse_key(&task_key::format_key(&fallback, 1)).is_some());
}

#[test]
fn task_key_contract_workspace_prefix_is_immutable_and_counter_only_grows() {
    let none = HashSet::new();
    let mut keys = WorkspaceTaskKeys::default();
    assert!(keys.is_unissued());
    assert_eq!(keys.issue("Alpha", &none, &HashMap::new()), "ALP-1");
    assert_eq!(
        keys.issue("Renamed Workspace", &none, &HashMap::new()),
        "ALP-2"
    );
    let above_counter = HashMap::from([("ALP".to_string(), 9)]);
    assert_eq!(keys.issue("Alpha", &none, &above_counter), "ALP-10");
    let below_counter = HashMap::from([("ALP".to_string(), 3)]);
    assert_eq!(keys.issue("Alpha", &none, &below_counter), "ALP-11");

    let persisted = serde_json::to_value(&keys).unwrap();
    assert_eq!(persisted, json!({ "prefix": "ALP", "next": 12 }));
    let restored: WorkspaceTaskKeys = serde_json::from_value(persisted).unwrap();
    assert_eq!(restored, keys);
    let legacy: WorkspaceTaskKeys = serde_json::from_value(json!({})).unwrap();
    assert!(legacy.is_unissued());
    assert_eq!(
        serde_json::to_value(WorkspaceTaskKeys::default()).unwrap(),
        json!({})
    );
}

#[test]
fn task_key_contract_tasks_keep_keys_across_restart_and_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan_id = new_plan(&store, dir.path());
    let unbound = add(&store, &plan_id, "Before binding");
    assert_eq!(stored_key(&store, &plan_id, &unbound), None);

    let port = WorkspacePort::bind(&store, "CodeTerm", WorkspaceTaskKeys::default());
    let first = add(&store, &plan_id, "First");
    let second = add(&store, &plan_id, "Second");
    assert!(first.starts_with("task_"), "{first}");
    assert_eq!(
        stored_key(&store, &plan_id, &first).as_deref(),
        Some("COD-1")
    );
    assert_eq!(
        stored_key(&store, &plan_id, &second).as_deref(),
        Some("COD-2")
    );

    store.rm_task(&plan_id, &second, 3).unwrap();
    let third = add(&store, &plan_id, "Third");
    assert_eq!(
        stored_key(&store, &plan_id, &third).as_deref(),
        Some("COD-3")
    );

    let persisted_workspace = serde_json::to_string(&*port.keys.lock()).unwrap();
    let database = store.database_path_for_test();
    drop(store);
    let store = PlanStore::open(database).unwrap();
    assert_eq!(
        stored_key(&store, &plan_id, &first).as_deref(),
        Some("COD-1")
    );
    assert_eq!(
        stored_key(&store, &plan_id, &third).as_deref(),
        Some("COD-3")
    );
    assert_eq!(stored_key(&store, &plan_id, &unbound), None);

    WorkspacePort::bind(
        &store,
        "CodeTerm",
        serde_json::from_str(&persisted_workspace).unwrap(),
    );
    let fourth = add(&store, &plan_id, "Fourth");
    assert_eq!(
        stored_key(&store, &plan_id, &fourth).as_deref(),
        Some("COD-4")
    );
    assert_eq!(
        stored_key(&store, &plan_id, &first).as_deref(),
        Some("COD-1")
    );

    WorkspacePort::bind(&store, "CodeTerm", WorkspaceTaskKeys::default());
    let fifth = add(&store, &plan_id, "Fifth");
    assert_eq!(
        stored_key(&store, &plan_id, &fifth).as_deref(),
        Some("COD-5"),
        "a recreated workspace record resumes above every stored key"
    );
}

#[test]
fn task_key_contract_legacy_task_is_untouched_and_falls_back_to_its_id() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan_id = new_plan(&store, dir.path());
    let mut plan = store.get(&plan_id).unwrap();
    plan.tasks.push(legacy_task("task_cafe12"));
    store.save(&mut plan, 3).unwrap();

    WorkspacePort::bind(&store, "CodeTerm", WorkspaceTaskKeys::default());
    let fresh = add(&store, &plan_id, "Fresh");

    let reloaded = store.get(&plan_id).unwrap();
    let legacy = reloaded
        .tasks
        .iter()
        .find(|task| task.id == "task_cafe12")
        .unwrap();
    assert_eq!(legacy.key, None);
    assert_eq!(legacy.display_key(), "CAFE12");
    assert!(serde_json::to_value(legacy).unwrap().get("key").is_none());
    assert_eq!(
        stored_key(&store, &plan_id, &fresh).as_deref(),
        Some("COD-1")
    );

    for reference in ["task_cafe12", "CAFE12", "cafe12"] {
        let resolved = store.resolve_task_ref(Some(&plan_id), reference).unwrap();
        assert_eq!(resolved.task_id, "task_cafe12", "{reference}");
    }
    assert_eq!(store.show_task("CAFE12").unwrap().display_key, "CAFE12");
}

#[test]
fn task_key_contract_every_task_reference_accepts_id_and_key_and_refuses_ambiguity() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan_id = new_plan(&store, dir.path());
    WorkspacePort::bind(&store, "Alpha", WorkspaceTaskKeys::default());
    let keyed = add(&store, &plan_id, "Keyed");
    assert_eq!(
        stored_key(&store, &plan_id, &keyed).as_deref(),
        Some("ALP-1")
    );

    for reference in [keyed.as_str(), "ALP-1", "alp-1"] {
        assert_eq!(
            store
                .resolve_task_ref(Some(&plan_id), reference)
                .unwrap()
                .task_id,
            keyed
        );
        assert_eq!(
            store.resolve_task_ref(None, reference).unwrap().task_id,
            keyed
        );
        assert_eq!(store.show_task(reference).unwrap().task.id, keyed);
    }

    let dependent = store
        .add_task(
            &plan_id,
            NewTask {
                name: "Dependent".into(),
                kind: "task".into(),
                depends_on: vec!["ALP-1".into()],
                ..Default::default()
            },
            4,
        )
        .unwrap();
    let plan = store.get(&plan_id).unwrap();
    let dependent_task = plan.tasks.iter().find(|task| task.id == dependent).unwrap();
    assert_eq!(dependent_task.depends_on, vec![keyed.clone()]);

    let keyed_task = plan
        .tasks
        .iter()
        .find(|task| task.id == keyed)
        .unwrap()
        .clone();
    let mut clash = legacy_task("task_clash1");
    clash.handle = Some("alp-1".into());
    let tasks = vec![keyed_task, clash.clone()];
    match task_key::match_task_ref(&tasks, "ALP-1") {
        TaskRefMatch::Ambiguous(hits) => assert_eq!(hits, vec![0, 1]),
        other => panic!("expected ambiguity, got {other:?}"),
    }
    assert_eq!(task_key::find_task_index(&tasks, "ALP-1"), None);
    assert_eq!(task_key::find_task_index(&tasks, "task_clash1"), Some(1));
    assert_eq!(task_key::find_task_index(&tasks, &keyed), Some(0));

    let mut plan = store.get(&plan_id).unwrap();
    plan.tasks.push(clash);
    store.save(&mut plan, 5).unwrap();
    let saved_handle = store
        .get(&plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == "task_clash1")
        .and_then(|task| task.handle);
    assert_eq!(saved_handle.as_deref(), Some("alp-1"));
    match store.resolve_task_ref(Some(&plan_id), "ALP-1") {
        Err(ResolveError::Ambiguous { candidates, .. }) => {
            assert_eq!(candidates.len(), 2);
            assert!(candidates.iter().any(|c| c.key.as_deref() == Some("ALP-1")));
        }
        other => panic!("expected ambiguity, got {other:?}"),
    }
    assert!(store.show_task("ALP-1").is_none());
    assert_eq!(
        store
            .resolve_task_ref(Some(&plan_id), &keyed)
            .unwrap()
            .task_id,
        keyed
    );
}

#[test]
fn task_key_contract_task_json_serializes_key_beside_id() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan_id = new_plan(&store, dir.path());
    WorkspacePort::bind(&store, "Alpha", WorkspaceTaskKeys::default());
    let keyed = add(&store, &plan_id, "Keyed");
    let mut plan = store.get(&plan_id).unwrap();
    plan.tasks.push(legacy_task("task_old001"));
    store.save(&mut plan, 5).unwrap();

    let listed = store.list_tasks(TaskListFilter {
        plan_id: Some(plan_id.clone()),
        ..Default::default()
    });
    let rows = serde_json::to_value(&listed).unwrap();
    let rows = rows.as_array().unwrap();
    let keyed_row = rows.iter().find(|row| row["task"]["id"] == keyed).unwrap();
    assert_eq!(keyed_row["display_key"], "ALP-1");
    assert_eq!(keyed_row["task"]["key"], "ALP-1");
    let legacy_row = rows
        .iter()
        .find(|row| row["task"]["id"] == "task_old001")
        .unwrap();
    assert_eq!(legacy_row["display_key"], "OLD001");
    assert!(legacy_row["task"].get("key").is_none());

    let detail = serde_json::to_value(store.show_task("ALP-1").unwrap()).unwrap();
    assert_eq!(detail["display_key"], "ALP-1");
    assert_eq!(detail["task"]["id"], keyed);
    assert_eq!(
        store.task_display_key(&plan_id, &keyed).as_deref(),
        Some("ALP-1")
    );
}

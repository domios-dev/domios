use super::*;
use crate::plan::{OrchestratorRef, Plan};
use crate::service_plan::NewTask;

#[test]
fn chat_snapshot_is_bounded_scoped_and_omits_task_bodies() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Fixture".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.team_id = Some("team".into());
    store.save(&mut plan, 1).unwrap();
    store
        .add_task(
            &plan.id,
            NewTask {
                name: "Needle".into(),
                body: "opaque large body".repeat(100),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let task = store.get(&plan.id).unwrap().tasks.remove(0);
    plan.tasks = (0..55)
        .map(|index| {
            let mut task = task.clone();
            task.id = format!("task_{index}");
            task.name = format!("Needle {index}");
            task
        })
        .collect();
    store.save(&mut plan, 3).unwrap();
    let result = store
        .chat_snapshot(None, Some("team"), Some("Needle"))
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0]["tasks"].as_array().unwrap().len(), 50);
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("opaque large body"));
    assert!(store
        .chat_snapshot(None, Some("foreign"), Some(""))
        .unwrap()
        .is_empty());
    assert!(store
        .chat_snapshot(Some("/other-project"), Some("team"), Some(""))
        .unwrap()
        .is_empty());
    assert!(store
        .chat_snapshot(None, Some("team"), Some("Absent"))
        .unwrap()
        .is_empty());
}

use super::*;
use crate::plan::{Goal, OrchestratorRef, Plan};

#[test]
fn goal_persists_and_reloads_from_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Goal Plan".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    store
        .set_goal(
            &plan.id,
            Goal {
                vector: "Ship the north-star".into(),
                vision: "Long markdown\n\n- detail".into(),
                constraints: vec!["backend only".into()],
                postulates: vec!["reread plan".into()],
            },
            2,
        )
        .unwrap();

    let goal_path = dir
        .path()
        .join(format!(".codeterm/plans/{}/goal.md", plan.id));
    let json_path = dir
        .path()
        .join(".codeterm/plans")
        .join(format!("{}.json", plan.id));
    assert_eq!(
        std::fs::read_to_string(&goal_path).unwrap(),
        "Long markdown\n\n- detail"
    );
    let raw_json = std::fs::read_to_string(&json_path).unwrap();
    assert!(raw_json.contains("\"goal\""));
    assert!(!raw_json.contains("Long markdown"));

    std::fs::write(&goal_path, "Edited vision").unwrap();
    store.reload_from_disk(&plan.id).unwrap();
    let loaded = store.get(&plan.id).unwrap();
    let goal = loaded.goal.unwrap();
    assert_eq!(goal.vector, "Ship the north-star");
    assert_eq!(goal.vision, "Edited vision");
    assert_eq!(goal.constraints, vec!["backend only"]);
    assert_eq!(goal.postulates, vec!["reread plan"]);
}

use super::*;
use crate::plan::{Goal, OrchestratorRef, Plan};

#[test]
fn goal_persists_and_reloads_from_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Goal Plan".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
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

    let plan_dir = super::store_v2::plan_state_dir(&store.get(&plan.id).unwrap()).unwrap();
    let goal_path = super::store_v2::goal_yaml_path(&plan_dir);
    let plan_path = super::store_v2::plan_yaml_path(&plan_dir);
    let goal_yaml: Goal =
        serde_yaml::from_str(&std::fs::read_to_string(&goal_path).unwrap()).unwrap();
    assert_eq!(goal_yaml.vector, "Ship the north-star");
    assert_eq!(goal_yaml.vision, "Long markdown\n\n- detail");
    let plan_yaml: Plan =
        serde_yaml::from_str(&std::fs::read_to_string(&plan_path).unwrap()).unwrap();
    assert!(
        plan_yaml.goal.is_none(),
        "goal lives in goal.yaml, not plan.yaml"
    );

    let mut edited_goal = goal_yaml;
    edited_goal.vision = "Edited vision".into();
    std::fs::write(&goal_path, serde_yaml::to_string(&edited_goal).unwrap()).unwrap();
    store.reload_from_disk(&plan.id).unwrap();
    let loaded = store.get(&plan.id).unwrap();
    let goal = loaded.goal.unwrap();
    assert_eq!(goal.vector, "Ship the north-star");
    assert_eq!(goal.vision, "Edited vision");
    assert_eq!(goal.constraints, vec!["backend only"]);
    assert_eq!(goal.postulates, vec!["reread plan"]);
}

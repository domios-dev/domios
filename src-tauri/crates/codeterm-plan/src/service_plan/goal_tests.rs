use super::mutations::goal_ops::GoalPatch;
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

fn goal_plan(dir: &std::path::Path) -> (PlanStore, String) {
    let store = PlanStore::new_for_test(dir);
    let mut plan = Plan::new(
        "Partial goal".into(),
        dir.display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    store
        .set_goal(
            &plan.id,
            Goal {
                vector: "the original vector".into(),
                vision: "the original vision".into(),
                constraints: vec!["backend only".into(), "no new deps".into()],
                postulates: vec!["reread the plan".into()],
            },
            2,
        )
        .unwrap();
    (store, plan.id)
}

/// The defect: a call naming one field destroyed every field it did not name,
/// so the operator's only protection was defensively re-passing what they had.
#[test]
fn goal_patch_keeps_every_field_it_was_not_passed() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id) = goal_plan(dir.path());

    let after_vector = store
        .patch_goal(
            &plan_id,
            GoalPatch {
                vector: Some("a sharpened vector".into()),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    assert_eq!(after_vector.vector, "a sharpened vector");
    assert_eq!(after_vector.vision, "the original vision");
    assert_eq!(after_vector.constraints, vec!["backend only", "no new deps"]);
    assert_eq!(after_vector.postulates, vec!["reread the plan"]);

    let after_constraint = store
        .patch_goal(
            &plan_id,
            GoalPatch {
                constraints: Some(vec!["one constraint only".into()]),
                ..Default::default()
            },
            4,
        )
        .unwrap();
    assert_eq!(
        after_constraint.vector, "a sharpened vector",
        "setting only --constraint must leave the vector standing"
    );
    assert_eq!(after_constraint.vision, "the original vision");
    assert_eq!(after_constraint.constraints, vec!["one constraint only"]);
    assert_eq!(after_constraint.postulates, vec!["reread the plan"]);

    // The store is the oracle, not the returned value.
    let reloaded = store.get(&plan_id).unwrap().goal.unwrap();
    assert_eq!(reloaded, after_constraint);
}

/// Replacement is reachable by naming every field, which is why no wholesale
/// flag exists; a list is cleared by passing it empty.
#[test]
fn goal_patch_replaces_only_what_it_names_and_clears_on_empty() {
    let dir = tempfile::tempdir().unwrap();
    let (store, plan_id) = goal_plan(dir.path());

    let cleared = store
        .patch_goal(
            &plan_id,
            GoalPatch {
                constraints: Some(Vec::new()),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    assert!(cleared.constraints.is_empty());
    assert_eq!(cleared.vector, "the original vector");

    let replaced = store
        .patch_goal(
            &plan_id,
            GoalPatch {
                vector: Some("v2".into()),
                vision: Some("s2".into()),
                constraints: Some(vec!["c2".into()]),
                postulates: Some(vec!["p2".into()]),
            },
            4,
        )
        .unwrap();
    assert_eq!(
        replaced,
        Goal {
            vector: "v2".into(),
            vision: "s2".into(),
            constraints: vec!["c2".into()],
            postulates: vec!["p2".into()],
        }
    );
}

/// A goal with no vector is not a goal. Asserted by error constant, and the
/// remedy field names the flag rather than describing it in prose.
#[test]
fn goal_patch_refuses_to_leave_the_vector_empty_and_names_the_flag() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "No goal yet".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();

    let error = store
        .patch_goal(
            &plan.id,
            GoalPatch {
                vision: Some("a vision with nothing steering it".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap_err();
    assert!(matches!(error, PlanError::GoalVectorRequired));
    let json = error.to_json();
    assert_eq!(json["type"], "goal_vector_required");
    assert_eq!(json["remedy"]["flag"], "--vector");
    assert!(
        store.get(&plan.id).unwrap().goal.is_none(),
        "a refused patch must not have written a partial goal"
    );
}

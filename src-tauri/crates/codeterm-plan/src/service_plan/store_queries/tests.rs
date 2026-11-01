use super::*;
use crate::plan::{OrchestratorRef, Section};

#[test]
fn list_index_counts_come_from_sql_without_loading_plans() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Counted".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner-tab"),
    );
    plan.team_id = Some("team-counted".into());
    store.save(&mut plan, 1).unwrap();
    for (index, name) in ["pending", "running", "finished", "parked"]
        .iter()
        .enumerate()
    {
        store
            .add_task(
                &plan.id,
                NewTask {
                    name: (*name).into(),
                    ..Default::default()
                },
                2 + index as u64,
            )
            .unwrap();
    }
    store
        .with_plan(&plan.id, 10, |plan| {
            let assignments = [
                (TaskStatus::Pending, " tab-a "),
                (TaskStatus::InProgress, "tab-b"),
                (TaskStatus::Done, "tab-c"),
                (TaskStatus::Paused, "tab-d"),
            ];
            for (task, (status, tab)) in plan.tasks.iter_mut().zip(assignments) {
                task.status = status;
                task.assigned_tab_id = Some(tab.into());
            }
            for id in ["sec_a", "sec_b"] {
                plan.sections.push(Section {
                    id: id.into(),
                    title: id.into(),
                    handle: None,
                    tags: vec![],
                    body: String::new(),
                    subsections: vec![Section {
                        id: format!("{id}_child"),
                        title: "child".into(),
                        handle: None,
                        tags: vec![],
                        body: String::new(),
                        subsections: vec![],
                        canvas: Default::default(),
                        attachments: vec![],
                    }],
                    canvas: Default::default(),
                    attachments: vec![],
                });
            }
            Ok(())
        })
        .unwrap();
    let stored = store.get(&plan.id).unwrap();
    let database_path = store.database_path_for_test();
    drop(store);

    let reopened = PlanStore::open(database_path.clone()).unwrap();
    let entry = reopened.list_index().remove(0);

    assert_eq!(entry.plan_id, plan.id);
    assert_eq!(entry.title, "Counted");
    assert_eq!(entry.status, PlanStatus::Active);
    assert_eq!(entry.team_id.as_deref(), Some("team-counted"));
    assert_eq!(entry.owner.tab_id.as_deref(), Some("owner-tab"));
    assert_eq!(entry.updated_at, stored.updated_at);
    assert_eq!(entry.file_path, database_path);
    assert_eq!(entry.task_count, stored.tasks.len());
    assert_eq!(entry.task_count, 4);
    assert_eq!(entry.open_task_count, 2);
    assert_eq!(entry.section_count, 2);
    assert_eq!(entry.element_count, stored.elements.len());
    assert_eq!(entry.loop_count, stored.loops.len());
    assert_eq!(entry.has_goal, stored.goal.is_some());
    assert_eq!(entry.assigned_tab_ids, vec!["tab-a", "tab-b", "tab-d"]);
    assert!(
        reopened.plan_loads_for_test().is_empty(),
        "list_index must answer from SQL without loading a plan"
    );
}

#[cfg(windows)]
mod windows {
    use super::*;

    /// Windows paths are case-insensitive on disk; a query naming the same
    /// project directory with different letter casing must still resolve.
    #[test]
    fn active_plan_for_project_matches_a_differently_cased_windows_path() {
        let root = tempfile::tempdir().unwrap();
        let project_dir = root.path().join("MyProject");
        std::fs::create_dir(&project_dir).unwrap();
        let store = PlanStore::new_for_test(&project_dir);
        store
            .create_plan_for_team(
                "Windows casing".into(),
                project_dir.display().to_string(),
                OrchestratorRef::from_tab("orch"),
                "team_case".into(),
                vec![],
                1,
            )
            .unwrap();

        let canonical = project_dir.canonicalize().unwrap().display().to_string();
        let differently_cased = canonical.replace("MyProject", "myproject");
        assert_ne!(
            differently_cased, canonical,
            "fixture path has no casing to flip"
        );

        assert!(
            store.active_plan_for_project(&differently_cased).is_some(),
            "a differently-cased query for the same directory must still resolve"
        );
    }
}

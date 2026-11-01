use super::*;
use crate::plan::{OrchestratorRef, Plan};

fn store() -> (tempfile::TempDir, PlanStore) {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    (root, store)
}

/// A second workspace whose plan artifacts stay inside its own temporary root.
fn other_workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    crate::service_plan::paths::mark_test_project_root(root.path());
    root
}

fn project(root: &tempfile::TempDir) -> String {
    root.path().display().to_string()
}

fn new_item(project_path: &str, title: &str) -> NewRadarItem {
    NewRadarItem {
        project_path: project_path.into(),
        title: title.into(),
        ..Default::default()
    }
}

fn active_plan(store: &PlanStore, project_path: &str) -> String {
    let mut plan = Plan::new(
        "Radar target".into(),
        project_path.into(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    plan.id
}

fn github_source(number: u64) -> RadarSource {
    RadarSource {
        provider: "github".into(),
        external_id: format!("acme/widgets#{number}"),
        url: Some(format!("https://github.com/acme/widgets/issues/{number}")),
    }
}

#[test]
fn radar_item_persists_without_creating_a_plan() {
    let (root, store) = store();
    let plans_before = store.all_plans_for_test().len();
    let mut new = new_item(&project(&root), "Flaky reconnect");
    new.body = "seen twice".into();
    new.tags = vec!["Mesh".into(), "mesh".into()];

    let added = store.radar().add(new, 10).unwrap();

    assert!(added.created);
    assert!(is_radar_item_id(&added.item.id));
    assert_eq!(added.item.status, RadarStatus::Open);
    assert_eq!(added.item.tags, vec!["mesh".to_string()]);
    assert_eq!(store.all_plans_for_test().len(), plans_before);
    let reopened = PlanStore::open(store.database_path_for_test()).unwrap();
    assert_eq!(reopened.radar().get(&added.item.id).unwrap(), added.item);
}

#[test]
fn radar_list_filters_by_workspace_status_tag_provider_and_text() {
    let (root, store) = store();
    let other = other_workspace();
    let radar = store.radar();
    let mut tagged = new_item(&project(&root), "Tagged");
    tagged.tags = vec!["ux".into()];
    let tagged = radar.add(tagged, 1).unwrap().item;
    let mut sourced = new_item(&project(&root), "From tracker");
    sourced.body = "needle in body".into();
    sourced.source = Some(github_source(7));
    let sourced = radar.add(sourced, 2).unwrap().item;
    radar
        .add(new_item(&project(&other), "Elsewhere"), 3)
        .unwrap();
    let workspace = Some(super::super::task_queries::canonical_project(&project(
        &root,
    )));

    let ids = |filter: RadarFilter| -> Vec<String> {
        radar
            .list(&filter)
            .unwrap()
            .into_iter()
            .map(|item| item.id)
            .collect()
    };

    assert_eq!(
        ids(RadarFilter {
            project_path: workspace.clone(),
            ..Default::default()
        }),
        vec![sourced.id.clone(), tagged.id.clone()]
    );
    assert_eq!(ids(RadarFilter::default()).len(), 3);
    assert_eq!(
        ids(RadarFilter {
            project_path: workspace.clone(),
            tag: Some("UX".into()),
            ..Default::default()
        }),
        vec![tagged.id.clone()]
    );
    assert_eq!(
        ids(RadarFilter {
            project_path: workspace.clone(),
            provider: Some("github".into()),
            ..Default::default()
        }),
        vec![sourced.id.clone()]
    );
    assert_eq!(
        ids(RadarFilter {
            project_path: workspace.clone(),
            query: Some("NEEDLE".into()),
            ..Default::default()
        }),
        vec![sourced.id.clone()]
    );
    assert!(ids(RadarFilter {
        project_path: workspace,
        statuses: vec![RadarStatus::Pulled],
        ..Default::default()
    })
    .is_empty());
}

#[test]
fn radar_edit_changes_only_named_fields_and_drop_removes_the_item() {
    let (root, store) = store();
    let radar = store.radar();
    let mut new = new_item(&project(&root), "Draft");
    new.body = "keep me".into();
    let item = radar.add(new, 1).unwrap().item;

    let edited = radar
        .update(
            &item.id,
            RadarEdit {
                title: Some("Final".into()),
                ..Default::default()
            },
            5,
        )
        .unwrap();

    assert_eq!(edited.title, "Final");
    assert_eq!(edited.body, "keep me");
    assert_eq!(edited.updated_at, 5);
    assert!(matches!(
        radar.update(&item.id, RadarEdit::default(), 6),
        Err(RadarError::Invalid(_))
    ));
    assert_eq!(radar.drop_item(&item.id).unwrap().title, "Final");
    assert!(matches!(radar.get(&item.id), Err(RadarError::NotFound(_))));
    assert!(matches!(
        radar.drop_item(&item.id),
        Err(RadarError::NotFound(_))
    ));
}

#[test]
fn radar_source_identity_refreshes_one_item_per_workspace() {
    let (root, store) = store();
    let radar = store.radar();
    let mut first = new_item(&project(&root), "Old title");
    first.source = Some(github_source(9));
    let first = radar.add(first, 1).unwrap();
    let mut again = new_item(&project(&root), "New title");
    again.source = Some(RadarSource {
        provider: "GitHub".into(),
        ..github_source(9)
    });

    let refreshed = radar.add(again, 2).unwrap();

    assert!(first.created);
    assert!(!refreshed.created);
    assert_eq!(refreshed.item.id, first.item.id);
    assert_eq!(refreshed.item.title, "New title");
    assert_eq!(refreshed.item.created_at, 1);
    assert_eq!(radar.list(&RadarFilter::default()).unwrap().len(), 1);
}

#[test]
fn radar_pull_creates_one_linked_plan_task() {
    let (root, store) = store();
    let plan_id = active_plan(&store, &project(&root));
    let mut new = new_item(&project(&root), "Ship the fix");
    new.body = "details".into();
    new.source = Some(github_source(42));
    let item = store.radar().add(new, 1).unwrap().item;

    let pulled = store
        .pull_radar_item(
            RadarPullRequest {
                item_id: item.id.clone(),
                plan_id: plan_id.clone(),
                ..Default::default()
            },
            20,
        )
        .unwrap();

    let link = pulled.pulled.clone().expect("pulled item records its task");
    assert_eq!(pulled.status, RadarStatus::Pulled);
    assert_eq!(link.plan_id, plan_id);
    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.tasks.len(), 1);
    let task = &plan.tasks[0];
    assert_eq!(task.id, link.task_id);
    assert_eq!(task.name, "Ship the fix");
    assert_eq!(task.body, "details");
    assert_eq!(task.external_refs.len(), 1);
    assert_eq!(store.radar().get(&item.id).unwrap(), pulled);
    let again = store.pull_radar_item(
        RadarPullRequest {
            item_id: item.id,
            plan_id: plan_id.clone(),
            ..Default::default()
        },
        21,
    );
    assert!(matches!(again, Err(RadarError::AlreadyPulled(ref pull)) if *pull == link));
    assert_eq!(store.get(&plan_id).unwrap().tasks.len(), 1);
}

#[test]
fn radar_pull_refuses_a_plan_of_another_workspace() {
    let (root, store) = store();
    let other = other_workspace();
    let foreign_plan = active_plan(&store, &project(&other));
    let item = store
        .radar()
        .add(new_item(&project(&root), "Stay home"), 1)
        .unwrap()
        .item;

    let refused = store.pull_radar_item(
        RadarPullRequest {
            item_id: item.id.clone(),
            plan_id: foreign_plan.clone(),
            ..Default::default()
        },
        2,
    );

    assert!(matches!(refused, Err(RadarError::Invalid(_))));
    assert!(store.get(&foreign_plan).unwrap().tasks.is_empty());
    assert_eq!(
        store.radar().get(&item.id).unwrap().status,
        RadarStatus::Open
    );
}

#[test]
fn radar_refuses_ids_that_are_not_item_names() {
    let (root, store) = store();
    let radar = store.radar();
    std::fs::create_dir_all(radar.root()).unwrap();
    std::fs::write(root.path().join("outside.json"), "{}").unwrap();

    for id in ["../outside", "rd_../../outside", "RD_ABC", "rd_", "task_1"] {
        assert!(
            matches!(radar.get(id), Err(RadarError::NotFound(_))),
            "{id}"
        );
        assert!(
            matches!(radar.drop_item(id), Err(RadarError::NotFound(_))),
            "{id}"
        );
    }
    assert!(root.path().join("outside.json").exists());
}

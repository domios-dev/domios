use super::*;
use crate::plan::{OrchestratorRef, Plan};
use crate::service_plan::{NewTask, PlanStore};

#[test]
fn external_ref_url_and_repo_number_canonicalize_identically() {
    let url =
        canonicalize_external_ref("https://github.com/RollaCode/CodeTerm-Releases/issues/017/")
            .unwrap();
    let shorthand = canonicalize_external_ref("rollacode/codeterm-releases#17").unwrap();
    assert_eq!(url, shorthand);
    assert_eq!(url.repository, "rollacode/codeterm-releases");
    assert_eq!(url.number, 17);
    assert_eq!(
        url.canonical_url,
        "https://github.com/rollacode/codeterm-releases/issues/17"
    );
}

#[test]
fn external_ref_rejects_malformed_and_ambiguous_inputs() {
    let malformed = canonicalize_external_ref("not-a-ref").unwrap_err();
    assert!(malformed.to_string().contains("must be a GitHub issue URL"));
    let ambiguous = canonicalize_external_ref("#17").unwrap_err();
    assert!(ambiguous.to_string().contains("exactly one '/'"));
    let wrong_path = canonicalize_external_ref("https://github.com/a/b/pull/17").unwrap_err();
    assert!(wrong_path.to_string().contains("URL path"));
}

#[test]
fn external_ref_relationship_serializes_as_data() {
    let item = canonicalize_external_ref_with_relationship(
        "rollacode/codeterm-releases#17",
        Some(ExternalRefRelationship::Fixes),
    )
    .unwrap();
    let json = serde_json::to_value(&item).unwrap();
    assert_eq!(json["kind"], "github_issue");
    assert_eq!(json["relationship"], "fixes");
}

#[test]
fn external_ref_round_trips_and_survives_task_updates_without_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "External refs".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let values = vec![
        "https://github.com/RollaCode/CodeTerm-Releases/issues/017".to_string(),
        "rollacode/codeterm-releases#17".to_string(),
    ];
    let task_id = store
        .add_task_with_external_refs(
            &plan.id,
            NewTask {
                name: "Linked task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            &values,
            2,
        )
        .unwrap();
    store
        .add_task_external_refs(&plan.id, &task_id, &values, 3)
        .unwrap();
    store
        .update_task(
            &plan.id,
            &task_id,
            Some("Renamed task"),
            None,
            None,
            None,
            Some("blocked"),
            Some(Some("external dependency")),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            4,
        )
        .unwrap();
    store
        .edit_plan_fixture(&plan.id, 5, |plan| {
            plan.tasks[0].external_refs[0].relationship = Some(ExternalRefRelationship::RelatesTo);
            Ok(())
        })
        .unwrap();

    store.forget_all_plans_for_test();
    store.reload_from_disk(&plan.id).unwrap();
    let task = store.inspect_task_for_test(&task_id).unwrap().task;
    assert_eq!(task.name, "Renamed task");
    assert_eq!(task.external_refs.len(), 1);
    assert_eq!(
        task.external_refs[0].relationship,
        Some(ExternalRefRelationship::RelatesTo)
    );
    assert_eq!(
        task.external_refs[0].canonical_url,
        "https://github.com/rollacode/codeterm-releases/issues/17"
    );
}

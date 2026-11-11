use super::super::{refuse_missing_explicit_cwd, resolve_dispatch_lane};
use super::git_project;
use codeterm_service_core::plan::TaskLaneMetadata;
use codeterm_service_core::service::LaneProtection;

#[test]
fn explicit_cwd_absent_or_existing_directory_is_accepted() {
    let root = tempfile::tempdir().unwrap();
    assert!(refuse_missing_explicit_cwd(None).is_ok());
    assert!(refuse_missing_explicit_cwd(Some("  ")).is_ok());
    assert!(refuse_missing_explicit_cwd(Some(root.path().to_str().unwrap())).is_ok());
}

#[test]
fn explicit_cwd_missing_path_is_refused_naming_the_path() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("no-such-lane");
    let missing = missing.to_str().unwrap();
    let error = refuse_missing_explicit_cwd(Some(missing)).unwrap_err();
    assert_eq!(error.code, -32602);
    let data = error.data.as_ref().unwrap();
    assert_eq!(data["type"], "invalid_task_dispatch");
    assert_eq!(data["stage"], "cwd_validation");
    assert!(error.message.contains(missing), "{}", error.message);
}

#[test]
fn explicit_cwd_file_is_refused_as_not_a_directory() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file.txt");
    std::fs::write(&file, "not a checkout").unwrap();
    let file = file.to_str().unwrap();
    let error = refuse_missing_explicit_cwd(Some(file)).unwrap_err();
    assert_eq!(error.data.as_ref().unwrap()["stage"], "cwd_validation");
    assert!(error.message.contains(file), "{}", error.message);
    assert!(
        error.message.contains("not a directory"),
        "{}",
        error.message
    );
}

#[test]
fn explicit_cwd_relative_path_is_refused_because_the_daemon_has_no_caller_directory() {
    let error = refuse_missing_explicit_cwd(Some("relative-lane")).unwrap_err();
    assert_eq!(error.data.as_ref().unwrap()["stage"], "cwd_validation");
    assert!(error.message.contains("relative-lane"), "{}", error.message);
}

#[test]
fn explicit_cwd_that_is_not_a_checkout_never_falls_back_to_the_workspace() {
    let (_root, project) = git_project();
    let plain = tempfile::tempdir().unwrap();
    let explicit = plain.path().to_str().unwrap();
    let workspace = project.to_str().unwrap();
    let error = resolve_dispatch_lane(
        Some(explicit),
        Some(workspace),
        Some(workspace),
        &TaskLaneMetadata::default(),
        LaneProtection::Enforced,
    )
    .unwrap_err();
    assert_eq!(error.code, -32012);
    assert!(error.message.contains(explicit), "{}", error.message);
    assert!(!error.message.contains(workspace), "{}", error.message);
}

#[test]
fn foreign_checkout_binding_records_git_root_and_refuses_rebinding_or_fallback() {
    use codeterm_service_core::plan::{ExecutionIntent, OrchestratorRef};
    use codeterm_service_core::service::plan::{NewTask, PlanStore};

    let (root, canonical) = git_project();
    let (_foreign_root, foreign) = git_project();
    let nested = foreign.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let store = PlanStore::new_for_test(root.path());
    let plan = store
        .create_plan_for_team(
            "checkout binding".into(),
            canonical.display().to_string(),
            OrchestratorRef::from_tab("owner"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let task = store
        .add_task(
            &plan.id,
            NewTask {
                name: "binding".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let attempt = store
        .begin_task_dispatch_with_policy(
            &plan.id,
            &task,
            "worker",
            "Finch",
            TaskLaneMetadata::default(),
            ExecutionIntent::Mutating,
            None,
            None,
            3,
        )
        .unwrap();
    assert!(store
        .record_attempt_checkout(
            &plan.id,
            &task,
            "worker",
            &attempt,
            root.path().join("missing").display().to_string(),
            4
        )
        .is_err());
    assert!(store
        .inspect_task_for_test(&task)
        .unwrap()
        .task
        .execution_attempts[0]
        .checkout_path
        .is_none());
    store
        .record_attempt_checkout(
            &plan.id,
            &task,
            "worker",
            &attempt,
            nested.display().to_string(),
            5,
        )
        .unwrap();
    let bound = store
        .inspect_task_for_test(&task)
        .unwrap()
        .task
        .execution_attempts[0]
        .clone();
    assert_eq!(
        std::fs::canonicalize(bound.checkout_path.unwrap()).unwrap(),
        std::fs::canonicalize(&foreign).unwrap()
    );
    assert_eq!(
        bound.shared_start_sha.unwrap(),
        codeterm_service_core::service::worker_lane::git_value(
            foreign.to_str().unwrap(),
            ["rev-parse", "HEAD"]
        )
        .unwrap()
    );
    assert!(store
        .record_attempt_checkout(
            &plan.id,
            &task,
            "worker",
            &attempt,
            canonical.display().to_string(),
            6
        )
        .is_err());
    assert!(store
        .record_attempt_checkout(
            &plan.id,
            &task,
            "other",
            &attempt,
            foreign.display().to_string(),
            7
        )
        .is_err());
    assert_eq!(
        std::fs::canonicalize(
            store
                .inspect_task_for_test(&task)
                .unwrap()
                .task
                .execution_attempts[0]
                .checkout_path
                .as_ref()
                .unwrap()
        )
        .unwrap(),
        std::fs::canonicalize(&foreign).unwrap()
    );
}

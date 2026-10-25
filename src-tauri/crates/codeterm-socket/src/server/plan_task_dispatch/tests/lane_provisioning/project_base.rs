use super::*;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use codeterm_service_core::service::SpawnAgentParams;

fn git_value(path: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    codeterm_service_core::process_util::no_window_std(&mut command);
    let output = command.current_dir(path).args(args).output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn project_worktree(fixture: &mut Fixture) -> PathBuf {
    let project = fixture.root.path().join("plan-project");
    super::super::git(
        &fixture.project,
        &[
            "worktree",
            "add",
            "-b",
            "feature/project",
            project.to_str().unwrap(),
        ],
    );
    std::fs::write(project.join("project.txt"), "project commit").unwrap();
    super::super::git(&project, &["add", "project.txt"]);
    super::super::git(&project, &["commit", "-qm", "project"]);
    fixture.plan.project_path = project.display().to_string();
    fixture.plan.canonical_project_path = fixture.project.display().to_string();
    project
}

fn dispatch_lane(fixture: &Fixture, lane: &TaskLaneMetadata) -> ProvisionedWorkerLane {
    let resolved = resolved_from_workspace(Path::new(&fixture.plan.project_path), lane);
    let placement =
        plan_dispatch_lane_placement(&fixture.plan, &fixture.mine, lane, resolved).unwrap();
    let DispatchLanePlacement::Provision { base_checkout } = placement else {
        panic!("the plan project must get an isolated lane: {placement:?}");
    };
    assert!(same_dir(
        &base_checkout,
        Path::new(&fixture.plan.project_path)
    ));
    let (mux, _) = MultiplexerState::new();
    let service = ServiceFixture::new(mux);
    let mut params = SpawnAgentParams::default();
    let provisioned = service
        .service
        .provision_dispatch_worker_lane(
            &mut params,
            &base_checkout,
            "project-base",
            lane.base.as_deref(),
        )
        .unwrap();
    assert_eq!(params.cwd.as_deref(), Some(provisioned.cwd.as_str()));
    assert_eq!(params.provisioned_cwd, params.cwd);
    provisioned
}

#[test]
fn dispatch_defaults_to_noncanonical_project_head_and_destination() {
    let mut fixture = fixture();
    let project = project_worktree(&mut fixture);
    let expected = git_value(&project, &["rev-parse", "HEAD"]);
    assert_ne!(
        expected,
        git_value(&fixture.project, &["rev-parse", "HEAD"])
    );
    let lane = dispatch_lane(&fixture, &TaskLaneMetadata::default());
    let provenance = lane.verify().unwrap();
    assert_eq!(
        git_value(Path::new(&lane.cwd), &["rev-parse", "HEAD"]),
        expected
    );
    assert_eq!(provenance.base, expected);
    let committed = lane_metadata_for_dispatch(
        &TaskLaneMetadata::default(),
        &provenance,
        fixture.project.to_str().unwrap(),
    );
    assert_eq!(committed.destination.as_deref(), Some("feature/project"));
    assert_eq!(committed.base.as_deref(), Some(expected.as_str()));
    lane.cleanup().unwrap();
}

#[test]
fn dispatch_honours_recorded_base_even_when_project_head_differs() {
    let mut fixture = fixture();
    let project = project_worktree(&mut fixture);
    let expected = git_value(&project, &["rev-parse", "HEAD"]);
    super::super::git(&project, &["branch", "task-base"]);
    std::fs::write(project.join("later.txt"), "later project commit").unwrap();
    super::super::git(&project, &["add", "later.txt"]);
    super::super::git(&project, &["commit", "-qm", "later"]);
    assert_ne!(expected, git_value(&project, &["rev-parse", "HEAD"]));
    fixture
        .store
        .update_task_lane(
            &fixture.plan.id,
            &fixture.mine,
            TaskLaneUpdate {
                base: Some(Some("task-base".into())),
                destination: Some(Some("feature/project".into())),
                ..Default::default()
            },
            4,
        )
        .unwrap();
    let recorded = fixture.store.show_task(&fixture.mine).unwrap().task.lane;
    let lane = dispatch_lane(&fixture, &recorded);
    let provenance = lane.verify().unwrap();
    assert_eq!(
        git_value(Path::new(&lane.cwd), &["rev-parse", "HEAD"]),
        expected
    );
    assert_eq!(provenance.base, expected);
    let committed =
        lane_metadata_for_dispatch(&recorded, &provenance, fixture.project.to_str().unwrap());
    assert_eq!(committed.base.as_deref(), Some(expected.as_str()));
    assert_eq!(committed.destination, recorded.destination);
    lane.cleanup().unwrap();
}

#[test]
fn dispatch_invalid_recorded_base_refuses_without_falling_back_to_head() {
    let fixture = fixture();
    let (mux, _) = MultiplexerState::new();
    let service = ServiceFixture::new(mux);
    let mut params = SpawnAgentParams::default();
    let before = git_value(&fixture.project, &["worktree", "list", "--porcelain"]);
    service
        .service
        .provision_dispatch_worker_lane(
            &mut params,
            fixture.project.to_str().unwrap(),
            "invalid-base",
            Some("missing-task-base"),
        )
        .unwrap_err();
    assert!(params.cwd.is_none());
    assert!(params.provisioned_cwd.is_none());
    assert_eq!(
        git_value(&fixture.project, &["worktree", "list", "--porcelain"]),
        before
    );
}

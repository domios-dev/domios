use super::preflight_facts;
use serde_json::json;

use super::super::canonical_writer::{
    compose_dispatch_brief, dispatch_worker_placement,
    refuse_lane_integration_while_canonical_held as refuse_projected_lane_integration,
    verify_canonical_writer_checkout, CANONICAL_WRITER_BRIEF, CANONICAL_WRITER_HOLDS_CHECKOUT_CODE,
    REFUSAL_LANE_INTEGRATION_WHILE_CANONICAL_WRITER, SHARED_WITHOUT_GIT_BRIEF,
};
use super::super::lane_safety::{
    refuse_unsafe_mutating_write, REFUSAL_MUTATING_WRITE_IN_CANONICAL,
};
use super::super::{plan_dispatch_lane_placement, preflight_dispatch_placement};
use codeterm_service_core::plan::{
    ExecutionIntent, OrchestratorRef, Plan, TaskExecutionAttempt, TaskLaneMetadata, WorkerPlacement,
};
use codeterm_service_core::service::plan::{NewTask, PlanError, PlanStore};
use codeterm_service_core::service::{
    canonical_path_of, classify_mutating_lane, live_canonical_writer, LaneProtection,
    UnsafeMutatingLane, WorkerLaneProvenance,
};

fn refuse_lane_integration_while_canonical_held(
    plan: &Plan,
    task_id: &str,
) -> Result<(), crate::server::JsonRpcError> {
    use codeterm_service_core::service::plan::{CanonicalWriterHoldRead, LaneIntegrationRead};
    let input = (|| {
        let task = plan.tasks.iter().find(|task| task.id == task_id)?;
        let lane = task
            .lane
            .worktree_path
            .as_deref()
            .filter(|path| !path.trim().is_empty())?;
        let (holder_task_id, holder_attempt_id) = live_canonical_writer(plan)?;
        if holder_task_id == task.id {
            return None;
        }
        let attempt = plan
            .tasks
            .iter()
            .find(|task| task.id == holder_task_id)?
            .execution_attempts
            .iter()
            .find(|attempt| attempt.id == holder_attempt_id)?;
        Some(LaneIntegrationRead {
            plan_id: plan.id.clone(),
            task_id: task.id.clone(),
            worktree_path: lane.into(),
            canonical_path: canonical_path_of(plan).into(),
            holder: CanonicalWriterHoldRead {
                task_id: holder_task_id.into(),
                attempt_id: holder_attempt_id.into(),
                placement: attempt.placement,
            },
        })
    })();
    refuse_projected_lane_integration(input.as_ref())
}

fn store_with_tasks(names: &[&str]) -> (tempfile::TempDir, PlanStore, String, Vec<String>) {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let mut plan = Plan::new(
        "Canonical writer".into(),
        root.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let ids = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            store
                .add_task(
                    &plan.id,
                    NewTask {
                        name: (*name).into(),
                        kind: "implement".into(),
                        ..Default::default()
                    },
                    2 + index as u64,
                )
                .unwrap()
        })
        .collect();
    (root, store, plan.id, ids)
}

fn begin(
    store: &PlanStore,
    plan_id: &str,
    task_id: &str,
    tab_id: &str,
    gates: Vec<String>,
    placement: WorkerPlacement,
) -> String {
    store
        .begin_task_dispatch_with_reserved_attempt_and_gates(
            plan_id,
            task_id,
            tab_id,
            "Worker",
            TaskLaneMetadata::default(),
            ExecutionIntent::Unspecified,
            None,
            None,
            &format!("attempt-{tab_id}"),
            gates,
            placement,
            10,
        )
        .unwrap()
}

fn close_live_attempts(plan: &mut Plan, task_id: &str) {
    plan.tasks
        .iter_mut()
        .filter(|task| task.id == task_id)
        .flat_map(|task| task.execution_attempts.iter_mut())
        .for_each(|attempt| attempt.closed_at = Some(99));
}

fn repository_task() -> codeterm_service_core::plan::Task {
    super::task(json!({
        "id": "task_repo",
        "name": "repo",
        "type": "code",
        "status": "pending"
    }))
}

#[test]
fn canonical_writer_placement_is_never_inferred_without_opt_in() {
    let task = repository_task();
    for params in [
        None,
        Some(json!({})),
        Some(json!({ "placement": null })),
        Some(json!({ "placement": "isolated_lane" })),
    ] {
        assert_eq!(
            dispatch_worker_placement(
                params.as_ref(),
                &ExecutionIntent::Mutating,
                &preflight_facts(&task)
            )
            .unwrap(),
            WorkerPlacement::IsolatedLane
        );
    }

    let (root, store, plan_id, ids) = store_with_tasks(&["mine"]);
    let plan = store.get(&plan_id).unwrap();
    let canonical = root.path().display().to_string();
    let error = refuse_unsafe_mutating_write(&plan, &ids[0], &canonical, ExecutionIntent::Mutating)
        .unwrap_err();
    assert_eq!(
        error.data.unwrap()["violation_id"],
        REFUSAL_MUTATING_WRITE_IN_CANONICAL
    );
    let placement = plan_dispatch_lane_placement(
        &plan,
        &ids[0],
        &TaskLaneMetadata::default(),
        WorkerLaneProvenance {
            canonical_cwd: canonical.clone(),
            worktree_path: canonical.clone(),
            branch: "main".into(),
            base: "base".into(),
            destination: None,
        },
    )
    .unwrap();
    assert!(
        placement.provisions(),
        "without the opt-in a mutating dispatch resolving canonical still gets an isolated lane"
    );
}

#[test]
fn canonical_writer_opt_in_is_refused_outside_a_mutating_repository_task_without_a_lane() {
    let opt_in = json!({ "placement": "canonical_writer" });
    let repo_task = repository_task();
    assert_eq!(
        dispatch_worker_placement(
            Some(&opt_in),
            &ExecutionIntent::Mutating,
            &preflight_facts(&repo_task)
        )
        .unwrap(),
        WorkerPlacement::CanonicalWriter
    );

    for intent in [ExecutionIntent::ReadOnly, ExecutionIntent::Unspecified] {
        let error = dispatch_worker_placement(Some(&opt_in), &intent, &preflight_facts(&repo_task))
            .unwrap_err();
        assert_eq!(error.code, -32602);
    }

    let nonrepo = super::task(json!({
        "id": "task_nonrepo",
        "name": "research",
        "type": "task",
        "status": "pending",
        "execution_intent": "non_repository"
    }));
    let laned = super::task(json!({
        "id": "task_laned",
        "name": "laned",
        "type": "code",
        "status": "pending",
        "lane": { "worktree_path": "/lanes/recorded" }
    }));
    for task in [&nonrepo, &laned] {
        let error = dispatch_worker_placement(
            Some(&opt_in),
            &ExecutionIntent::Mutating,
            &preflight_facts(&task),
        )
        .unwrap_err();
        assert_eq!(error.code, -32602, "{}", task.id);
    }

    for malformed in [
        json!({ "placement": "canonical" }),
        json!({ "placement": 1 }),
    ] {
        let error = dispatch_worker_placement(
            Some(&malformed),
            &ExecutionIntent::Mutating,
            &preflight_facts(&repo_task),
        )
        .unwrap_err();
        assert_eq!(error.code, -32602);
    }
}

#[test]
fn canonical_writer_placement_is_persisted_and_binds_the_report_to_canonical() {
    let (_root, store, plan_id, ids) = store_with_tasks(&["writer", "lane"]);
    let writer_attempt = begin(
        &store,
        &plan_id,
        &ids[0],
        "writer-tab",
        vec!["gate-token-focused".into()],
        WorkerPlacement::CanonicalWriter,
    );
    let lane_attempt = begin(
        &store,
        &plan_id,
        &ids[1],
        "lane-tab",
        Vec::new(),
        WorkerPlacement::IsolatedLane,
    );

    let plan = store.get(&plan_id).unwrap();
    let attempt_json = |task_id: &str| {
        let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
        serde_json::to_value(&task.execution_attempts[0]).unwrap()
    };
    assert_eq!(attempt_json(&ids[0])["id"], writer_attempt);
    assert_eq!(attempt_json(&ids[0])["placement"], "canonical_writer");
    assert_eq!(attempt_json(&ids[1])["id"], lane_attempt);
    assert!(attempt_json(&ids[1]).get("placement").is_none());

    store
        .finish_task_dispatch(
            &plan_id,
            &ids[0],
            "writer-tab",
            &writer_attempt,
            "session-writer",
            "nonce-writer",
            TaskLaneMetadata::default(),
            11,
        )
        .unwrap();
    let plan = store.get(&plan_id).unwrap();
    let task = plan.tasks.iter().find(|task| task.id == ids[0]).unwrap();
    let attempt = &task.execution_attempts[0];
    assert_eq!(attempt.placement, WorkerPlacement::CanonicalWriter);
    assert_eq!(
        attempt.worktree_path.as_deref(),
        Some(canonical_path_of(&plan))
    );
    assert_eq!(attempt.base, None);
    assert_eq!(
        task.lane.worktree_path, None,
        "canonical is never recorded as a lane, so lane cleanup can never remove it"
    );
}

#[test]
fn canonical_writer_occupied_canonical_refuses_a_second_live_writer() {
    let (root, store, plan_id, ids) = store_with_tasks(&["first", "second"]);
    let canonical = root.path().display().to_string();
    let first_attempt = begin(
        &store,
        &plan_id,
        &ids[0],
        "first-tab",
        Vec::new(),
        WorkerPlacement::CanonicalWriter,
    );
    let mut plan = store.get(&plan_id).unwrap();

    let error = preflight_dispatch_placement(
        &plan,
        &ids[1],
        &canonical,
        LaneProtection::Enforced,
        false,
        &Default::default(),
    )
    .unwrap_err();
    assert_eq!(error.code, -32011);
    let data = error.data.unwrap();
    assert_eq!(data["occupying_task_id"], ids[0]);
    assert_eq!(data["occupying_attempt_id"], first_attempt);

    preflight_dispatch_placement(
        &plan,
        &ids[1],
        &canonical,
        LaneProtection::Enforced,
        true,
        &Default::default(),
    )
    .expect("an ordinary lane dispatch keeps its provisioning contract while canonical is held");

    close_live_attempts(&mut plan, &ids[0]);
    preflight_dispatch_placement(
        &plan,
        &ids[1],
        &canonical,
        LaneProtection::Enforced,
        false,
        &Default::default(),
    )
    .expect("a closed canonical writer releases the checkout");
}

#[test]
fn canonical_writer_holds_canonical_against_lane_integration() {
    let (root, store, plan_id, ids) = store_with_tasks(&["writer", "lane"]);
    let lane_path = root.path().join("lane").display().to_string();
    let with_lane = |mut plan: Plan| {
        plan.tasks
            .iter_mut()
            .find(|task| task.id == ids[1])
            .unwrap()
            .lane
            .worktree_path = Some(lane_path.clone());
        plan
    };

    let idle = with_lane(store.get(&plan_id).unwrap());
    refuse_lane_integration_while_canonical_held(&idle, &ids[1])
        .expect("no canonical writer means lane integration is unobstructed");

    let writer_attempt = begin(
        &store,
        &plan_id,
        &ids[0],
        "writer-tab",
        Vec::new(),
        WorkerPlacement::CanonicalWriter,
    );
    let mut held = with_lane(store.get(&plan_id).unwrap());
    let error = refuse_lane_integration_while_canonical_held(&held, &ids[1]).unwrap_err();
    assert_eq!(error.code, CANONICAL_WRITER_HOLDS_CHECKOUT_CODE);
    let data = error.data.unwrap();
    assert_eq!(
        data["violation_id"],
        REFUSAL_LANE_INTEGRATION_WHILE_CANONICAL_WRITER
    );
    assert_eq!(data["holder_task_id"], ids[0]);
    assert_eq!(data["holder_attempt_id"], writer_attempt);

    refuse_lane_integration_while_canonical_held(&held, &ids[0])
        .expect("the canonical writer's own verdict is never blocked by its hold");

    close_live_attempts(&mut held, &ids[0]);
    refuse_lane_integration_while_canonical_held(&held, &ids[1])
        .expect("a closed canonical writer releases lane integration");
}

#[test]
fn canonical_writer_allows_accepting_an_ancestor_lane_without_checkout_mutation() {
    let (_root, store, plan_id, ids) = store_with_tasks(&["writer", "lane"]);
    let (_git_root, project) = super::git_project();
    let lane = project.parent().unwrap().join("accepted-lane");
    super::git(
        &project,
        &[
            "worktree",
            "add",
            "-b",
            "lane/accepted",
            lane.to_str().unwrap(),
        ],
    );
    begin(
        &store,
        &plan_id,
        &ids[0],
        "writer-tab",
        Vec::new(),
        WorkerPlacement::CanonicalWriter,
    );
    let mut plan = store.get(&plan_id).unwrap();
    plan.canonical_project_path = project.display().to_string();
    plan.tasks
        .iter_mut()
        .find(|task| task.id == ids[1])
        .unwrap()
        .lane
        .worktree_path = Some(lane.display().to_string());
    std::fs::write(project.join("writer-uncommitted.txt"), "active writer").unwrap();
    refuse_lane_integration_while_canonical_held(&plan, &ids[1]).unwrap();
    super::git(
        &lane,
        &["commit", "--allow-empty", "-m", "unmerged lane change"],
    );
    assert!(refuse_lane_integration_while_canonical_held(&plan, &ids[1]).is_err());
    assert_eq!(
        std::fs::read_to_string(project.join("writer-uncommitted.txt")).unwrap(),
        "active writer"
    );
}

#[test]
fn canonical_writer_gate_claim_outside_authorization_is_refused() {
    let (_root, store, plan_id, ids) = store_with_tasks(&["writer"]);
    begin(
        &store,
        &plan_id,
        &ids[0],
        "writer-tab",
        vec!["gate-token-focused".into()],
        WorkerPlacement::CanonicalWriter,
    );

    let error = store
        .validate_worker_gate_claim(&plan_id, &ids[0], "writer-tab", Some("gate-token-broad"))
        .unwrap_err();
    assert!(
        matches!(error, PlanError::UnauthorizedGateClaim { .. }),
        "{error:?}"
    );
    store
        .validate_worker_gate_claim(&plan_id, &ids[0], "writer-tab", Some("gate-token-focused"))
        .expect("the exact authorized gate stays claimable");
}

#[test]
fn canonical_writer_brief_section_rides_only_the_opt_in() {
    let base = "brief-body-token".to_string();
    let isolated = compose_dispatch_brief(base.clone(), WorkerPlacement::IsolatedLane, false);
    assert!(isolated.starts_with(&base));
    assert_eq!(isolated.matches(CANONICAL_WRITER_BRIEF).count(), 0);

    let canonical = compose_dispatch_brief(base.clone(), WorkerPlacement::CanonicalWriter, false);
    assert!(canonical.starts_with(&base));
    assert_eq!(canonical.matches(CANONICAL_WRITER_BRIEF).count(), 1);
}

#[test]
fn shared_brief_without_git_replaces_the_commit_contract_only_for_shared_placement() {
    let base = "brief-body-token".to_string();
    let without_git = compose_dispatch_brief(base.clone(), WorkerPlacement::Shared, true);
    assert_eq!(without_git, format!("{base}{SHARED_WITHOUT_GIT_BRIEF}"));
    let git = compose_dispatch_brief(base.clone(), WorkerPlacement::Shared, false);
    assert_eq!(git.matches(SHARED_WITHOUT_GIT_BRIEF).count(), 0);
    let read_only = compose_dispatch_brief(base.clone(), WorkerPlacement::ReadOnly, true);
    assert_eq!(read_only, base);
}

#[test]
fn canonical_worker_kind_composition_preserves_admission_and_report_identity() {
    let canonical = "C:/repo";
    let mut task = repository_task();
    task.executor_role = Some("repository-integrator".into());
    let mut foreign_task = repository_task();
    foreign_task.id = "task_foreign".into();
    foreign_task.lane.worktree_path = Some("C:/repo/.worktrees/foreign".into());
    let mut plan = Plan::new(
        "Canonical placement".into(),
        canonical.into(),
        OrchestratorRef::from_tab("orch"),
    );
    plan.canonical_project_path = canonical.into();
    plan.tasks = vec![task.clone(), foreign_task.clone()];

    assert_eq!(
        dispatch_worker_placement(
            Some(&json!({ "placement": "canonical_writer" })),
            &ExecutionIntent::Mutating,
            &preflight_facts(&task),
        )
        .unwrap(),
        WorkerPlacement::CanonicalWriter
    );
    assert_eq!(
        dispatch_worker_placement(None, &ExecutionIntent::ReadOnly, &preflight_facts(&task))
            .unwrap(),
        WorkerPlacement::ReadOnly
    );
    assert!(dispatch_worker_placement(
        Some(&json!({ "placement": "canonical_writer" })),
        &ExecutionIntent::ReadOnly,
        &preflight_facts(&task),
    )
    .is_err());
    assert!(matches!(
        classify_mutating_lane(&plan, &task.id, "C:/repo/.worktrees/foreign"),
        Some(UnsafeMutatingLane::ForeignLane { owner, .. }) if owner == foreign_task.id
    ));
    assert!(refuse_unsafe_mutating_write(
        &plan,
        &task.id,
        "C:/repo/.worktrees/foreign",
        ExecutionIntent::Mutating,
    )
    .is_err());

    for kind in ["qa", "release_manager"] {
        for task_id in [None, Some(task.id.as_str())] {
            let profile = codeterm_types::agent::role_kind::RoleType::parse(kind)
                .unwrap()
                .kind_id;
            let mut params = codeterm_service_core::service::SpawnAgentParams {
                role: Some("Worker".into()),
                role_profile: Some(profile),
                task_id: task_id.map(str::to_string),
                plan_id: task_id.is_none().then(|| "plan-context".into()),
                execution_intent: Some(ExecutionIntent::Mutating),
                cwd: Some(canonical.into()),
                ..Default::default()
            };
            params.normalize_role_profile().unwrap();
            let preset = codeterm_service_core::service::role_preset::resolve(
                Some(codeterm_service_core::multiplexer::types::AgentRole::Worker),
                params.role_profile.as_deref(),
                None,
            )
            .unwrap();
            let guidance =
                codeterm_service_core::service::role_preset::builtin_guidance(kind).unwrap();

            assert_eq!(params.task_id.as_deref(), task_id);
            assert_eq!(preset.kind_id, kind);
            assert_eq!(preset.fragments[0].source.label(), "builtin");
            assert_eq!(preset.contract_guidance(), guidance);
        }
    }

    plan.tasks[0].execution_attempts.push(TaskExecutionAttempt {
        id: "attempt-canonical".into(),
        executor_tab_id: "writer-tab".into(),
        placement: WorkerPlacement::CanonicalWriter,
        ..Default::default()
    });
    assert_eq!(
        live_canonical_writer(&plan),
        Some((task.id.as_str(), "attempt-canonical"))
    );
    let occupied = preflight_dispatch_placement(
        &plan,
        &foreign_task.id,
        canonical,
        LaneProtection::Enforced,
        false,
        &Default::default(),
    )
    .unwrap_err();
    assert_eq!(occupied.code, -32011);
    assert_eq!(occupied.data.unwrap()["occupying_task_id"], task.id);
}

#[test]
fn canonical_writer_checkout_must_be_an_attached_repository() {
    let (_root, project) = super::git_project();
    let project_cwd = project.display().to_string();
    verify_canonical_writer_checkout(&project_cwd).expect("attached canonical checkout");

    let plain = tempfile::tempdir().unwrap();
    let error = verify_canonical_writer_checkout(&plain.path().display().to_string()).unwrap_err();
    assert_eq!(error.code, -32012);

    super::git(&project, &["checkout", "-q", "--detach"]);
    let error = verify_canonical_writer_checkout(&project_cwd).unwrap_err();
    assert_eq!(error.code, -32012);
}

#[test]
fn nonrepository_dispatch_reports_nonrepository_placement() {
    let task = super::task(json!({
        "id": "task_nonrepo", "name": "nonrepo", "type": "code",
        "status": "pending", "execution_intent": "non_repository"
    }));
    assert_eq!(
        dispatch_worker_placement(None, &ExecutionIntent::Mutating, &preflight_facts(&task))
            .unwrap(),
        WorkerPlacement::NonRepository
    );
    assert!(dispatch_worker_placement(
        Some(&json!({"placement": "canonical_writer"})),
        &ExecutionIntent::Mutating,
        &preflight_facts(&task)
    )
    .is_err());
}

#[test]
fn placement_without_git_stays_shared_and_refuses_lanes_while_git_keeps_lanes() {
    use super::super::canonical_writer::resolve_placement;
    use codeterm_service_core::service::plan::shared_checkout::NO_GIT_HISTORY;
    let project = tempfile::tempdir().unwrap();
    let task = super::task(json!({
        "id": "task_plain", "name": "plain", "type": "code", "status": "pending",
        "files": {"edit": ["notes.md"]}
    }));
    let mut facts = preflight_facts(&task);
    facts.project_path = project.path().to_string_lossy().into_owned();
    let mutating = ExecutionIntent::Mutating;
    let place = |params: Option<serde_json::Value>,
                 facts: &codeterm_service_core::service::plan::TaskDispatchPreflightRead,
                 without_git: bool| {
        resolve_placement(params.as_ref(), &mutating, facts, &[], without_git)
    };

    assert_eq!(
        place(None, &facts, true).unwrap().0,
        WorkerPlacement::Shared
    );
    assert_eq!(
        place(Some(json!({"cwd": facts.project_path})), &facts, true)
            .unwrap()
            .0,
        WorkerPlacement::Shared
    );
    for params in [
        json!({"placement": "lane"}),
        json!({"placement": "isolated_lane"}),
        json!({"placement": "canonical_writer"}),
        json!({"cwd": "/outside-the-project-folder"}),
    ] {
        let refusal = place(Some(params.clone()), &facts, true).unwrap_err();
        assert!(
            refusal.message.contains(NO_GIT_HISTORY),
            "{params}: {}",
            refusal.message
        );
    }
    let mut build_breaking = preflight_facts(&task);
    build_breaking.project_path = facts.project_path.clone();
    build_breaking.build_breaking = true;
    assert!(place(None, &build_breaking, true)
        .unwrap_err()
        .message
        .contains(NO_GIT_HISTORY));

    assert_eq!(
        place(Some(json!({"placement": "lane"})), &facts, false)
            .unwrap()
            .0,
        WorkerPlacement::IsolatedLane
    );
    assert_eq!(
        place(None, &build_breaking, false).unwrap().0,
        WorkerPlacement::IsolatedLane
    );
    assert_eq!(
        place(None, &facts, false).unwrap().0,
        WorkerPlacement::Shared
    );
}

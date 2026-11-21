use super::*;
use crate::plan::{ExecutionIntent, TaskExecutionIntent, WorkerPlacement};
use crate::service_plan::{CanonicalWriterHoldRead, LaneIntegrationRead};

fn integration_reference(plan: &Plan, task_id: &str) -> Option<LaneIntegrationRead> {
    let task = plan.tasks.iter().find(|task| task.id == task_id)?;
    let worktree_path = task
        .lane
        .worktree_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())?;
    let (holder, attempt) = plan
        .tasks
        .iter()
        .filter(|candidate| candidate.id != task.id)
        .find_map(|candidate| {
            candidate
                .execution_attempts
                .iter()
                .find(|attempt| {
                    crate::service_plan::shared_checkout::attempt_reserves_checkout_writer(
                        candidate, attempt,
                    )
                })
                .map(|attempt| (candidate, attempt))
        })?;
    let plan_canonical = if plan.canonical_project_path.trim().is_empty() {
        plan.project_path.as_str()
    } else {
        plan.canonical_project_path.as_str()
    };
    let canonical_path =
        crate::service_plan::shared_checkout::attempt_recorded_checkout(attempt, plan_canonical);
    if canonical_path.trim().is_empty() {
        return None;
    }
    Some(LaneIntegrationRead {
        plan_id: plan.id.clone(),
        task_id: task.id.clone(),
        worktree_path: worktree_path.into(),
        canonical_path: canonical_path.into(),
        holder: CanonicalWriterHoldRead {
            task_id: holder.id.clone(),
            attempt_id: attempt.id.clone(),
            placement: attempt.placement,
        },
    })
}

fn configure(plan: &mut Plan) {
    plan.tasks[0].lane.worktree_path = Some("lane".into());
    let mut holder = plan.tasks[0].clone();
    holder.id = "holder".into();
    holder.execution_attempts[0].id = "holder-attempt".into();
    holder.execution_attempts[0].placement = WorkerPlacement::CanonicalWriter;
    if let Some(receipt) = holder.execution_attempts[0].executor_termination.as_mut() {
        receipt.attempt_id = "holder-attempt".into();
    }
    plan.tasks.push(holder);
}

#[test]
fn review_followup_preserves_latest_local_worker_and_first_canonical_hold() {
    for variant in 0..21 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = authority_plan(directory.path());
        configure(&mut plan);
        match variant {
            1 => plan.tasks[1].execution_attempts[0].closed_at = Some(1),
            2 => plan.tasks[1].execution_attempts[0].retired_at = Some(1),
            3 => plan.tasks[1].execution_attempts[0].placement = WorkerPlacement::IsolatedLane,
            4 => plan.tasks[1].execution_attempts[0].placement = WorkerPlacement::ReadOnly,
            5 => plan.tasks[1].execution_attempts[0].placement = WorkerPlacement::NonRepository,
            6 => plan.tasks[1].execution_attempts[0].placement = WorkerPlacement::Shared,
            7 => plan.tasks[0].lane.worktree_path = None,
            8 => plan.tasks[0].lane.worktree_path = Some("\u{2003}".into()),
            9 => plan.tasks[0].execution_attempts[0].placement = WorkerPlacement::Shared,
            10 => plan.tasks[0].execution_attempts[0].mesh_peer = Some("peer".into()),
            11 => plan.canonical_project_path = "\u{2009}\t".into(),
            12 => plan.canonical_project_path = " canonical ".into(),
            13 => {
                let mut first = plan.tasks[1].execution_attempts[0].clone();
                first.id = "isolated".into();
                first.placement = WorkerPlacement::IsolatedLane;
                plan.tasks[1].execution_attempts.insert(0, first);
            }
            14 => {
                let mut last = plan.tasks[0].execution_attempts[0].clone();
                last.executor_tab_id = "last".into();
                last.closed_at = Some(1);
                last.retired_at = Some(2);
                plan.tasks[0].execution_attempts.push(last);
            }
            15 => plan.tasks[1].execution_intent = TaskExecutionIntent::NonRepository,
            16 => {
                plan.tasks[1].execution_attempts[0].execution_intent =
                    Some(ExecutionIntent::ReadOnly);
            }
            17 => {
                plan.tasks[1].status = TaskStatus::Cancelled;
                plan.tasks[1].execution_attempts[0].executor_termination = None;
            }
            18 => {
                plan.tasks[1].status = TaskStatus::Cancelled;
                plan.tasks[1].execution_attempts[0]
                    .executor_termination
                    .as_mut()
                    .unwrap()
                    .session_binding_nonce = Some("mismatched".into());
            }
            19 => plan.tasks[1].status = TaskStatus::Cancelled,
            20 => {
                plan.tasks[0].execution_attempts[0].placement = WorkerPlacement::CanonicalWriter;
                plan.tasks[1].execution_attempts[0].closed_at = Some(1);
            }
            _ => {}
        }
        let database = fixtures::database(directory.path(), &plan);
        let projected = database
            .task_lane_integration_projection(&plan.id, "task_exact")
            .unwrap();
        assert_eq!(
            projected,
            integration_reference(&plan, "task_exact"),
            "variant {variant}"
        );
        if matches!(variant, 4 | 5 | 15 | 16 | 19 | 20) {
            assert!(projected.is_none(), "variant {variant} must not hold");
        }
        if matches!(variant, 0 | 6 | 17 | 18) {
            assert!(projected.is_some(), "variant {variant} must hold");
        }
        let worker = plan.tasks[0].execution_attempts.last().and_then(|attempt| {
            attempt
                .mesh_peer
                .is_none()
                .then(|| attempt.executor_tab_id.clone())
        });
        assert_eq!(
            database
                .task_review_worker_projection(&plan.id, "task_exact")
                .unwrap(),
            worker,
            "variant {variant}"
        );
        assert!(database
            .task_lane_integration_projection(&plan.id, "missing")
            .unwrap()
            .is_none());
        assert!(database
            .task_review_worker_projection("missing", "task_exact")
            .unwrap()
            .is_none());
    }
}

#[test]
fn review_followup_fixture_returns_only_selected_worker_lane_and_holder() {
    let mut prior = None;
    for (strides, attempts, body, history) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[0].lane.worktree_path = Some("lane".into());
        plan.tasks[2].execution_attempts = vec![plan.tasks[0].execution_attempts[0].clone()];
        plan.tasks[2].execution_attempts[0].placement = WorkerPlacement::CanonicalWriter;
        let original = plan.tasks[0].execution_attempts[0].clone();
        let old = (0..history)
            .map(|index| {
                let mut attempt = original.clone();
                attempt.id = format!("closed-{index}");
                attempt.closed_at = Some(2);
                attempt
            })
            .collect::<Vec<_>>();
        plan.tasks[0].execution_attempts.splice(0..0, old);
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.tasks[1].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let integration = database
            .task_lane_integration_projection(&plan.id, "task_0000")
            .unwrap()
            .unwrap();
        let worker = database
            .task_review_worker_projection(&plan.id, "task_0000")
            .unwrap();
        assert_eq!(integration.holder.task_id, "task_0002");
        assert_eq!(worker.as_deref(), Some("worker"));
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&(integration, worker)).unwrap().len();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 2048);
        assert!(decoded < 2048);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        println!("fixture={} consumer=review_followup tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unselected_target_history={history} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
        database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.placement','invalid') WHERE task_id='task_0002'",[]).unwrap();
        assert!(database
            .task_lane_integration_projection(&plan.id, "task_0000")
            .is_err());
    }
}

fn init_git_repo(root: &std::path::Path) {
    let run = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    std::fs::create_dir_all(root).unwrap();
    run(&["init"]);
    run(&["config", "user.name", "Fixture"]);
    run(&["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(root.join("README.md"), "seed").unwrap();
    run(&["add", "README.md"]);
    run(&["commit", "-m", "initial"]);
}

#[test]
fn review_followup_enforces_cross_plan_aliases_and_actual_repository_provenance() {
    let temp = tempfile::tempdir().unwrap();
    let domios = temp.path().join("domios");
    let website = temp.path().join("website");
    init_git_repo(&domios);
    init_git_repo(&website);
    let domios_lane = temp.path().join("domios-lane");
    let website_lane = temp.path().join("website-lane");
    for (repo, branch, lane) in [
        (&domios, "lane/domios", &domios_lane),
        (&website, "lane/website", &website_lane),
    ] {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["worktree", "add", "-b", branch, lane.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let mut plan_a = Plan::new(
        "Domios writer".into(),
        domios.display().to_string(),
        OrchestratorRef::default(),
    );
    let mut writer_domios =
        crate::service_plan::load_audit::task("writer_domios", TaskStatus::InProgress, Some("w1"));
    let mut writer_attempt = crate::service_plan::load_audit::attempt("attempt_domios", "w1");
    writer_attempt.placement = WorkerPlacement::CanonicalWriter;
    writer_attempt.execution_intent = Some(ExecutionIntent::Mutating);
    writer_attempt.worktree_path = Some(domios.display().to_string());
    writer_domios.execution_attempts.push(writer_attempt);
    plan_a.tasks.push(writer_domios);

    let mut plan_b = Plan::new(
        "Media review".into(),
        domios.join(".").display().to_string(),
        OrchestratorRef::default(),
    );
    let mut lane_domios_task =
        crate::service_plan::load_audit::task("task_domios_lane", TaskStatus::InReview, Some("l1"));
    lane_domios_task.lane.worktree_path = Some(domios_lane.display().to_string());
    let mut lane_website_task = crate::service_plan::load_audit::task(
        "task_website_lane",
        TaskStatus::InReview,
        Some("l2"),
    );
    lane_website_task.lane.worktree_path = Some(website_lane.display().to_string());
    plan_b.tasks = vec![lane_domios_task, lane_website_task];

    let database = fixtures::database(temp.path(), &plan_a);
    database.persist_plan_rows(&plan_b, None).unwrap();

    let domios_hold = database
        .task_lane_integration_projection(&plan_b.id, "task_domios_lane")
        .unwrap()
        .expect("live canonical writer in Plan A must block Plan B alias lane");
    assert_eq!(domios_hold.plan_id, plan_a.id);
    assert_eq!(domios_hold.holder.task_id, "writer_domios");
    assert_eq!(domios_hold.holder.attempt_id, "attempt_domios");

    assert!(
        database
            .task_lane_integration_projection(&plan_b.id, "task_website_lane")
            .unwrap()
            .is_none(),
        "external WEBSITE lane provenance must not conflict with Domios canonical writer"
    );

    let mut plan_c = Plan::new(
        "Website writer".into(),
        website.join(".").display().to_string(),
        OrchestratorRef::default(),
    );
    let mut writer_website =
        crate::service_plan::load_audit::task("writer_website", TaskStatus::InProgress, Some("w2"));
    let mut website_attempt = crate::service_plan::load_audit::attempt("attempt_website", "w2");
    website_attempt.placement = WorkerPlacement::CanonicalWriter;
    website_attempt.execution_intent = Some(ExecutionIntent::Mutating);
    writer_website.execution_attempts.push(website_attempt);
    plan_c.tasks.push(writer_website);
    database.persist_plan_rows(&plan_c, None).unwrap();

    let website_hold = database
        .task_lane_integration_projection(&plan_b.id, "task_website_lane")
        .unwrap()
        .expect("live WEBSITE canonical writer must block WEBSITE lane");
    assert_eq!(website_hold.plan_id, plan_c.id);
    assert_eq!(website_hold.holder.task_id, "writer_website");
    assert_eq!(
        crate::service_plan::shared_checkout::resolved_checkout_target(
            &website_hold.canonical_path
        ),
        crate::service_plan::shared_checkout::resolved_checkout_target(
            &website.display().to_string()
        )
    );
}

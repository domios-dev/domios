use super::*;
use crate::plan::{AgentRef, PlanEvidenceClass, PlanEvidenceReporterGrant, PlannerRef, TaskStatus};

fn grant(tab: &str) -> PlanEvidenceReporterGrant {
    PlanEvidenceReporterGrant {
        tab_id: tab.into(),
        session_id: "session".into(),
        session_binding_digest: "digest".into(),
        class: PlanEvidenceClass::Review,
        granted_by: AgentRef::default(),
        granted_at: 1,
    }
}

#[test]
fn hierarchy_report_reads_select_constant_bytes_without_writes() {
    let mut previous = None;
    for (strides, attempts, body, grants) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.owner.tab_id = Some("owner".into());
        plan.team_id = Some("team".into());
        plan.planner = Some(PlannerRef::Tab {
            tab_id: "planner".into(),
            session_id: Some("planner-session".into()),
            title: None,
        });
        plan.evidence_reporters = vec![grant("reporter")];
        plan.evidence_reporters
            .extend((0..grants).map(|_| grant("other")));
        plan.tasks[0].assigned_tab_id = Some("worker".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let authority = database
            .hierarchy_authority_projection(&plan.id, "reporter")
            .unwrap();
        let planner = database.hierarchy_planner_projection(&plan.id).unwrap();
        let identity = database.plan_identity_projection(&plan.id).unwrap();
        let held = database
            .report_tab_holds_task_projection(&plan.id, &plan.tasks[0].id, "worker")
            .unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(authority.owner, plan.owner);
        assert_eq!(authority.planner, plan.planner);
        assert_eq!(authority.team_id, plan.team_id);
        assert_eq!(authority.evidence_reporters, vec![grant("reporter")]);
        assert_eq!(planner, plan.planner);
        assert_eq!(identity.owner, plan.owner);
        assert!(held);
        assert_eq!(metrics.queries, 4);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response = serde_json::to_vec(&(authority, planner, identity, held))
            .unwrap()
            .len();
        println!("fixture={} consumers=resolve_plan_ctx,tab_holds_task,verify_planner_report_authority,classify_plan_only_report_authority tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unrelated_grants={grants} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, metrics.queries, metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn hierarchy_report_task_holds_preserve_reference_and_competition_authority() {
    for variant in 0..9 {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = Plan::new(
            "Authority".into(),
            directory.path().display().to_string(),
            OrchestratorRef::default(),
        );
        let mut task = crate::service_plan::load_audit::task("task_exact", TaskStatus::Done, None);
        task.handle = Some("named".into());
        task.key = Some("COD-9001".into());
        let mut attempt = crate::service_plan::load_audit::attempt("current", "worker");
        attempt.competition_group_id = Some(String::new());
        match variant {
            1 => attempt.closed_at = Some(1),
            2 => attempt.retired_at = Some(1),
            3 => attempt.competition_group_id = None,
            4 => attempt.executor_tab_id = "other".into(),
            5 => {
                task.assigned_tab_id = Some("worker".into());
                attempt.competition_group_id = None;
            }
            6 => plan.status = crate::plan::PlanStatus::Archived,
            7 => task.lane.tab_id = Some("worker".into()),
            _ => {}
        }
        if variant != 7 {
            task.execution_attempts.push(attempt);
        }
        plan.tasks.push(task);
        if variant == 8 {
            let mut other = plan.tasks[0].clone();
            other.id = "task_other".into();
            plan.tasks.push(other);
        }
        let database = fixtures::database(directory.path(), &plan);
        for reference in ["task_exact", "NAMED", "cod-9001", "missing"] {
            let expected =
                crate::plan::task_key::find_task(&plan.tasks, reference).is_some_and(|task| {
                    task.assigned_tab_id.as_deref() == Some("worker")
                        || task.execution_attempts.iter().any(|attempt| {
                            attempt.executor_tab_id == "worker"
                                && attempt.closed_at.is_none()
                                && attempt.retired_at.is_none()
                                && attempt.competition_group_id.is_some()
                        })
                });
            assert_eq!(
                database
                    .report_tab_holds_task_projection(&plan.id, reference, "worker")
                    .unwrap(),
                expected,
                "variant={variant} reference={reference}"
            );
        }
        assert!(!database
            .report_tab_holds_task_projection("missing", "task_exact", "worker")
            .unwrap());
    }
}

#[test]
fn hierarchy_report_authority_preserves_grant_order_and_propagates_storage_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Authority".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let first = grant("reporter");
    let mut last = first.clone();
    last.class = PlanEvidenceClass::CanonicalQa;
    plan.evidence_reporters = vec![first.clone(), grant("other"), last.clone()];
    let database = fixtures::database(directory.path(), &plan);
    assert_eq!(
        database
            .hierarchy_authority_projection(&plan.id, "reporter")
            .unwrap()
            .evidence_reporters,
        vec![first, last]
    );
    assert!(database
        .hierarchy_planner_projection(&plan.id)
        .unwrap()
        .is_none());
    assert!(matches!(
        database.hierarchy_authority_projection("missing", "reporter"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.hierarchy_planner_projection("missing"),
        Err(PlanError::NotFound)
    ));
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.planner',17) WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(matches!(
        database.hierarchy_planner_projection(&plan.id),
        Err(PlanError::Quarantined { .. })
    ));
    assert!(matches!(
        database.hierarchy_authority_projection(&plan.id, "reporter"),
        Err(PlanError::Quarantined { .. })
    ));
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_task_attempts")
        .unwrap();
    assert!(database
        .report_tab_holds_task_projection(&plan.id, "task_exact", "worker")
        .is_err());
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plans")
        .unwrap();
    assert!(database.hierarchy_planner_projection(&plan.id).is_err());
    assert!(database
        .hierarchy_authority_projection(&plan.id, "reporter")
        .is_err());
}

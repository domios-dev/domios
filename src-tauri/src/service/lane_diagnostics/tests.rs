use super::*;
use crate::agent::reports::{ReportDetails, ReportStatus};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{OrchestratorRef, Plan};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn lane_diagnostics_lane_path_never_allows_a_watcher_but_canonical_does() {
    assert!(!analysis_allowed(analysis_scope(
        "C:/repo",
        "C:/repo-worktrees/.diagnostics",
        Some("C:/repo-worktrees/lane-a"),
        Some("C:/repo-worktrees/lane-a"),
        "C:/repo-worktrees/lane-a",
    )));
    assert!(analysis_allowed(analysis_scope(
        "C:/repo",
        "C:/repo-worktrees/.diagnostics",
        Some("C:/repo-worktrees/lane-a"),
        Some("C:/repo-worktrees/lane-a"),
        "C:/repo",
    )));
}

#[test]
fn lane_diagnostics_cargo_json_missing_import_keeps_primary_location_and_code() {
    let output = r#"CODETERM_DIAGNOSTIC_TOOL	cargo-check
{"reason":"compiler-message","message":{"level":"error","message":"cannot find value `x`","code":{"code":"E0425"},"spans":[{"file_name":"src/main.rs","line_start":7,"column_start":9,"is_primary":true}]}}"#;
    let findings = parse_diagnostics(output, &[]);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file.as_deref(), Some("src/main.rs"));
    assert_eq!(findings[0].line, Some(7));
    assert_eq!(findings[0].code.as_deref(), Some("E0425"));
}

#[test]
fn lane_diagnostics_filter_to_changed_source_files() {
    let output = concat!(
        "CODETERM_DIAGNOSTIC_TOOL\tcargo-check\n",
        r#"{"reason":"compiler-message","message":{"level":"warning","message":"changed","spans":[{"file_name":"src/changed.rs","line_start":2,"is_primary":true}]}}"#,
        "\n",
        r#"{"reason":"compiler-message","message":{"level":"warning","message":"unrelated","spans":[{"file_name":"src/other.rs","line_start":9,"is_primary":true}]}}"#,
    );
    let findings = parse_diagnostics(output, &["src-tauri/src/changed.rs".into()]);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].file.as_deref(), Some("src/changed.rs"));
}

#[test]
fn lane_diagnostics_clean_tool_output_does_not_create_findings() {
    let output = concat!(
        "CODETERM_DIAGNOSTIC_TOOL\tcargo-check\nChecking codeterm\n",
        "CODETERM_DIAGNOSTIC_EXIT\tcargo-check\t0\n",
        "CODETERM_DIAGNOSTIC_TOOL\twebforks-gate\nweb fork inventory is fresh\n",
        "CODETERM_DIAGNOSTIC_EXIT\twebforks-gate\t0\n",
    );
    assert!(parse_diagnostics(output, &[]).is_empty());
}

#[test]
fn lane_diagnostics_typescript_type_mismatch_keeps_file_line_column_and_code() {
    let findings = parse_diagnostics(
        "CODETERM_DIAGNOSTIC_TOOL\ttsc\nsrc/app.ts(12,4): error TS2322: type mismatch",
        &[],
    );
    assert_eq!(findings[0].file.as_deref(), Some("src/app.ts"));
    assert_eq!(findings[0].line, Some(12));
    assert_eq!(findings[0].column, Some(4));
    assert_eq!(findings[0].code.as_deref(), Some("TS2322"));
}

#[test]
fn lane_diagnostics_fmt_and_gate_findings_keep_their_tool_identity() {
    let findings = parse_diagnostics(
        "CODETERM_DIAGNOSTIC_TOOL\tfmt\nDiff in src/lib.rs at line 3:\nCODETERM_DIAGNOSTIC_TOOL\twebforks-gate\nmissing inventory entry",
        &["src/lib.rs".into()],
    );
    assert_eq!(findings[0].file.as_deref(), Some("src/lib.rs"));
    assert_eq!(findings[0].line, Some(3));
    assert_eq!(findings[1].tool, "webforks-gate");
}

#[test]
fn lane_diagnostics_touched_crates_are_unique_and_diff_scoped() {
    let crates = touched_crates(&[
        "src-tauri/src/lib.rs".into(),
        "src-tauri/crates/codeterm-plan/src/lib.rs".into(),
        "src-tauri/crates/codeterm-plan/src/other.rs".into(),
        "cli/src/main.rs".into(),
    ]);
    assert_eq!(crates, vec!["app", "plan", "cli"]);
}

#[test]
fn lane_diagnostics_summary_round_trips_counts_and_locations() {
    let summary = LaneDiagnosticsSummary::from_findings(
        "plan".into(),
        "task".into(),
        "attempt".into(),
        "warm".into(),
        vec!["cargo-check".into()],
        vec![
            LaneDiagnosticFinding {
                tool: "cargo-check".into(),
                severity: "error".into(),
                file: Some("src/lib.rs".into()),
                line: Some(4),
                column: Some(2),
                message: "missing import".into(),
                code: Some("E0432".into()),
            },
            LaneDiagnosticFinding {
                tool: "fmt".into(),
                severity: "warning".into(),
                ..Default::default()
            },
        ],
    );
    let json = serde_json::to_string(&summary).unwrap();
    let round_trip: LaneDiagnosticsSummary = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.error_count, 1);
    assert_eq!(round_trip.warning_count, 1);
    assert_eq!(round_trip.findings[0].line, Some(4));
    assert_eq!(round_trip.findings[0].code.as_deref(), Some("E0432"));
}

#[test]
fn lane_diagnostics_incomplete_diagnostics_are_explicitly_bounded() {
    let summary = LaneDiagnosticsSummary::incomplete(
        "plan".into(),
        "task".into(),
        "attempt".into(),
        "warm".into(),
        "deadline exceeded".into(),
    );
    assert!(summary.incomplete);
    assert_eq!(summary.status, "incomplete");
    assert_eq!(summary.warning_count, 1);
}

#[test]
fn lane_diagnostics_busy_gateway_keeps_owner_and_command() {
    let reason = gateway_busy_reason(
        "Build gateway busy: job=job-1 owner=dev command=build.sh dev\nCommand was not started",
    )
    .unwrap();
    let summary = LaneDiagnosticsSummary::skipped(
        "plan".into(),
        "task".into(),
        "attempt".into(),
        "warm".into(),
        reason,
    );
    assert_eq!(summary.status, "skipped");
    assert!(!summary.incomplete);
    assert_eq!(summary.findings[0].tool, "build-gateway");
    assert!(summary.findings[0].message.contains("owner=dev"));
    assert!(summary.findings[0].message.contains("command=build.sh dev"));
}

#[test]
fn lane_diagnostics_finished_run_targets_both_tabs_once() {
    assert_eq!(
        diagnostics_followup_targets(Some("orchestrator"), "worker"),
        vec!["orchestrator", "worker"]
    );
    assert_eq!(
        diagnostics_followup_targets(Some("orchestrator"), "orchestrator"),
        vec!["orchestrator"]
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn lane_diagnostics_report_returns_before_blocked_runner() {
    let (mut mux, orchestrator) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&orchestrator))
        .unwrap();
    let mut fixture = ServiceFixture::new(mux);
    let canonical = fixture.temp.path().join("canonical");
    let lane = fixture.temp.path().join("lane");
    std::fs::create_dir_all(&canonical).unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&worker).unwrap();
        tab.agent_session_id = Some("worker-session".into());
        tab.session_binding_nonce = Some("worker-nonce".into());
        tab.reported_session_binding_nonce = Some("worker-nonce".into());
        tab.provisioned_cwd = Some(lane.display().to_string());
    }
    let mut plan = Plan::new(
        "diagnostics report".into(),
        canonical.display().to_string(),
        OrchestratorRef::from_tab(&orchestrator),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "diagnostics task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .assign_task(&plan.id, &task_id, &worker, None, 3)
        .unwrap();
    fixture
        .service
        .plan_store
        .with_plan(&plan.id, 4, |plan| {
            let task = plan.tasks.iter_mut().find(|task| task.id == task_id).unwrap();
            task.lane.worktree_path = Some(lane.display().to_string());
            Ok(())
        })
        .unwrap();
    fixture
        .service
        .plan_store
        .update_attempt_executor_identity(
            &plan.id,
            &task_id,
            &worker,
            None,
            Some("worker-session"),
            Some("worker-nonce"),
            5,
        )
        .unwrap();
    let attempt_id = fixture
        .service
        .plan_store
        .get(&plan.id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap()
        .execution_attempts
        .into_iter()
        .next()
        .unwrap()
        .id;
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let runner = Arc::new(move |request: DiagnosticsRequest, _: Duration| {
        started_tx.send(request.attempt_id.clone()).unwrap();
        release_rx.recv().unwrap();
        Ok(LaneDiagnosticsSummary::from_findings(
            request.plan_id,
            request.task_id,
            request.attempt_id,
            "warm".into(),
            vec!["fake".into()],
            Vec::new(),
        ))
    });
    fixture.service.lane_diagnostics = LaneDiagnosticsService::with_runner_for_test(
        fixture.temp.path().join("diagnostics"),
        runner,
    );
    let service = fixture.service.clone();
    let (returned_tx, returned_rx) = mpsc::channel();
    let report_plan = plan.id.clone();
    let report_task = task_id.clone();
    let report_worker = worker.clone();
    let report_attempt = attempt_id.clone();
    let report = std::thread::spawn(move || {
        let result = service.submit_report(
            &report_worker,
            "report delivered",
            ReportDetails {
                status: Some(ReportStatus::InReview),
                attempt_id: Some(report_attempt),
                caller_tab_id: Some(report_worker.clone()),
                caller_session_id: Some("worker-session".into()),
                session_binding_nonce: Some("worker-nonce".into()),
                owner_epoch: Some(0),
                ..Default::default()
            },
            Some(&report_plan),
            Some(&report_task),
            None,
        );
        returned_tx.send(result.is_ok()).unwrap();
        result
    });
    assert_eq!(
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        attempt_id
    );
    assert!(returned_rx.recv_timeout(Duration::from_secs(1)).unwrap());
    release_tx.send(()).unwrap();
    assert!(report.join().unwrap().is_ok());
}

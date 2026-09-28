use super::*;
use crate::client::{RpcError, SocketCallError};
use std::collections::VecDeque;

/// Tokens no CLI code path can synthesize: if one reaches rendered output it
/// travelled through the daemon response, which is the whole contract.
const DAEMON_PLAN_PROJECT: &str = "<daemon-reported-plan-project>";
const DAEMON_CALLER_PROJECT: &str = "<daemon-reported-caller-project>";

#[derive(Default)]
struct FakeRpc {
    calls: Vec<(String, Option<Value>)>,
    responses: VecDeque<Value>,
    failure: Option<(String, SocketCallError)>,
}

impl FakeRpc {
    fn scripted<const N: usize>(responses: [Value; N]) -> Self {
        Self {
            calls: vec![],
            responses: VecDeque::from(responses),
            failure: None,
        }
    }

    fn refusing_after(method: &str, response: Value, error: SocketCallError) -> Self {
        Self {
            responses: VecDeque::from([response]),
            failure: Some((method.to_string(), error)),
            ..Self::default()
        }
    }

    fn methods(&self) -> Vec<&str> {
        self.calls
            .iter()
            .map(|(method, _)| method.as_str())
            .collect()
    }
}

impl RpcClient for FakeRpc {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, SocketCallError> {
        self.calls.push((method.to_string(), params));
        if self
            .failure
            .as_ref()
            .is_some_and(|(expected, _)| expected == method)
        {
            let (_, error) = self.failure.take().expect("matching failure remains");
            return Err(error);
        }
        if let Some(scripted) = self.responses.pop_front() {
            return Ok(scripted);
        }
        Ok(json!({
            "task": { "id": "task_1", "name": "probe", "status": "pending" },
            "plan_id": "pl_1",
            "plan_title": "Probe plan"
        }))
    }
}

#[test]
fn plan_task_dispatch_json_error_keeps_invalid_model_details() {
    let data = json!({
        "type": "invalid_model",
        "provider": "codex",
        "requested_model": "gpt-9-bogus",
        "closest_model_ids": ["gpt-6-sol", "gpt-6-astra"],
        "refresh_command": "codeterm agent models codex",
        "message": "unknown model"
    });
    let mut client = FakeRpc::refusing_after(
        "plan.task.dispatch",
        json!({ "task": { "interruption": { "owner_epoch": 17 } } }),
        SocketCallError::Rpc(RpcError {
            code: -32602,
            message: "unknown model".into(),
            data: Some(data),
        }),
    );
    let error = run_dispatch(
        &mut client,
        true,
        PlanTaskAction::Dispatch {
            plan_id: "plan_a".into(),
            task_id: Some("task_a".into()),
            agent: Some("codex".into()),
            authorized_gates: Vec::new(),
            canonical_writer: false,
            model: Some("gpt-9-bogus".into()),
            reasoning_effort: None,
            parent: None,
            workspace: None,
            cwd: None,
            execution_intent: "read_only".into(),
            provider_reserve_pct: 0.0,
        },
    )
    .expect_err("unknown authoritative model is a structured dispatch error");
    let mut out = Vec::new();
    let mut err = Vec::new();
    crate::cmd::print::write_cli_error(true, &error, &mut out, &mut err);

    assert!(err.is_empty());
    assert_eq!(client.calls[1].0, "plan.task.dispatch");
    let output: Value = serde_json::from_slice(&out).expect("one JSON error envelope");
    assert_eq!(output["ok"], false);
    assert_eq!(output["kind"], "error");
    assert_eq!(output["data"]["type"], "invalid_model");
    assert_eq!(output["data"]["provider"], "codex");
    assert_eq!(output["data"]["requested_model"], "gpt-9-bogus");
    assert_eq!(output["data"]["closest_model_ids"][0], "gpt-6-sol");
    assert_eq!(
        output["data"]["refresh_command"],
        "codeterm agent models codex"
    );
    assert_eq!(output["code"], -32602);
}

fn list_action(status: Option<&str>) -> PlanTaskAction {
    PlanTaskAction::List {
        plan_id: Some("pl_1".into()),
        status: status.map(str::to_string),
        all: false,
    }
}

fn empty_with_reason(reason: Value) -> Value {
    json!({ "tasks": [], "empty_reason": reason })
}

fn mismatch_reason() -> Value {
    json!({
        "kind": EMPTY_REASON_PLAN_OUTSIDE_PROJECT,
        "plan_id": "pl_1",
        "plan_project": DAEMON_PLAN_PROJECT,
        "requested_project": DAEMON_CALLER_PROJECT,
    })
}

#[test]
fn plan_task_show_routes_to_task_show_rpc_with_plan_and_task() {
    let mut client = FakeRpc::default();
    let out = run_task(
        &mut client,
        false,
        PlanTaskAction::Show {
            plan_id: "pl_1".into(),
            task_id: "task_1".into(),
            siblings: false,
            all: false,
        },
    )
    .expect("show runs");
    assert!(out.is_none(), "human render returns no value to reprint");
    assert_eq!(client.calls.len(), 1);
    assert_eq!(client.calls[0].0, "task.show");
    let params = client.calls[0].1.as_ref().expect("params");
    assert_eq!(params["plan_id"], "pl_1");
    assert_eq!(params["task_id"], "task_1");
}

#[test]
fn plan_task_show_json_returns_raw_rpc_value() {
    let mut client = FakeRpc::default();
    let out = run_task(
        &mut client,
        true,
        PlanTaskAction::Show {
            plan_id: "pl_1".into(),
            task_id: "task_1".into(),
            siblings: false,
            all: false,
        },
    )
    .expect("show runs")
    .expect("json returns the value");
    assert_eq!(out["task"]["id"], "task_1");
    assert_eq!(client.calls[0].0, "task.show");
}

#[test]
fn split_creates_children_with_parent_and_inherited_routing() {
    let mut client = FakeRpc::scripted([
        json!({ "task": { "interruption": { "owner_epoch": 3 } } }),
        json!({
            "parent_task_id": "parent",
            "children": [{"task_id": "child_a"}, {"task_id": "child_b"}]
        }),
    ]);
    let response = run_task(
        &mut client,
        true,
        PlanTaskAction::Split {
            plan_id: "pl_1".into(),
            task_id: "parent".into(),
            children: vec!["Part A".into(), "Part B".into()],
            from_file: None,
        },
    )
    .unwrap()
    .unwrap();

    assert_eq!(client.methods().last().copied(), Some("plan.task.split"));
    let params = client.calls.last().unwrap().1.as_ref().unwrap();
    assert_eq!(params["task_id"], "parent");
    assert_eq!(params["children"][0]["name"], "Part A");
    assert_eq!(params["children"][1]["name"], "Part B");
    assert_eq!(response["parent_task_id"], "parent");
    assert_eq!(response["children"].as_array().unwrap().len(), 2);
}

#[test]
fn criterion_file_parses_json_array_form() {
    let items = parse_criteria_file("[\"first\", \"second bullet\"]");
    assert_eq!(items, vec!["first", "second bullet"]);
}

#[test]
fn criterion_file_parses_one_per_line_form_dropping_blanks() {
    let items = parse_criteria_file("first\n\n  second bullet  \n");
    assert_eq!(items, vec!["first", "second bullet"]);
}

#[test]
fn merge_criteria_appends_file_content_to_flag_values() {
    let merged = merge_criteria(vec!["from-flag".into()], None).unwrap();
    assert_eq!(merged, vec!["from-flag"]);
}

#[test]
fn awaiting_dispatch_ready_is_not_rendered_as_completed_dispatch() {
    let response = json!({
        "accepted": true,
        "state": "awaiting_dispatch_ready",
        "tab_id": "tab-1",
        "agent": "codex",
        "readiness": {
            "ready": false,
            "awaiting_event": "dispatch_ready"
        }
    });

    let rendered = render_dispatch_response(&response);

    assert!(!rendered.contains("Dispatched"), "{rendered}");
    assert!(rendered.contains("awaiting dispatch_ready"), "{rendered}");
}

#[test]
fn spawned_worker_is_not_rendered_as_an_accepted_dispatch() {
    let response = json!({
        "accepted": false,
        "spawned": true,
        "state": "awaiting_worker_ack",
        "tab_id": "tab-1",
        "agent": "codex",
        "readiness": {
            "ready": false,
            "awaiting_event": "worker_ack"
        }
    });

    let rendered = render_dispatch_response(&response);

    assert!(!rendered.contains("Dispatched"), "{rendered}");
    assert!(!rendered.contains("Accepted"), "{rendered}");
    assert!(
        rendered.contains("Spawned codex worker Tab tab-1"),
        "{rendered}"
    );
    assert!(rendered.contains("awaiting worker_ack"), "{rendered}");
}

#[test]
fn hidden_summary_names_each_hidden_status_and_all_flag() {
    let tasks = vec![
        json!({ "task": { "status": "pending" } }),
        json!({ "task": { "status": "paused" } }),
        json!({ "task": { "status": "done" } }),
        json!({ "task": { "status": "done" } }),
        json!({ "task": { "status": "cancelled" } }),
    ];
    let summary = hidden_task_summary(1, &tasks).unwrap();
    assert!(summary.contains("1 shown; 4 hidden"), "{summary}");
    assert!(summary.contains("paused=1"), "{summary}");
    assert!(summary.contains("done=2"), "{summary}");
    assert!(summary.contains("cancelled=1"), "{summary}");
    assert!(summary.contains("Use --all"), "{summary}");
}

/// ct#384: a plan outside the requested project is a refusal, not an empty
/// result — rendering it at exit 0 is exactly how a mis-rooted plan looked
/// healthy through an entire drafting pass. `run_task` must surface it as an
/// `Err` (which the CLI's `handle_plan` turns into a non-zero exit) naming the
/// machine-readable reason code, still built from the daemon's own report.
#[test]
fn plan_outside_project_is_a_refusal_not_an_empty_result() {
    let mut client = FakeRpc::scripted([empty_with_reason(mismatch_reason())]);

    let error = run_task(&mut client, true, list_action(Some("done")))
        .expect_err("a plan outside its project must refuse, not report success");

    assert_eq!(client.methods(), vec!["task.list"]);
    assert!(
        error.starts_with(EMPTY_REASON_PLAN_OUTSIDE_PROJECT),
        "{error}"
    );
    assert!(error.contains(DAEMON_PLAN_PROJECT), "{error}");
    assert!(error.contains(DAEMON_CALLER_PROJECT), "{error}");
}

#[test]
fn filter_miss_never_renders_as_a_project_mismatch() {
    let mismatch = render_empty_reason(&mismatch_reason()).expect("mismatch renders");
    let miss = render_empty_reason(&json!({ "kind": EMPTY_REASON_NO_MATCH_FOR_FILTERS }))
        .expect("filter miss renders");

    assert_ne!(mismatch, miss);
    assert!(mismatch.contains(DAEMON_PLAN_PROJECT), "{mismatch}");
    assert!(!miss.contains(DAEMON_PLAN_PROJECT), "{miss}");
    assert!(!miss.contains(DAEMON_CALLER_PROJECT), "{miss}");
}

/// These reason kinds mean "nothing matched", not "the request was refused":
/// the plan was found, in scope, and listable — exit 0 with a human hint
/// stays correct for all of them.
#[test]
fn empty_scope_and_filter_reasons_render_a_message_and_exit_zero() {
    for reason in [
        json!({ "kind": EMPTY_REASON_NO_PLANS_IN_SCOPE, "requested_project": DAEMON_CALLER_PROJECT }),
        json!({ "kind": EMPTY_REASON_NO_TASKS_IN_SCOPE, "plan_id": "pl_1" }),
        json!({ "kind": EMPTY_REASON_ALL_TASKS_INACTIVE, "hidden": 4 }),
        json!({ "kind": EMPTY_REASON_NO_MATCH_FOR_FILTERS }),
    ] {
        let kind = reason["kind"].as_str().expect("kind").to_string();
        let mut client = FakeRpc::scripted([empty_with_reason(reason)]);

        let out = run_task(&mut client, true, list_action(Some("done")))
            .unwrap_or_else(|error| panic!("{kind} must exit 0, got error: {error}"))
            .expect("json returns the value");

        assert!(out["task_list_message"].is_string(), "{kind}");
    }
}

/// ct#384 + the exit-0 sweep: `plan_not_found`, `plan_archived`, and
/// `plan_outside_project` name a plan the caller explicitly asked for that
/// cannot be listed as requested. Each must refuse (non-zero exit) and name
/// its own reason code — never collapse into a friendly empty list.
#[test]
fn plan_scoped_refusal_reasons_error_and_name_their_code() {
    for reason in [
        json!({ "kind": EMPTY_REASON_PLAN_NOT_FOUND, "plan_id": "pl_1" }),
        json!({ "kind": EMPTY_REASON_PLAN_ARCHIVED, "plan_id": "pl_1" }),
        mismatch_reason(),
    ] {
        let kind = reason["kind"].as_str().expect("kind").to_string();
        let mut client = FakeRpc::scripted([empty_with_reason(reason)]);

        let error = run_task(&mut client, true, list_action(Some("done")))
            .expect_err(&format!("{kind} must refuse, not report success"));

        assert!(error.starts_with(&kind), "{kind}: {error}");
    }
}

#[test]
fn an_unknown_daemon_reason_is_passed_through_without_invention() {
    let reason = json!({ "kind": "kind_from_a_newer_daemon" });
    let mut client = FakeRpc::scripted([empty_with_reason(reason.clone())]);

    let out = run_task(&mut client, true, list_action(Some("done")))
        .expect("unknown reason is not a failure")
        .expect("json returns the value");

    assert!(out["task_list_message"].is_null());
    assert_eq!(out["empty_reason"], reason);
}

#[test]
fn hidden_task_summary_replaces_the_inactive_only_message() {
    let mut client = FakeRpc::scripted([
        empty_with_reason(json!({ "kind": EMPTY_REASON_ALL_TASKS_INACTIVE, "hidden": 2 })),
        json!({ "tasks": [
            { "task": { "status": "done" } },
            { "task": { "status": "done" } }
        ] }),
    ]);

    let out = run_task(&mut client, true, list_action(None))
        .expect("empty active list is a valid result")
        .expect("json returns the value");

    assert_eq!(client.methods(), vec!["task.list", "task.list"]);
    let summary = out["task_list_summary"].as_str().expect("summary");
    assert!(summary.contains("done=2"), "{summary}");
    assert!(out["task_list_message"].is_null());
}

fn assert_attempt_snapshot(attempt: Value, expected: &str) {
    let rendered = render_attempts(&json!({ "attempts": [attempt] }));

    assert_eq!(rendered, expected);
    assert!(
        !rendered.split_whitespace().any(|field| field == "at="),
        "{rendered}"
    );
}

#[test]
fn assigned_only_attempt_labels_each_timestamp() {
    assert_attempt_snapshot(
        json!({
            "attempt_id": "attempt-assigned",
            "executor_tab_id": "tab-worker",
            "status": "assigned",
            "provider": "codex",
            "model": "gpt-test",
            "reasoning_effort": "xhigh",
            "session_model": "gpt-runtime",
            "assigned_at": 100,
            "acknowledged_at": null,
            "reported_at": null,
            "closed_at": null,
        }),
        "attempt_id=attempt-assigned executor_tab_id=tab-worker status=assigned provider=codex model=gpt-test requested_model=- provider_resolved_model=- reasoning_effort=xhigh session_model=gpt-runtime\n  assigned_at=100 acknowledged_at=none reported_at=none closed_at=none\n",
    );
}

#[test]
fn acknowledged_in_progress_attempt_separates_ack_from_assignment() {
    assert_attempt_snapshot(
        json!({
            "attempt_id": "attempt-acknowledged",
            "executor_tab_id": "tab-worker",
            "status": "assigned",
            "provider": "codex",
            "model": "gpt-test",
            "assigned_at": 100,
            "acknowledged_at": 110,
            "reported_at": null,
            "closed_at": null,
        }),
        "attempt_id=attempt-acknowledged executor_tab_id=tab-worker status=assigned provider=codex model=gpt-test requested_model=- provider_resolved_model=- reasoning_effort=- session_model=-\n  assigned_at=100 acknowledged_at=110 reported_at=none closed_at=none\n",
    );
}

#[test]
fn reported_partial_attempt_separates_report_from_ack() {
    assert_attempt_snapshot(
        json!({
            "attempt_id": "attempt-partial",
            "executor_tab_id": "tab-worker",
            "status": "partial",
            "provider": "codex",
            "model": "gpt-test",
            "assigned_at": 100,
            "acknowledged_at": 110,
            "reported_at": 120,
            "closed_at": null,
        }),
        "attempt_id=attempt-partial executor_tab_id=tab-worker status=partial provider=codex model=gpt-test requested_model=- provider_resolved_model=- reasoning_effort=- session_model=-\n  assigned_at=100 acknowledged_at=110 reported_at=120 closed_at=none\n",
    );
}

#[test]
fn closed_attempt_labels_its_closure_separately() {
    assert_attempt_snapshot(
        json!({
            "attempt_id": "attempt-closed",
            "executor_tab_id": "tab-worker",
            "status": "closed",
            "provider": "codex",
            "model": "gpt-test",
            "assigned_at": 100,
            "acknowledged_at": 110,
            "reported_at": 120,
            "closed_at": 130,
        }),
        "attempt_id=attempt-closed executor_tab_id=tab-worker status=closed provider=codex model=gpt-test requested_model=- provider_resolved_model=- reasoning_effort=- session_model=-\n  assigned_at=100 acknowledged_at=110 reported_at=120 closed_at=130\n",
    );
}

#[test]
fn attempts_json_returns_explicit_timestamp_fields_unchanged() {
    let response = json!({
        "attempts": [{
            "attempt_id": "attempt-json",
            "provider": "codex",
            "model": "gpt-6-luna",
            "requested_model": "gpt-6-luna",
            "provider_resolved_model": "gpt-6-luna",
            "reasoning_effort": "xhigh",
            "session_model": "gpt-6-luna",
            "model_identity_mismatch": {
                "requested_model": "gpt-6-luna",
                "provider_resolved_model": "gpt-6-luna",
                "actual_model": "other-model",
                "reason": "runtime_session_model_mismatch",
            },
            "assigned_at": 100,
            "acknowledged_at": 110,
            "reported_at": null,
            "closed_at": null,
        }]
    });
    let mut client = FakeRpc::scripted([response.clone()]);

    let out = run_task(
        &mut client,
        true,
        PlanTaskAction::Attempts {
            plan_id: "pl_1".into(),
            task_id: "task_1".into(),
        },
    )
    .expect("attempts run")
    .expect("json returns the raw response");

    assert_eq!(out, response);
    assert_eq!(out["attempts"][0]["assigned_at"], 100);
    assert_eq!(out["attempts"][0]["acknowledged_at"], 110);
    assert_eq!(out["attempts"][0]["reported_at"], Value::Null);
    assert_eq!(out["attempts"][0]["closed_at"], Value::Null);
    assert_eq!(out["attempts"][0]["provider"], "codex");
    assert_eq!(out["attempts"][0]["model"], "gpt-6-luna");
    assert_eq!(out["attempts"][0]["reasoning_effort"], "xhigh");
    assert_eq!(out["attempts"][0]["session_model"], "gpt-6-luna");
}

#[test]
fn attempt_detail_labels_dispatched_and_session_models() {
    let rendered = render_attempt(&json!({
        "attempt": {
            "provider": "codex",
            "model": "gpt-6-luna",
            "requested_model": "gpt-6-luna",
            "provider_resolved_model": "gpt-6-luna",
            "reasoning_effort": "xhigh",
            "session_model": "other-model",
            "model_identity_mismatch": {
                "requested_model": "gpt-6-luna",
                "provider_resolved_model": "gpt-6-luna",
                "actual_model": "other-model",
                "reason": "runtime_session_model_mismatch",
            },
        }
    }));

    assert!(rendered.contains("provider: codex\n"), "{rendered}");
    assert!(rendered.contains("model: gpt-6-luna\n"), "{rendered}");
    assert!(
        rendered.contains("requested_model: gpt-6-luna\n"),
        "{rendered}"
    );
    assert!(
        rendered.contains("provider_resolved_model: gpt-6-luna\n"),
        "{rendered}"
    );
    assert!(rendered.contains("reasoning_effort: xhigh\n"), "{rendered}");
    assert!(
        rendered.contains("session_model: other-model\n"),
        "{rendered}"
    );
    assert!(rendered.contains("model_identity_mismatch: {\"actual_model\":\"other-model\",\"provider_resolved_model\":\"gpt-6-luna\",\"reason\":\"runtime_session_model_mismatch\",\"requested_model\":\"gpt-6-luna\"}\n"), "{rendered}");
}

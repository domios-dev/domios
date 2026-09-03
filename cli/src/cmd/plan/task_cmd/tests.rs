use super::*;
use crate::client::SocketCallError;
use std::collections::VecDeque;

/// Tokens no CLI code path can synthesize: if one reaches rendered output it
/// travelled through the daemon response, which is the whole contract.
const DAEMON_PLAN_PROJECT: &str = "<daemon-reported-plan-project>";
const DAEMON_CALLER_PROJECT: &str = "<daemon-reported-caller-project>";

#[derive(Default)]
struct FakeRpc {
    calls: Vec<(String, Option<Value>)>,
    responses: VecDeque<Value>,
}

impl FakeRpc {
    fn scripted<const N: usize>(responses: [Value; N]) -> Self {
        Self {
            calls: vec![],
            responses: VecDeque::from(responses),
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

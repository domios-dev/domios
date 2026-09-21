//! A stale identity must not mutate plan state it no longer owns.
//!
//! The defect cases assert the STORED STATE of the task the caller does not
//! hold, not a return value: a guard that refuses and writes anyway would still
//! satisfy an error assertion. Both external entrances run against the same
//! fixture, so a socket-only fix cannot pass, and the legitimate-path controls
//! fail if the guard is too broad.

use serde_json::{json, Value};

use super::{apply_batch_with_authority, BatchAuthority, BatchWriteError};
use crate::api::plan_routes::batch::apply_authorized_batch;
use crate::plan::{OrchestratorRef, Plan, Task};
use crate::service::hierarchy::report_plan_ctx::STALE_TASK_HINT_REFUSAL;
use crate::service::plan::batch::{BatchOp, BatchRequest};
use crate::service::plan::{NewTask, PlanStore};
use crate::socket::server::plan::apply_batch_rpc;

const OWNER: &str = "orch";
const WORKER: &str = "worker";
const SUCCESSOR: &str = "successor";

struct Fixture {
    _dir: tempfile::TempDir,
    store: PlanStore,
    plan_id: String,
    /// The task the worker was moved OFF; another tab holds it now.
    foreign: String,
    /// The task the worker actually holds.
    held: String,
}

impl Fixture {
    fn task(&self, task_id: &str) -> Task {
        self.store
            .get(&self.plan_id)
            .unwrap()
            .tasks
            .into_iter()
            .find(|task| task.id == task_id)
            .unwrap()
    }

    /// Everything a stale write would move, minus the ids and clocks the store
    /// mints: status, body, sections, then each milestone in order.
    fn stored_state(&self, task_id: &str) -> String {
        let task = self.task(task_id);
        let milestones = task
            .milestones
            .iter()
            .map(|item| format!("{}|{}", item.kind, item.text))
            .collect::<Vec<_>>()
            .join(";");
        format!(
            "{:?}/{}/{}/{}",
            task.status,
            task.body,
            task.section_ids.join(","),
            milestones
        )
    }
}

fn add_task(store: &PlanStore, plan_id: &str, name: &str, now_ms: u64) -> String {
    store
        .add_task(
            plan_id,
            NewTask {
                name: name.into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["the boundary holds".into()],
                test_plan: Some("focused regression".into()),
                ..Default::default()
            },
            now_ms,
        )
        .unwrap()
}

/// One fixture for both entrances: `WORKER` was moved off `foreign` onto `held`,
/// leaving it with a task id the daemon has since reassigned.
fn moved_off_a_task() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "authority boundary".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(OWNER),
    );
    store.save(&mut plan, 1).unwrap();
    let foreign = add_task(&store, &plan.id, "moved on", 2);
    let held = add_task(&store, &plan.id, "actually held", 2);
    store
        .assign_task(&plan.id, &foreign, SUCCESSOR, None, 3)
        .unwrap();
    store.assign_task(&plan.id, &held, WORKER, None, 4).unwrap();
    Fixture {
        _dir: dir,
        store,
        plan_id: plan.id,
        foreign,
        held,
    }
}

/// Exactly what `codeterm plan record --task <id> --status done --body-file …
/// --sections … --step …` puts on the wire.
fn record_ops(task_id: &str, marker: &str) -> Value {
    json!([
        {
            "op": "set_task_sections",
            "client_id": "task",
            "task": task_id,
            "sections": [format!("sec_{marker}")],
        },
        { "op": "set_task_status", "client_id": "task", "task": task_id, "status": "done" },
        { "op": "set_task_body", "task": task_id, "body_md": format!("{marker} BODY") },
        {
            "op": "add_milestone",
            "client_id": "step_0",
            "task": task_id,
            "kind": "status_update",
            "text": format!("{marker} MILESTONE"),
            "from": { "tab_id": WORKER },
        },
    ])
}

fn socket_params(task_id: &str, marker: &str, caller_tab_id: Option<&str>) -> Value {
    let mut params = json!({ "key": format!("k_{marker}"), "ops": record_ops(task_id, marker) });
    if let Some(tab) = caller_tab_id {
        params["caller_tab_id"] = json!(tab);
    }
    params
}

fn http_body(task_id: &str, marker: &str, caller_tab_id: Option<&str>) -> Value {
    socket_params(task_id, marker, caller_tab_id)
}

#[test]
fn the_socket_entrance_refuses_a_stale_caller_and_writes_nothing() {
    let fx = moved_off_a_task();
    let before = fx.stored_state(&fx.foreign);

    let error = apply_batch_rpc(
        &fx.store,
        &fx.plan_id,
        &socket_params(&fx.foreign, "SOCKET_HIJACK", Some(WORKER)),
        10,
    )
    .expect_err("a tab that no longer holds the task must not write it");

    assert!(
        error.message.contains(STALE_TASK_HINT_REFUSAL),
        "the refusal must be observable to the caller: {}",
        error.message
    );
    assert_eq!(
        fx.stored_state(&fx.foreign),
        before,
        "the foreign task's stored state must be untouched: no status, no body, no sections, no milestone"
    );
    assert!(fx.task(&fx.foreign).milestones.is_empty());
}

#[test]
fn the_http_entrance_refuses_the_same_stale_caller_and_writes_nothing() {
    let fx = moved_off_a_task();
    let before = fx.stored_state(&fx.foreign);

    let (status, payload) = apply_authorized_batch(
        &fx.store,
        &fx.plan_id,
        &http_body(&fx.foreign, "HTTP_HIJACK", Some(WORKER)),
        11,
    )
    .expect_err("a permitted client is not thereby the task's holder");

    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
    assert!(payload.0["error"]
        .as_str()
        .unwrap()
        .contains(STALE_TASK_HINT_REFUSAL));
    assert_eq!(
        fx.stored_state(&fx.foreign),
        before,
        "the foreign task's stored state must be untouched at this entrance too"
    );
    assert!(fx.task(&fx.foreign).milestones.is_empty());
}

#[test]
fn the_tab_that_holds_the_task_records_normally_at_both_entrances() {
    let socket = moved_off_a_task();
    apply_batch_rpc(
        &socket.store,
        &socket.plan_id,
        &socket_params(&socket.held, "SOCKET_OK", Some(WORKER)),
        12,
    )
    .expect("the holder's record must not be refused at the socket entrance");

    let http = moved_off_a_task();
    apply_authorized_batch(
        &http.store,
        &http.plan_id,
        &http_body(&http.held, "SOCKET_OK", Some(WORKER)),
        12,
    )
    .expect("the holder's record must not be refused at the HTTP entrance");

    for fx in [&socket, &http] {
        let task = fx.task(&fx.held);
        assert_eq!(task.status, crate::plan::TaskStatus::Done);
        assert_eq!(task.body, "SOCKET_OK BODY");
        assert_eq!(task.section_ids, vec!["sec_SOCKET_OK".to_string()]);
        assert_eq!(task.milestones.len(), 1);
        assert_eq!(task.milestones[0].text, "SOCKET_OK MILESTONE");
    }
    assert_eq!(
        socket.stored_state(&socket.held),
        http.stored_state(&http.held),
        "both entrances must let the holder land the same write"
    );
}

#[test]
fn the_internal_report_path_still_writes_its_milestones() {
    let fx = moved_off_a_task();
    // The report path composes these against a task THIS caller does not hold —
    // its authority came from the tab/session/nonce check upstream, not from here.
    let ops = || {
        vec![
            BatchOp::SetTaskStatus {
                client_id: None,
                task: fx.foreign.clone(),
                status: "done".into(),
                status_label: None,
            },
            BatchOp::AddMilestone {
                client_id: Some("report".into()),
                task: fx.foreign.clone(),
                kind: "worker_report".into(),
                text: "REPORT MILESTONE".into(),
                from: Some(OrchestratorRef::from_tab(SUCCESSOR)),
                key: None,
                links: vec![],
            },
        ]
    };

    let result = apply_batch_with_authority(
        &fx.store,
        &fx.plan_id,
        BatchRequest {
            key: None,
            ops: ops(),
        },
        13,
        BatchAuthority::EstablishedUpstream {
            established_by: "test: authority established upstream",
        },
    )
    .unwrap_or_else(|_| panic!("the report path's declared authority must still write"));
    assert_eq!(result.milestone_ids.len(), 1);

    let task = fx.task(&fx.foreign);
    assert_eq!(task.status, crate::plan::TaskStatus::Done);
    assert_eq!(task.milestones.len(), 1);
    assert_eq!(task.milestones[0].text, "REPORT MILESTONE");

    // The same ops from an undeclared caller are refused, so the declaration is
    // what let them through — not a gap the guard happens to leave open.
    let refused = apply_batch_with_authority(
        &fx.store,
        &fx.plan_id,
        BatchRequest {
            key: None,
            ops: ops(),
        },
        14,
        BatchAuthority::Caller {
            tab_id: Some(WORKER),
        },
    );
    assert!(matches!(refused, Err(BatchWriteError::Refused(_))));
}

#[test]
fn the_refusal_leaves_a_trace_naming_the_attempting_tab() {
    let fx = moved_off_a_task();

    let socket = apply_batch_rpc(
        &fx.store,
        &fx.plan_id,
        &socket_params(&fx.foreign, "TRACE", Some(WORKER)),
        15,
    )
    .expect_err("refused");
    let socket_trace = socket.data.expect("the refusal must carry structured data");
    assert_eq!(socket_trace["type"], json!(super::STALE_TASK_BINDING_TYPE));
    assert_eq!(socket_trace["attempted_by_tab"], json!(WORKER));
    assert_eq!(socket_trace["plan_id"], json!(fx.plan_id));
    assert_eq!(socket_trace["task_id"], json!(fx.foreign));
    assert!(socket.message.contains(WORKER) && socket.message.contains(&fx.foreign));

    let (_, payload) = apply_authorized_batch(
        &fx.store,
        &fx.plan_id,
        &http_body(&fx.foreign, "TRACE", Some(WORKER)),
        16,
    )
    .expect_err("refused");
    assert_eq!(payload.0["plan_error"], socket_trace);

    // An unidentified caller is traced as such rather than passing anonymously.
    let anonymous = apply_batch_rpc(
        &fx.store,
        &fx.plan_id,
        &socket_params(&fx.foreign, "TRACE_ANON", None),
        17,
    )
    .expect_err("refused");
    assert_eq!(
        anonymous.data.expect("structured data")["attempted_by_tab"],
        Value::Null
    );
}

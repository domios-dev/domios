use super::*;
use crate::args::plan::PlanAction;
use crate::client::{RpcError, SocketCallError};

#[test]
fn utility_cleanup_forwards_own_binding_without_claiming_parent_session() {
    let caller: CallerContext = super::super::caller_context::test_utility_caller();
    let mut client = Recorder::default();
    clean_lanes_as(
        &mut client,
        true,
        Some("plan-token".into()),
        true,
        false,
        caller,
    )
    .unwrap();
    assert_eq!(
        client.calls,
        vec![(
            "plan.lanes_clean".into(),
            json!({
                "plan_id": "plan-token", "confirm": true, "fetch": false,
                "caller_tab_id": "utility", "session_binding_nonce": "utility-nonce"
            })
        )]
    );
}

#[derive(Default)]
struct Recorder {
    calls: Vec<(String, Value)>,
    refuse: bool,
}

impl RpcClient for Recorder {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, SocketCallError> {
        self.calls.push((method.into(), params.unwrap()));
        if self.refuse {
            return Err(SocketCallError::Rpc(RpcError {
                code: -32003,
                message: "refusal-token".into(),
                data: None,
            }));
        }
        Ok(json!({"dry_run": true, "lanes": []}))
    }
}

#[test]
fn lane_clean_cli_forwards_dry_run_confirm_and_fetch_once() {
    let _guard = crate::test_env_lock().lock().unwrap();
    for (confirm, fetch) in [(false, false), (true, false), (false, true)] {
        let mut client = Recorder::default();
        let result = crate::cmd::plan::run_plan(
            &mut client,
            true,
            PlanAction::Lanes {
                plan_id: Some("plan-token".into()),
                stale: false,
                clean: true,
                confirm,
                fetch,
            },
        )
        .unwrap()
        .unwrap();
        assert!(result["lanes"].as_array().unwrap().is_empty());
        assert_eq!(client.calls.len(), 1);
        assert_eq!(client.calls[0].0, "plan.lanes_clean");
        let params = &client.calls[0].1;
        assert_eq!(params["plan_id"], "plan-token");
        assert_eq!(params["confirm"], confirm);
        assert_eq!(params["fetch"], fetch);
        if let Ok(nonce) = std::env::var("CODETERM_SESSION_BINDING_NONCE") {
            assert_eq!(params["session_binding_nonce"], nonce);
        }
    }
}

#[test]
fn lane_clean_cli_propagates_refusal_without_retry() {
    let mut client = Recorder {
        refuse: true,
        ..Default::default()
    };
    assert!(clean_lanes(&mut client, true, Some("plan-token".into()), true, false).is_err());
    assert_eq!(client.calls.len(), 1);
}

#[test]
fn lane_clean_output_has_one_compact_line_per_supplied_lane_and_reason() {
    let rows = json!({"lanes": [
        {"task_id": "t1", "branch": "b1", "worktree": "w1", "action": "remove", "reason": "opaque-a"},
        {"task_id": "t2", "branch": "b2", "worktree": "w2", "action": "kept", "reason": "opaque-b\nopaque-c"}
    ]});
    let output = crate::cmd::plan_lanes::render_lane_cleanup(&rows);
    assert_eq!(output.lines().count(), 2);
    for (line, row) in output.lines().zip(rows["lanes"].as_array().unwrap()) {
        for key in ["task_id", "branch", "worktree", "action"] {
            assert!(line.contains(row[key].as_str().unwrap()));
        }
        let reason = row["reason"]
            .as_str()
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(line.ends_with(&reason));
    }
}

#[test]
fn lane_clean_output_includes_residue_without_task_or_branch() {
    let rows = json!({"lanes": [{"task_id": "", "branch": null, "worktree": "orphan-path", "action": "remove", "reason": "artifact residue"}]});
    assert_eq!(
        crate::cmd::plan_lanes::render_lane_cleanup(&rows),
        "remove - - orphan-path: artifact residue\n"
    );
}

#[test]
fn lane_clean_cli_consumes_progress_by_cursor_without_resubmitting_cleanup() {
    struct ProgressClient {
        calls: Vec<(String, Value)>,
    }
    impl RpcClient for ProgressClient {
        fn call_detailed(
            &mut self,
            method: &str,
            params: Option<Value>,
        ) -> Result<Value, SocketCallError> {
            self.calls.push((method.into(), params.unwrap()));
            Ok(match self.calls.len() {
                1 => json!({"job_id": "job-token"}),
                2 => json!({"events": [], "cursor": 3, "done": false}),
                3 => json!({"events": [], "cursor": 4, "done": true}),
                _ => panic!("unexpected request"),
            })
        }
    }
    let mut client = ProgressClient { calls: vec![] };
    clean_lanes_as(
        &mut client,
        false,
        Some("plan".into()),
        true,
        false,
        CallerContext::default(),
    )
    .unwrap();
    assert_eq!(client.calls.len(), 3);
    assert_eq!(client.calls[0].0, "plan.lanes_clean");
    assert_eq!(client.calls[0].1["stream"], true);
    assert_eq!(
        client.calls[1],
        (
            "plan.lanes_clean_next".into(),
            json!({"job_id":"job-token", "cursor":0})
        )
    );
    assert_eq!(
        client.calls[2],
        (
            "plan.lanes_clean_next".into(),
            json!({"job_id":"job-token", "cursor":3})
        )
    );
}

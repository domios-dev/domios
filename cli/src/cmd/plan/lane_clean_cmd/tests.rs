use super::*;
use crate::args::plan::PlanAction;
use crate::client::{RpcError, SocketCallError};

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

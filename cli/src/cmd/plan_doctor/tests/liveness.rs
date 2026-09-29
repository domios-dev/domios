use serde_json::{json, Value};

use crate::client::{RpcClient, RpcError, SocketCallError};

struct ReconcileRpc {
    plan: Value,
    reconcile: Result<Value, String>,
    calls: Vec<String>,
}

impl RpcClient for ReconcileRpc {
    fn call_detailed(
        &mut self,
        method: &str,
        _params: Option<Value>,
    ) -> Result<Value, SocketCallError> {
        self.calls.push(method.to_string());
        match method {
            "plan.get" => Ok(self.plan.clone()),
            "plan.files" => Ok(json!({
                "index_path": "C:/central/plans/active/pl_live/INDEX.md",
                "entries": []
            })),
            "plan.reconcile" => self.reconcile.clone().map_err(|message| {
                SocketCallError::Rpc(RpcError {
                    code: -32603,
                    message,
                    data: None,
                })
            }),
            other => panic!("unexpected RPC method {other}"),
        }
    }
}

fn plan() -> Value {
    json!({
        "id": "pl_live",
        "title": "Liveness",
        "status": "active",
        "sections": [],
        "tasks": [{"id": "task_gone", "name": "Gone", "status": "in_progress"}]
    })
}

fn report(client: &mut ReconcileRpc, kind: &str) -> Value {
    let dir = tempfile::tempdir().unwrap();
    super::super::doctor_report_authoritative(
        client,
        Some("pl_live".into()),
        Some(dir.path().display().to_string()),
        kind,
    )
    .unwrap()
}

#[test]
fn doctor_reports_in_progress_tab_gone() {
    let mut client = ReconcileRpc {
        plan: plan(),
        reconcile: Ok(json!({
            "drift": [
                {"task_id": "task_gone", "plan_id": "pl_live", "kind": "in_progress_tab_gone",
                 "detail": "detail-token", "suggested_fix": "needs-reassign"},
                {"task_id": "task_other", "plan_id": "pl_live", "kind": "stale_in_review",
                 "detail": "other", "suggested_fix": "stale-review"}
            ]
        })),
        calls: Vec::new(),
    };

    let report = report(&mut client, "all");

    assert!(client.calls.iter().any(|call| call == "plan.reconcile"));
    let lost: Vec<&Value> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["kind"] == "in_progress_tab_gone")
        .collect();
    assert_eq!(lost.len(), 1);
    assert_eq!(lost[0]["severity"], "error");
    assert_eq!(lost[0]["task_id"], "task_gone");
    assert_eq!(lost[0]["detail"], "detail-token");
    assert!(lost[0]["fix"].as_str().unwrap().contains("task_gone"));
    assert_eq!(report["ok"], false);
}

#[test]
fn quality_doctor_does_not_probe_liveness() {
    let mut client = ReconcileRpc {
        plan: plan(),
        reconcile: Err("must not be called".into()),
        calls: Vec::new(),
    };

    report(&mut client, "quality");

    assert!(!client.calls.iter().any(|call| call == "plan.reconcile"));
}

#[test]
fn unreadable_liveness_is_a_warning_not_silence() {
    let mut client = ReconcileRpc {
        plan: plan(),
        reconcile: Err("daemon unavailable".into()),
        calls: Vec::new(),
    };

    let report = report(&mut client, "structure");

    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["kind"] == "liveness_unchecked"
                && finding["severity"] == "warning")
    );
}

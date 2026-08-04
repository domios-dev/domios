use crate::client::{RpcClient, RpcError, SocketCallError};

use super::*;

enum PlanResponse {
    Value(Value),
    NotFound,
}

struct FakeRpc {
    response: PlanResponse,
    calls: Vec<(String, Option<Value>)>,
}

impl FakeRpc {
    fn with_plan(plan: Value) -> Self {
        Self {
            response: PlanResponse::Value(plan),
            calls: Vec::new(),
        }
    }

    fn not_found() -> Self {
        Self {
            response: PlanResponse::NotFound,
            calls: Vec::new(),
        }
    }
}

impl RpcClient for FakeRpc {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, SocketCallError> {
        self.calls.push((method.to_string(), params));
        match method {
            "plan.get" => match &self.response {
                PlanResponse::Value(value) => Ok(value.clone()),
                PlanResponse::NotFound => Err(SocketCallError::Rpc(RpcError {
                    code: -32004,
                    message: "plan not found".into(),
                    data: Some(json!({ "type": "not_found" })),
                })),
            },
            "plan.files" => Ok(json!({
                "index_path": "C:/central/plans/active/pl_real/INDEX.md",
                "entries": []
            })),
            other => panic!("unexpected RPC method {other}"),
        }
    }
}

fn project_arg() -> (tempfile::TempDir, Option<String>) {
    let project = tempfile::tempdir().unwrap();
    let arg = Some(project.path().display().to_string());
    (project, arg)
}

#[test]
fn production_layout_real_plan_is_inspected() {
    let plan = json!({
        "id": "pl_real",
        "title": "Authoritative",
        "status": "active",
        "mode": "planning",
        "tasks": []
    });
    let mut client = FakeRpc::with_plan(plan);
    let (_project, project) = project_arg();

    let report =
        doctor_report_authoritative(&mut client, Some("pl_real".into()), project, "quality")
            .unwrap();

    assert_eq!(report["checked_plans"], 1);
    assert_eq!(client.calls[0].0, "plan.get");
}

#[test]
fn production_layout_unknown_plan_queries_authority() {
    let mut client = FakeRpc::not_found();
    let (_project, project) = project_arg();

    let report =
        doctor_report_authoritative(&mut client, Some("pl_missing".into()), project, "quality")
            .unwrap();

    assert_eq!(report["checked_plans"], 0);
    assert_eq!(report["ok"], false);
    assert_eq!(report["error_count"], 1);
    assert_eq!(report["findings"][0]["kind"], "plan_not_inspected");
    assert_eq!(client.calls[0].0, "plan.get");
}

#[test]
fn production_layout_malformed_plan_is_reported() {
    let mut client = FakeRpc::with_plan(Value::String("malformed".into()));
    let (_project, project) = project_arg();

    let report =
        doctor_report_authoritative(&mut client, Some("pl_real".into()), project, "quality")
            .unwrap();

    assert_eq!(report["checked_plans"], 1);
    assert_eq!(report["error_count"], 1);
    assert_eq!(report["findings"][0]["kind"], "malformed_plan");
}

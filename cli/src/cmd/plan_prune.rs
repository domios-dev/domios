use serde_json::{json, Value};

use crate::client::RpcClient;
use crate::cmd::plan_local::project_root;

#[derive(Debug, Clone)]
struct Candidate {
    plan_id: String,
    path: String,
    title: String,
    reason: &'static str,
}

impl Candidate {
    fn json(&self) -> Value {
        json!({
            "plan_id": self.plan_id,
            "path": self.path,
            "title": self.title,
            "reason": self.reason,
        })
    }
}

pub fn run(
    client: &mut dyn RpcClient,
    json_mode: bool,
    apply: bool,
    project: Option<String>,
) -> Result<Option<Value>, String> {
    let report = prune_report(client, project, apply)?;
    if json_mode {
        return Ok(Some(report));
    }
    print!("{}", render_prune(&report));
    Ok(None)
}

fn prune_report(
    client: &mut dyn RpcClient,
    project: Option<String>,
    apply: bool,
) -> Result<Value, String> {
    let project_root = project_root(project)?;
    let listed = client.call(
        "plan.list",
        Some(json!({ "project": project_root.display().to_string() })),
    )?;
    let mut candidates = Vec::new();
    for entry in listed
        .get("plans")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(plan_id) = entry.get("plan_id").and_then(Value::as_str) else {
            continue;
        };
        if plan_id == "inbox" {
            continue;
        }
        let plan = match client.call("plan.get", Some(json!({ "plan_id": plan_id }))) {
            Ok(plan) => plan,
            Err(_) => continue,
        };
        if let Some(reason) = prune_reason(&plan) {
            candidates.push(Candidate {
                plan_id: plan_id.to_string(),
                path: format!("authoritative:{plan_id}"),
                title: plan
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .into(),
                reason,
            });
        }
    }
    let mut deleted = Vec::new();
    if apply {
        for candidate in &candidates {
            client.call("plan.delete", Some(json!({ "plan_id": candidate.plan_id })))?;
            deleted.push(candidate.json());
        }
    }
    Ok(json!({
        "dry_run": !apply,
        "project": project_root.display().to_string(),
        "candidate_count": candidates.len(),
        "candidates": candidates.iter().map(Candidate::json).collect::<Vec<_>>(),
        "deleted": deleted,
    }))
}

fn prune_reason(plan: &Value) -> Option<&'static str> {
    if !is_effectively_empty(plan) {
        return None;
    }
    let title = plan.get("title").and_then(|v| v.as_str()).unwrap_or("");
    if is_smoke_title(title) {
        return Some("smoke_empty");
    }
    if title.trim().is_empty() {
        return Some("empty_title");
    }
    None
}

fn is_effectively_empty(plan: &Value) -> bool {
    is_empty_array(plan.get("tasks"))
        && is_empty_array(plan.get("sections"))
        && is_empty_array(plan.get("elements"))
        && is_empty_array(plan.get("loops"))
        && is_empty_array(plan.get("spec_refs"))
        && is_empty_array(plan.get("spec_links"))
        && plan.get("goal").is_none_or(|v| v.is_null())
}

fn is_empty_array(value: Option<&Value>) -> bool {
    value.is_none_or(|v| v.as_array().is_none_or(|items| items.is_empty()))
}

fn is_smoke_title(title: &str) -> bool {
    let trimmed = title.trim();
    let lower = trimmed.to_ascii_lowercase();
    lower.contains("smoke")
        || trimmed == "T"
        || lower == "test"
        || lower.starts_with("test ")
        || lower.starts_with("tmp ")
        || lower.starts_with("temp ")
}

pub fn render_prune(report: &Value) -> String {
    let dry_run = report
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let count = report
        .get("candidate_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let mode = if dry_run { "dry-run" } else { "apply" };
    let mut out = format!("Plan prune ({mode}): {count} candidate(s)\n");
    let candidates = report
        .get("candidates")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for candidate in candidates {
        let plan_id = candidate
            .get("plan_id")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let reason = candidate
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let title = candidate
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        out.push_str(&format!("  - {plan_id} {reason}: \"{title}\"\n"));
    }
    if dry_run && count > 0 {
        out.push_str("Run with --apply to delete these plan files.\n");
    }
    let deleted = report
        .get("deleted")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if !deleted.is_empty() {
        out.push_str(&format!("Deleted: {}\n", deleted.len()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::SocketCallError;

    struct FakeRpc {
        calls: Vec<String>,
    }

    impl RpcClient for FakeRpc {
        fn call_detailed(
            &mut self,
            method: &str,
            _params: Option<Value>,
        ) -> Result<Value, SocketCallError> {
            self.calls.push(method.into());
            Ok(match method {
                "plan.list" => json!({ "plans": [{ "plan_id": "pl_tmp" }] }),
                "plan.get" => json!({
                    "id": "pl_tmp",
                    "title": "smoke test",
                    "tasks": [],
                    "sections": []
                }),
                "plan.delete" => json!({ "ok": true }),
                other => panic!("unexpected RPC method {other}"),
            })
        }
    }

    #[test]
    fn smoke_empty_plan_is_candidate() {
        let plan = json!({
            "id": "pl_smoke",
            "title": "geometry smoke",
            "status": "active",
            "tasks": [],
            "sections": []
        });
        assert_eq!(prune_reason(&plan), Some("smoke_empty"));
    }

    #[test]
    fn populated_plan_is_not_candidate() {
        let plan = json!({
            "id": "pl_1",
            "title": "smoke",
            "status": "active",
            "tasks": [{"id": "task_1"}],
            "sections": []
        });
        assert_eq!(prune_reason(&plan), None);
    }

    #[test]
    fn dry_run_reads_authoritative_store_without_deleting() {
        let dir = tempfile::tempdir().unwrap();
        let mut client = FakeRpc { calls: Vec::new() };
        let report =
            prune_report(&mut client, Some(dir.path().display().to_string()), false).unwrap();
        assert_eq!(report["candidate_count"], 1);
        assert!(report["dry_run"].as_bool().unwrap());
        assert_eq!(client.calls, ["plan.list", "plan.get"]);
    }

    #[test]
    fn apply_deletes_through_authoritative_rpc() {
        let dir = tempfile::tempdir().unwrap();
        let mut client = FakeRpc { calls: Vec::new() };
        let report =
            prune_report(&mut client, Some(dir.path().display().to_string()), true).unwrap();
        assert_eq!(report["deleted"].as_array().unwrap().len(), 1);
        assert_eq!(client.calls, ["plan.list", "plan.get", "plan.delete"]);
    }
}

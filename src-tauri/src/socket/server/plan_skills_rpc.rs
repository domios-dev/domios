use serde_json::{json, Value};
use sha2::Digest;

use super::plan::plan_rpc_err;
use super::{get_string_param, plan_refs, JsonRpcError};
use crate::service::plan::plan_skills::{
    create_plan_skill, list_plan_skills, show_plan_skill, NewPlanSkill,
};
use crate::service::plan::PlanStore;
use crate::service::ServiceState;

pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "plan.skills.list" | "plan.skills.show" | "plan.skills.new"
    )
}

pub(super) fn dispatch(
    method: &str,
    params: &Option<Value>,
    svc: &ServiceState,
    now_ms: u64,
) -> Result<Value, JsonRpcError> {
    let store = &svc.plan_store;
    let result = dispatch_store(method, params, store)?;
    if method != "plan.skills.show" {
        return Ok(result);
    }
    let plan_id = plan_refs::resolve_plan(store, params)?;
    let Some(body) = params.as_ref() else {
        return Ok(result);
    };
    let binding = [
        body.get("caller_tab_id").and_then(Value::as_str),
        body.get("caller_session_id").and_then(Value::as_str),
        body.get("session_binding_nonce").and_then(Value::as_str),
        body.get("task_id").and_then(Value::as_str),
    ];
    if binding.iter().any(|value| value.is_some()) && binding.iter().any(|value| value.is_none()) {
        return Err(plan_rpc_err(crate::service::plan::PlanError::Io(
            "plan skill caller identity is incomplete".into(),
        )));
    }
    let [Some(tab_id), Some(session_id), Some(nonce), Some(task_id)] = binding else {
        return Ok(result);
    };
    let live = svc.mux.read().get_tab(tab_id).map(|tab| {
        tab.agent_session_id.as_deref() == Some(session_id)
            && tab.session_binding_nonce.as_deref() == Some(nonce)
            && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
    }) == Some(true);
    if !live {
        return Err(plan_rpc_err(crate::service::plan::PlanError::Io(
            "plan skill caller binding is not current".into(),
        )));
    }
    let skill = result.get("skill").ok_or_else(|| JsonRpcError {
        code: -32603,
        message: "plan skill response is missing skill".into(),
        data: None,
    })?;
    let slug = skill
        .get("slug")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let body = skill
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let digest = format!("sha256:{:x}", sha2::Sha256::digest(body.as_bytes()));
    let attempt_id = params
        .as_ref()
        .and_then(|params| params.get("attempt_id"))
        .and_then(Value::as_str);
    store
        .record_plan_skill_load(
            &plan_id, task_id, tab_id, session_id, nonce, attempt_id, slug, &digest, now_ms,
        )
        .map_err(plan_rpc_err)?;
    Ok(result)
}

fn dispatch_store(
    method: &str,
    params: &Option<Value>,
    store: &PlanStore,
) -> Result<Value, JsonRpcError> {
    let plan_id = plan_refs::resolve_plan(store, params)?;
    match method {
        "plan.skills.list" => {
            let skills = list_plan_skills(store, &plan_id).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": plan_id, "skills": skills }))
        }
        "plan.skills.show" => {
            let slug = get_string_param(params, "slug")?;
            let skill = show_plan_skill(store, &plan_id, &slug).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": plan_id, "skill": skill }))
        }
        "plan.skills.new" => {
            let body = params.as_ref().ok_or(JsonRpcError {
                code: -32602,
                message: "params required".to_string(),
                data: None,
            })?;
            let request = NewPlanSkill {
                slug: get_string_param(params, "slug")?,
                title: optional_string(body, "title"),
                area: optional_string(body, "area"),
                check: optional_string(body, "check"),
                rationale: optional_string(body, "rationale"),
            };
            let skill = create_plan_skill(store, &plan_id, request).map_err(plan_rpc_err)?;
            Ok(json!({ "plan_id": plan_id, "skill": skill }))
        }
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

fn optional_string(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(Value::as_str).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::OrchestratorRef;
    use crate::service::plan::paths::{plan_dir, plan_sidecar_root};
    use crate::service::plan::PlanStore;

    fn rpc(store: &PlanStore, method: &str, params: Value) -> Result<Value, JsonRpcError> {
        dispatch_store(method, &Some(params), store)
    }

    #[test]
    fn plan_skills_rpc_uses_daemon_plan_project_outside_caller_cwd() {
        let daemon_root = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let caller = tempfile::tempdir().unwrap();
        crate::service::plan::paths::mark_test_project_root(project.path());
        let store = PlanStore::new_for_test(daemon_root.path());
        let plan = store
            .create_plan_for_team(
                "Skills".into(),
                project.path().display().to_string(),
                OrchestratorRef::from_tab("orch"),
                "team-skills".into(),
                vec![],
                1,
            )
            .unwrap();

        let created = rpc(
            &store,
            "plan.skills.new",
            json!({
                "plan_id": plan.id,
                "slug": "Deploy Safely",
                "title": "Safe Deploy",
                "area": "release",
                "check": "./scripts/check.sh",
                "rationale": "accepted fixture"
            }),
        )
        .unwrap();
        assert_eq!(created["plan_id"], plan.id);
        assert_eq!(created["skill"]["slug"], "deploy-safely");
        assert_eq!(created["skill"]["title"], "Safe Deploy");
        assert_eq!(created["skill"]["check"], "./scripts/check.sh");
        assert_eq!(created["skill"]["verdict"], "keep");
        assert_eq!(created["skill"]["ledger_seq"], 1);

        let listed = rpc(&store, "plan.skills.list", json!({ "plan_id": plan.id })).unwrap();
        assert_eq!(listed["plan_id"], plan.id);
        assert_eq!(listed["skills"][0]["slug"], "deploy-safely");
        assert_eq!(listed["skills"][0]["title"], "Safe Deploy");
        assert_eq!(listed["skills"][0]["check"], "./scripts/check.sh");
        assert_eq!(listed["skills"][0]["verdict"], "keep");

        let shown = rpc(
            &store,
            "plan.skills.show",
            json!({ "plan_id": plan.id, "slug": "Deploy Safely" }),
        )
        .unwrap();
        assert_eq!(shown["plan_id"], plan.id);
        assert_eq!(shown["skill"]["title"], "Safe Deploy");
        assert!(shown["skill"]["body"]
            .as_str()
            .unwrap()
            .contains("check: ./scripts/check.sh"));
        assert_eq!(shown["skill"]["ledger"][0]["seq"], 1);
        assert_eq!(shown["skill"]["ledger"][0]["verdict"], "keep");
        assert_eq!(shown["skill"]["ledger"][0]["rationale"], "accepted fixture");

        let plan_dir = plan_dir(plan_sidecar_root(&plan), &plan.status, &plan.id).unwrap();
        assert_eq!(
            std::fs::read_dir(plan_dir.join("skills"))
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_dir())
                .count(),
            1
        );
        assert_eq!(std::fs::read_dir(caller.path()).unwrap().count(), 0);
    }

    #[test]
    fn plan_skills_rpc_failures_are_structured_and_atomic() {
        let daemon_root = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        crate::service::plan::paths::mark_test_project_root(project.path());
        let store = PlanStore::new_for_test(daemon_root.path());
        let plan = store
            .create_plan_for_team(
                "Skills".into(),
                project.path().display().to_string(),
                OrchestratorRef::from_tab("orch"),
                "team-skills-failures".into(),
                vec![],
                1,
            )
            .unwrap();

        let invalid = rpc(
            &store,
            "plan.skills.new",
            json!({ "plan_id": plan.id, "slug": "!!!" }),
        )
        .unwrap_err();
        assert_eq!(
            invalid.data.unwrap()["type"],
            serde_json::json!("invalid_plan_skill_slug")
        );

        rpc(
            &store,
            "plan.skills.new",
            json!({ "plan_id": plan.id, "slug": "deploy" }),
        )
        .unwrap();
        let duplicate = rpc(
            &store,
            "plan.skills.new",
            json!({ "plan_id": plan.id, "slug": "deploy" }),
        )
        .unwrap_err();
        assert_eq!(
            duplicate.data.unwrap()["type"],
            serde_json::json!("plan_skill_already_exists")
        );
        let shown = rpc(
            &store,
            "plan.skills.show",
            json!({ "plan_id": plan.id, "slug": "deploy" }),
        )
        .unwrap();
        assert_eq!(shown["skill"]["ledger"].as_array().unwrap().len(), 1);

        let unknown = rpc(
            &store,
            "plan.skills.list",
            json!({ "plan_id": "plan_missing" }),
        )
        .unwrap_err();
        assert!(unknown.data.is_some());

        let failing_plan = store
            .create_plan_for_team(
                "Write failure".into(),
                project.path().display().to_string(),
                OrchestratorRef::from_tab("orch-failure"),
                "team-skills-write-failure".into(),
                vec![],
                2,
            )
            .unwrap();
        let failing_dir = plan_dir(
            plan_sidecar_root(&failing_plan),
            &failing_plan.status,
            &failing_plan.id,
        )
        .unwrap();
        std::fs::write(failing_dir.join("skills"), "blocked").unwrap();
        let write_error = rpc(
            &store,
            "plan.skills.new",
            json!({ "plan_id": failing_plan.id, "slug": "blocked" }),
        )
        .unwrap_err();
        assert_eq!(write_error.data.unwrap()["type"], serde_json::json!("io"));
        assert!(failing_dir.join("skills").is_file());
        assert!(!failing_dir.join("skills").join("blocked").exists());
        assert!(!failing_dir.join("skills").join("ledger.jsonl").exists());
    }
}

use serde_json::{json, Value};
use sha2::Digest;

use super::plan::plan_rpc_err;
use super::{get_string_param, plan_refs, JsonRpcError};
use crate::plan::PlannerRef;
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
    let supplied_session_id = body.get("caller_session_id").and_then(Value::as_str);
    let identity = [
        body.get("caller_tab_id").and_then(Value::as_str),
        body.get("session_binding_nonce").and_then(Value::as_str),
    ];
    let [Some(tab_id), Some(nonce)] = identity else {
        if identity.iter().any(Option::is_some)
            || supplied_session_id.is_some()
            || body.get("task_id").is_some()
            || body.get("attempt_id").is_some()
        {
            return Err(plan_rpc_err(crate::service::plan::PlanError::Io(
                "plan skill caller identity is incomplete".into(),
            )));
        }
        return Ok(result);
    };
    let session_id = match supplied_session_id {
        Some(session_id) => session_id.to_string(),
        None => nonce_proven_session_id(svc, tab_id, nonce)?,
    };
    let live = svc.mux.read().get_tab(tab_id).map(|tab| {
        tab.agent_session_id.as_deref() == Some(session_id.as_str())
            && tab.session_binding_nonce.as_deref() == Some(nonce)
            && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
    }) == Some(true);
    if !live {
        return Err(plan_rpc_err(crate::service::plan::PlanError::Io(
            "plan skill caller binding is not current".into(),
        )));
    }
    // Owner/Planner reads their own plan without a task binding or ledger entry.
    if plan_caller_is_owner_or_planner(store, &plan_id, tab_id) {
        return Ok(result);
    }
    let task_id = resolve_skill_task_id(
        store,
        &plan_id,
        tab_id,
        body.get("task_id").and_then(Value::as_str),
    )
    .map_err(plan_rpc_err)?;
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
            &plan_id,
            &task_id,
            tab_id,
            &session_id,
            nonce,
            attempt_id,
            slug,
            &digest,
            now_ms,
        )
        .map_err(plan_rpc_err)?;
    Ok(result)
}

/// Checked only after `live` proves the caller genuinely owns `tab_id`.
fn plan_caller_is_owner_or_planner(store: &PlanStore, plan_id: &str, tab_id: &str) -> bool {
    let Some(plan) = store.get(plan_id) else {
        return false;
    };
    if plan.owner.tab_id.as_deref() == Some(tab_id) {
        return true;
    }
    matches!(
        &plan.planner,
        Some(PlannerRef::Tab { tab_id: bound, .. }) if bound == tab_id
    )
}

// A task tab cannot export its session id before PTY spawn; only a nonce that
// is both issued to the tab and echoed back proves the session that owns it.
fn nonce_proven_session_id(
    svc: &ServiceState,
    tab_id: &str,
    nonce: &str,
) -> Result<String, JsonRpcError> {
    svc.mux
        .read()
        .get_tab(tab_id)
        .filter(|tab| {
            tab.session_binding_nonce.as_deref() == Some(nonce)
                && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
        })
        .and_then(|tab| tab.agent_session_id.clone())
        .ok_or_else(|| {
            plan_rpc_err(crate::service::plan::PlanError::Io(
                "plan skill caller session is not nonce-proven by a live tab".into(),
            ))
        })
}

fn resolve_skill_task_id(
    store: &PlanStore,
    plan_id: &str,
    tab_id: &str,
    supplied_task_id: Option<&str>,
) -> Result<String, crate::service::plan::PlanError> {
    supplied_task_id
        .map(str::to_string)
        .or_else(|| {
            store
                .task_binding_for_tab(tab_id)
                .filter(|(bound_plan_id, _)| bound_plan_id == plan_id)
                .map(|(_, task_id)| task_id)
        })
        .ok_or_else(|| {
            crate::service::plan::PlanError::Io(
                "plan skill caller has no current task binding".into(),
            )
        })
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
    use crate::multiplexer::state::MultiplexerState;
    use crate::plan::{OrchestratorRef, PlanSkillManifestEntry};
    use crate::service::plan::paths::{plan_dir, plan_sidecar_root};
    use crate::service::plan::PlanStore;
    use crate::service::test_fixture::ServiceFixture;
    use crate::service::ServiceState;
    use std::sync::Arc;

    fn add_task(store: &PlanStore, plan_id: &str, name: &str) -> String {
        store
            .add_task(
                plan_id,
                crate::service::plan::NewTask {
                    name: name.into(),
                    kind: "task".into(),
                    ..Default::default()
                },
                3,
            )
            .unwrap()
    }

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

    #[test]
    fn skill_evidence_resolves_a_task_assigned_after_process_launch() {
        let daemon_root = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
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
        store
            .set_mode(&plan.id, crate::plan::PlanMode::Execution, 2)
            .unwrap();
        let task_id = add_task(&store, &plan.id, "post-launch task");
        store
            .set_task_assignment(&task_id, Some("running-tab"), Some("worker"), 4)
            .unwrap();

        assert_eq!(
            resolve_skill_task_id(&store, &plan.id, "running-tab", None).unwrap(),
            task_id
        );
        assert!(resolve_skill_task_id(&store, &plan.id, "foreign-tab", None).is_err());
    }

    const CALLER_NONCE: &str = "nonce-live-5upl98";
    const CALLER_SESSION: &str = "session-live-5upl98";
    const CALLER_SKILL: &str = "verify-before-claim";

    /// A task-bound Worker as production leaves it: the tab holds the issued and
    /// echoed nonce, and the session id exists only there, never in the env.
    struct LiveCaller {
        svc: ServiceState,
        plan_id: String,
        task_id: String,
        attempt_id: String,
        _fixture: ServiceFixture,
        _project: tempfile::TempDir,
    }

    impl LiveCaller {
        fn new() -> Self {
            let project = tempfile::tempdir().unwrap();
            crate::service::plan::paths::mark_test_project_root(project.path());
            let (mut mux, _) = MultiplexerState::new();
            let workspace = mux.list_workspaces()[0].id.clone();
            let tab_id = mux.create_tab(&workspace).unwrap();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.agent_type = Some("claude".into());
            tab.agent_session_id = Some(CALLER_SESSION.into());
            tab.session_binding_nonce = Some(CALLER_NONCE.into());
            tab.reported_session_binding_nonce = Some(CALLER_NONCE.into());

            let fixture = ServiceFixture::new(mux);
            let store = fixture.service.plan_store.clone();
            let plan = store
                .create_plan_for_team(
                    "Skills".into(),
                    project.path().display().to_string(),
                    OrchestratorRef::from_tab("orch"),
                    "team-live-identity".into(),
                    vec![],
                    1,
                )
                .unwrap();
            store
                .set_mode(&plan.id, crate::plan::PlanMode::Execution, 2)
                .unwrap();
            create_plan_skill(
                &store,
                &plan.id,
                NewPlanSkill {
                    slug: CALLER_SKILL.into(),
                    title: Some("Startup transition claim audit".into()),
                    area: Some("startup-team-recovery".into()),
                    check: None,
                    rationale: Some("mandatory".into()),
                },
            )
            .unwrap();
            let task_id = add_task(&store, &plan.id, "nonce-proven identity");
            store
                .assign_task(&plan.id, &task_id, &tab_id, None, 4)
                .unwrap();
            store
                .update_attempt_executor_identity(
                    &plan.id,
                    &task_id,
                    &tab_id,
                    Some("Worker"),
                    Some(CALLER_SESSION),
                    Some(CALLER_NONCE),
                    5,
                )
                .unwrap();
            let attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
                .id
                .clone();
            let shown = rpc(
                &store,
                "plan.skills.show",
                json!({ "plan_id": plan.id, "slug": CALLER_SKILL }),
            )
            .unwrap();
            let body = shown["skill"]["body"].as_str().unwrap().to_string();
            store
                .set_attempt_plan_skill_manifest(
                    &plan.id,
                    &task_id,
                    &attempt_id,
                    vec![PlanSkillManifestEntry {
                        slug: CALLER_SKILL.into(),
                        title: "Startup transition claim audit".into(),
                        purpose: "Startup transition claim audit".into(),
                        content_digest: format!(
                            "sha256:{:x}",
                            sha2::Sha256::digest(body.as_bytes())
                        ),
                    }],
                    6,
                )
                .unwrap();
            let svc: ServiceState = Arc::new(fixture.service.clone());
            Self {
                svc,
                plan_id: plan.id,
                task_id,
                attempt_id,
                _fixture: fixture,
                _project: project,
            }
        }

        fn tab_id(&self) -> String {
            self.svc
                .plan_store
                .show_task(&self.task_id)
                .unwrap()
                .task
                .assigned_tab_id
                .unwrap()
        }

        /// The exact envelope production can export: tab, nonce, task, attempt.
        fn production_params(&self) -> Value {
            json!({
                "plan_id": self.plan_id,
                "slug": CALLER_SKILL,
                "caller_tab_id": self.tab_id(),
                "session_binding_nonce": CALLER_NONCE,
                "task_id": self.task_id,
                "attempt_id": self.attempt_id,
            })
        }

        fn show(&self, params: Value) -> Result<Value, JsonRpcError> {
            dispatch("plan.skills.show", &Some(params), &self.svc, 7)
        }

        fn loaded_skills(&self) -> Vec<String> {
            self.svc
                .plan_store
                .show_task(&self.task_id)
                .unwrap()
                .task
                .execution_attempts[0]
                .loaded_plan_skills
                .iter()
                .map(|evidence| evidence.slug.clone())
                .collect()
        }
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn plan_skill_show_backfills_nonce_proven_live_session() {
        let caller = LiveCaller::new();
        let mut params = caller.production_params();
        assert!(params.get("caller_session_id").is_none());

        let shown = caller.show(params.clone()).unwrap();

        assert_eq!(shown["skill"]["slug"], CALLER_SKILL);
        assert_eq!(caller.loaded_skills(), vec![CALLER_SKILL.to_string()]);

        // A caller that can supply the tab's own session id stays acceptable.
        params["caller_session_id"] = json!(CALLER_SESSION);
        caller.show(params).unwrap();
        assert_eq!(caller.loaded_skills(), vec![CALLER_SKILL.to_string()]);
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn plan_skill_show_refuses_stale_or_mismatched_nonce() {
        let caller = LiveCaller::new();
        let mut mismatched = caller.production_params();
        mismatched["session_binding_nonce"] = json!("nonce-foreign");
        assert!(caller.show(mismatched).is_err());

        let mut unrelated = caller.production_params();
        unrelated["caller_tab_id"] = json!("some-other-tab");
        assert!(caller.show(unrelated).is_err());

        let mut foreign_session = caller.production_params();
        foreign_session["caller_session_id"] = json!("session-foreign");
        assert!(caller.show(foreign_session).is_err());
        assert!(caller.loaded_skills().is_empty());

        // A session transition clears the echoed nonce; the configured nonce
        // alone must never prove that the current agent owns the tab.
        let tab_id = caller.tab_id();
        {
            let mut mux = caller.svc.mux.write();
            mux.get_tab_mut(&tab_id)
                .unwrap()
                .reported_session_binding_nonce = None;
        }
        assert!(caller.show(caller.production_params()).is_err());

        // A nonce-proven tab with no session yet is unbound, not backfillable.
        {
            let mut mux = caller.svc.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.reported_session_binding_nonce = Some(CALLER_NONCE.into());
            tab.agent_session_id = None;
        }
        assert!(caller.show(caller.production_params()).is_err());
        assert!(caller.loaded_skills().is_empty());
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn plan_skill_show_refuses_partial_identity_without_live_backfill() {
        let caller = LiveCaller::new();
        for partial in [
            json!({ "caller_tab_id": caller.tab_id() }),
            json!({ "session_binding_nonce": CALLER_NONCE }),
            json!({ "caller_session_id": CALLER_SESSION }),
            json!({ "task_id": caller.task_id }),
            json!({ "attempt_id": caller.attempt_id }),
        ] {
            let mut params = json!({ "plan_id": caller.plan_id, "slug": CALLER_SKILL });
            for (key, value) in partial.as_object().unwrap() {
                params[key] = value.clone();
            }
            let error = caller.show(params).unwrap_err();
            assert!(
                error.message.contains("incomplete"),
                "{partial} must be refused as incomplete: {error:?}"
            );
        }

        // No identity at all stays a plain read: the skill is returned and no
        // attempt evidence is invented for a caller that claimed nothing.
        let shown = caller
            .show(json!({ "plan_id": caller.plan_id, "slug": CALLER_SKILL }))
            .unwrap();
        assert_eq!(shown["skill"]["slug"], CALLER_SKILL);
        assert!(caller.loaded_skills().is_empty());
    }

    const OWNER_SKILL: &str = "verify-before-claim";
    const OWNER_NONCE: &str = "nonce-owner-5upl98";
    const OWNER_SESSION: &str = "session-owner-5upl98";

    /// A plan with a live, nonce-proven owner tab, a live Planner tab, a
    /// live tab bound to neither, and one skill to read.
    struct OwnerFixture {
        svc: ServiceState,
        plan_id: String,
        owner_tab: String,
        planner_tab: String,
        stranger_tab: String,
        _fixture: ServiceFixture,
        _project: tempfile::TempDir,
    }

    impl OwnerFixture {
        fn new() -> Self {
            let project = tempfile::tempdir().unwrap();
            crate::service::plan::paths::mark_test_project_root(project.path());
            let (mut mux, _) = MultiplexerState::new();
            let workspace = mux.list_workspaces()[0].id.clone();
            let owner_tab = mux.create_tab(&workspace).unwrap();
            let planner_tab = mux.create_tab(&workspace).unwrap();
            let stranger_tab = mux.create_tab(&workspace).unwrap();
            for (tab, session, nonce) in [
                (&owner_tab, OWNER_SESSION, OWNER_NONCE),
                (&planner_tab, "session-planner", "nonce-planner"),
                (&stranger_tab, "session-stranger", "nonce-stranger"),
            ] {
                let tab = mux.get_tab_mut(tab).unwrap();
                tab.agent_type = Some("claude".into());
                tab.agent_session_id = Some(session.into());
                tab.session_binding_nonce = Some(nonce.into());
                tab.reported_session_binding_nonce = Some(nonce.into());
            }

            let fixture = ServiceFixture::new(mux);
            let store = fixture.service.plan_store.clone();
            let plan = store
                .create_plan_for_team(
                    "Skills".into(),
                    project.path().display().to_string(),
                    OrchestratorRef::from_tab(&owner_tab),
                    "team-owner-read".into(),
                    vec![],
                    1,
                )
                .unwrap();
            store
                .set_planner_ref(
                    &plan.id,
                    Some(PlannerRef::Tab {
                        tab_id: planner_tab.clone(),
                        session_id: None,
                        title: None,
                    }),
                    2,
                )
                .unwrap();
            create_plan_skill(
                &store,
                &plan.id,
                NewPlanSkill {
                    slug: OWNER_SKILL.into(),
                    title: Some("Verify Before Claim".into()),
                    area: None,
                    check: None,
                    rationale: Some("mandatory".into()),
                },
            )
            .unwrap();
            let svc: ServiceState = Arc::new(fixture.service.clone());
            Self {
                svc,
                plan_id: plan.id,
                owner_tab,
                planner_tab,
                stranger_tab,
                _fixture: fixture,
                _project: project,
            }
        }

        fn show(&self, tab_id: &str, nonce: &str) -> Result<Value, JsonRpcError> {
            dispatch(
                "plan.skills.show",
                &Some(json!({
                    "plan_id": self.plan_id,
                    "slug": OWNER_SKILL,
                    "caller_tab_id": tab_id,
                    "session_binding_nonce": nonce,
                })),
                &self.svc,
                7,
            )
        }
    }

    /// Neither tab is ever assigned a task, so success here proves the
    /// owner/Planner path never reaches `resolve_skill_task_id`.
    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn plan_skill_show_serves_the_plan_owner_and_planner() {
        let f = OwnerFixture::new();

        let shown = f
            .show(&f.owner_tab, OWNER_NONCE)
            .expect("the plan owner must read its own plan's skill");
        assert_eq!(shown["skill"]["slug"], OWNER_SKILL);

        let shown = f
            .show(&f.planner_tab, "nonce-planner")
            .expect("the Planner must read the plan's skill");
        assert_eq!(shown["skill"]["slug"], OWNER_SKILL);
    }

    /// codeterm-releases#53: a spoofed/stale/unproven owner claim is refused
    /// exactly like any other unrelated tab, never granted the bypass.
    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
    fn plan_skill_show_refuses_every_spoofed_or_unrelated_owner_claim() {
        let f = OwnerFixture::new();

        // Spoofed: the real owner's tab id, a nonce that tab never issued.
        assert!(f.show(&f.owner_tab, "nonce-guessed").is_err());
        // Stale: a nonce the owner tab held before it rotated.
        assert!(f.show(&f.owner_tab, "nonce-owner-previous").is_err());
        // Missing reported nonce: issued but never echoed back live.
        {
            let mut mux = f.svc.mux.write();
            mux.get_tab_mut(&f.owner_tab)
                .unwrap()
                .reported_session_binding_nonce = None;
        }
        assert!(f.show(&f.owner_tab, OWNER_NONCE).is_err());
        // Live and nonce-proven, but bound to neither owner nor Planner.
        assert!(f.show(&f.stranger_tab, "nonce-stranger").is_err());
    }
}

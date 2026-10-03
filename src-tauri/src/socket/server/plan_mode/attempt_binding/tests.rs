use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{OrchestratorRef, Plan, PlanMode, PlanSkillManifestEntry, TaskLaneMetadata};
use crate::service::plan::plan_skills;
use crate::service::plan::task_assignment::AssignmentRuntimeBinding;
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;
use serde_json::json;
use sha2::Digest;
use std::sync::Arc;

const OLD_NONCE: &str = "old-attempt-nonce";
const CURRENT_NONCE: &str = "current-session-nonce";
const SESSION_ID: &str = "same-provider-session";

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn attempt_binding_restore_existing_rebind_rpc_restores_current_attempt_for_real_skill_read() {
    let (mut mux, parent_tab_id) = MultiplexerState::new();
    mux.set_orchestrator_role(&parent_tab_id).unwrap();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&tab_id, AgentRole::Worker, Some(&parent_tab_id))
        .unwrap();
    let team_id = mux.get_tab(&tab_id).unwrap().team_id.clone().unwrap();
    assert_eq!(mux.get_tab(&tab_id).unwrap().role, Some(AgentRole::Worker));
    assert_eq!(mux.get_tab(&tab_id).unwrap().role_profile, None);
    assert!(mux.get_team(&team_id).is_some());
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("codex".into());
        tab.pty_id = Some("pty-worker".into());
        tab.agent_session_id = Some(SESSION_ID.into());
        tab.chat_view = true;
        tab.session_binding_nonce = Some(OLD_NONCE.into());
        tab.reported_session_binding_nonce = Some(OLD_NONCE.into());
    }

    let fixture = ServiceFixture::new(mux);
    let store = &fixture.service.plan_store;
    let mut plan = Plan::new(
        "Same attempt restoration".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    plan.mode = PlanMode::Execution;
    plan.team_id = Some(team_id.clone());
    assert_eq!(plan.team_id.as_deref(), Some(team_id.as_str()));
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Read the announced skill".into(),
                kind: "implement".into(),
                lane: TaskLaneMetadata {
                    workspace_id: Some(workspace_id),
                    ..Default::default()
                },
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_runtime_binding(
            &plan.id,
            &task_id,
            &tab_id,
            None,
            Some("worker"),
            Some(&AssignmentRuntimeBinding {
                session_id: SESSION_ID.into(),
                session_binding_nonce: OLD_NONCE.into(),
                provider: Some("codex".into()),
                model: None,
                session_model: None,
                reasoning_effort: None,
            }),
            None,
            3,
        )
        .unwrap();
    let attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
        .id
        .clone();
    store
        .acknowledge_task_attempt(&plan.id, &task_id, &tab_id, &attempt_id, 4)
        .unwrap();
    let skill = plan_skills::create_plan_skill(
        store,
        &plan.id,
        plan_skills::NewPlanSkill {
            slug: "restored-authority".into(),
            title: Some("Restored Authority".into()),
            area: Some("task-authority".into()),
            check: None,
            rationale: Some("exercise the real skill read".into()),
        },
    )
    .unwrap();
    let skill_body = plan_skills::show_plan_skill(store, &plan.id, &skill.slug)
        .unwrap()
        .body;
    let content_digest = format!("sha256:{:x}", sha2::Sha256::digest(skill_body.as_bytes()));
    store
        .set_attempt_plan_skill_manifest(
            &plan.id,
            &task_id,
            &attempt_id,
            vec![PlanSkillManifestEntry {
                slug: skill.slug.clone(),
                title: "Restored Authority".into(),
                purpose: "same-attempt regression".into(),
                content_digest,
            }],
            5,
        )
        .unwrap();

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.remember_superseded_binding_nonce();
        tab.session_binding_nonce = Some(CURRENT_NONCE.into());
        tab.reported_session_binding_nonce = Some(CURRENT_NONCE.into());
    }
    let owner_epoch = store
        .show_task(&task_id)
        .unwrap()
        .task
        .interruption
        .owner_epoch;
    let caller = json!({
        "plan_id": plan.id,
        "task_id": task_id,
        "attempt_id": attempt_id,
        "owner_epoch": owner_epoch,
        "caller_tab_id": tab_id,
        "session_id": SESSION_ID,
        "session_binding_nonce": CURRENT_NONCE,
    });
    let service = Arc::new(fixture.service.clone());
    let before_role_and_team_refusals = store.show_task(&task_id).unwrap().task;
    for refusal_cause in ["role", "role_profile", "missing_team", "unknown_team"] {
        {
            let mut mux = fixture.service.mux.write();
            let tab = mux.get_tab_mut(&tab_id).unwrap();
            tab.role = Some(AgentRole::Worker);
            tab.role_profile = None;
            tab.team_id = Some(team_id.clone());
            match refusal_cause {
                "role" => {
                    tab.role = Some(AgentRole::Manager);
                    tab.role_profile = Some("planner".into());
                }
                "role_profile" => tab.role_profile = Some("planner".into()),
                "missing_team" => tab.team_id = None,
                "unknown_team" => tab.team_id = Some("unregistered-team".into()),
                _ => unreachable!(),
            }
        }
        let before_mux_tab = {
            let mux = fixture.service.mux.read();
            serde_json::to_value(mux.get_tab(&tab_id).unwrap()).unwrap()
        };
        let refusal = super::super::dispatch(
            "plan.session.rebind",
            &Some(caller.clone()),
            &fixture.service.app_handle,
            &service,
        )
        .expect_err("targeted restoration requires the current Worker and its Team");
        assert_eq!(
            refusal.data.as_ref().unwrap()["type"],
            "attempt_binding_restore_refused"
        );
        assert_eq!(
            store.show_task(&task_id).unwrap().task,
            before_role_and_team_refusals
        );
        let mux = fixture.service.mux.read();
        assert_eq!(
            serde_json::to_value(mux.get_tab(&tab_id).unwrap()).unwrap(),
            before_mux_tab
        );
    }
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.role = Some(AgentRole::Worker);
        tab.role_profile = None;
        tab.team_id = Some(team_id.clone());
    }
    store
        .with_plan(&plan.id, 5, |stored_plan| {
            stored_plan.team_id = Some("different-team".into());
            Ok(())
        })
        .unwrap();
    let before_mux_tab = {
        let mux = fixture.service.mux.read();
        serde_json::to_value(mux.get_tab(&tab_id).unwrap()).unwrap()
    };
    let refusal = super::super::dispatch(
        "plan.session.rebind",
        &Some(caller.clone()),
        &fixture.service.app_handle,
        &service,
    )
    .expect_err("the plan Team must match the authenticated Worker Team");
    assert_eq!(
        refusal.data.as_ref().unwrap()["type"],
        "attempt_binding_restore_refused"
    );
    assert_eq!(
        store.show_task(&task_id).unwrap().task,
        before_role_and_team_refusals
    );
    let mux = fixture.service.mux.read();
    assert_eq!(
        serde_json::to_value(mux.get_tab(&tab_id).unwrap()).unwrap(),
        before_mux_tab
    );
    drop(mux);
    store
        .with_plan(&plan.id, 5, |stored_plan| {
            stored_plan.team_id = Some(team_id.clone());
            Ok(())
        })
        .unwrap();
    let rebound = super::super::dispatch(
        "plan.session.rebind",
        &Some(caller.clone()),
        &fixture.service.app_handle,
        &service,
    )
    .expect("the existing rebind RPC should restore the exact acknowledged attempt");

    let read = crate::socket::server::plan_skills_rpc::dispatch(
        "plan.skills.show",
        &Some(json!({
            "plan_id": plan.id,
            "slug": skill.slug,
            "task_id": task_id,
            "attempt_id": attempt_id,
            "caller_tab_id": tab_id,
            "caller_session_id": SESSION_ID,
            "session_binding_nonce": CURRENT_NONCE,
        })),
        &service,
        6,
    );
    assert!(
        read.is_ok(),
        "same-attempt rebind should authorize the real skill read; current-attempt guard returned {read:?}; RPC result was {rebound}"
    );
    let read = read.unwrap();
    assert_eq!(read["skill"]["slug"], skill.slug);
    let before_refusal = store.show_task(&task_id).unwrap().task;
    for (field, invalid) in [
        ("caller_tab_id", "unrelated-tab"),
        ("session_id", "unrelated-session"),
        ("session_binding_nonce", "unrelated-current-nonce"),
    ] {
        let mut wrong_caller = caller.clone();
        wrong_caller[field] = json!(invalid);
        let refusal = super::super::dispatch(
            "plan.session.rebind",
            &Some(wrong_caller),
            &fixture.service.app_handle,
            &service,
        )
        .expect_err("a caller without exact live tab identity must be refused");
        assert_eq!(
            refusal.data.as_ref().unwrap()["type"],
            "attempt_binding_restore_refused"
        );
        assert_eq!(store.show_task(&task_id).unwrap().task, before_refusal);
    }
    {
        let mut mux = fixture.service.mux.write();
        mux.get_tab_mut(&tab_id)
            .unwrap()
            .reported_session_binding_nonce = Some("unrelated-reported-nonce".into());
    }
    let refusal = super::super::dispatch(
        "plan.session.rebind",
        &Some(caller.clone()),
        &fixture.service.app_handle,
        &service,
    )
    .expect_err("the provider-reported nonce must equal the issued current nonce");
    assert_eq!(
        refusal.data.as_ref().unwrap()["type"],
        "attempt_binding_restore_refused"
    );
    assert_eq!(store.show_task(&task_id).unwrap().task, before_refusal);
    {
        let mut mux = fixture.service.mux.write();
        mux.get_tab_mut(&tab_id)
            .unwrap()
            .reported_session_binding_nonce = Some(CURRENT_NONCE.into());
    }
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.session_binding_nonce.as_deref(), Some(CURRENT_NONCE));
        assert_eq!(
            tab.reported_session_binding_nonce.as_deref(),
            Some(CURRENT_NONCE)
        );
    }
    assert_eq!(rebound["status"], "restored");
    assert_eq!(rebound["plan_id"], plan.id);
    assert_eq!(rebound["task_id"], task_id);
    assert_eq!(rebound["attempt_id"], attempt_id);
    assert!(rebound.get("session_binding_nonce").is_none());
    let after_restore = store.show_task(&task_id).unwrap().task;
    assert_eq!(
        after_restore.execution_attempts[0]
            .binding_restorations
            .len(),
        1
    );
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.session_binding_nonce.as_deref(), Some(CURRENT_NONCE));
        assert_eq!(
            tab.reported_session_binding_nonce.as_deref(),
            Some(CURRENT_NONCE)
        );
    }
    let replay = super::super::dispatch(
        "plan.session.rebind",
        &Some(caller),
        &fixture.service.app_handle,
        &service,
    )
    .expect("the exact current binding should be idempotent");
    assert_eq!(replay["status"], "already_current");
    assert_eq!(store.show_task(&task_id).unwrap().task, after_restore);
    assert_eq!(
        store.show_task(&task_id).unwrap().task.execution_attempts[0]
            .loaded_plan_skills
            .len(),
        1
    );
}

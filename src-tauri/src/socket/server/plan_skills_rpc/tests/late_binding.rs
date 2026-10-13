use super::*;
use crate::multiplexer::types::{AgentRole, TaskBinding};
use crate::service::startup_lease::StartupTaskIdentity;

const LAUNCH: &str = "retained-launch-pty";

struct LateLaunch {
    caller: LiveCaller,
    tab: String,
    parent: String,
    attempt: String,
}

impl LateLaunch {
    fn new() -> Self {
        Self::start("active")
    }

    fn start(state: &str) -> Self {
        let caller = LiveCaller::new();
        let tab = caller.tab_id();
        let (parent, team) = {
            let mut mux = caller.svc.mux.write();
            let workspace = mux.find_workspace_for_tab(&tab).unwrap();
            let parent = mux.create_tab(&workspace).unwrap();
            mux.get_tab_mut(&parent).unwrap().agent_type = Some("claude".into());
            mux.set_orchestrator_role(&parent).unwrap();
            mux.set_tab_role(&tab, AgentRole::Worker, Some(&parent))
                .unwrap();
            (parent, mux.get_tab(&tab).unwrap().team_id.clone().unwrap())
        };
        caller
            .svc
            .plan_store
            .with_plan(&caller.plan_id, 8, |plan| {
                plan.team_id = Some(team);
                Ok(())
            })
            .unwrap();
        crate::socket::server::dispatch_service_rpc(
            "agent.ack",
            Some(json!({
                "tab_id": tab, "session_binding_nonce": CALLER_NONCE,
            })),
            &caller.svc,
        )
        .unwrap();
        caller.show(caller.production_params()).unwrap();
        caller
            .svc
            .plan_store
            .revert_task_dispatch(&caller.plan_id, &caller.task_id, 9)
            .unwrap();
        let attempt = caller
            .svc
            .plan_store
            .begin_task_dispatch_with_policy(
                &caller.plan_id,
                &caller.task_id,
                &tab,
                "Worker",
                crate::plan::TaskLaneMetadata::default(),
                crate::plan::ExecutionIntent::Unspecified,
                None,
                Some("claude"),
                10,
            )
            .unwrap();
        let manifest = crate::service::plan::plan_skills::plan_skill_manifest(
            &caller.svc.plan_store,
            &caller.plan_id,
        )
        .unwrap();
        caller
            .svc
            .plan_store
            .set_attempt_plan_skill_manifest(
                &caller.plan_id,
                &caller.task_id,
                &attempt,
                manifest,
                11,
            )
            .unwrap();
        {
            let mut mux = caller.svc.mux.write();
            let worker = mux.get_tab_mut(&tab).unwrap();
            worker.pty_id = Some(LAUNCH.into());
            worker.agent_booting = true;
            worker.agent_session_id = None;
            worker.reported_session_binding_nonce = None;
            mux.set_task_binding_once(
                &tab,
                TaskBinding {
                    plan_id: Some(caller.plan_id.clone()),
                    task_id: Some(caller.task_id.clone()),
                    attempt_id: Some(attempt.clone()),
                    spawn_handle: None,
                    bound_at: 10,
                },
            )
            .unwrap();
        }
        let identity = StartupTaskIdentity {
            plan_id: caller.plan_id.clone(),
            task_id: caller.task_id.clone(),
            attempt_id: attempt.clone(),
        };
        assert!(caller.svc.prepare_agent_startup_supervision(
            &tab,
            Some(&parent),
            Some(identity.clone())
        ));
        if state != "absent" {
            caller
                .svc
                .activate_agent_startup_supervision(&tab, LAUNCH, CALLER_NONCE, Some(&identity))
                .unwrap();
        }
        let launch = Self {
            caller,
            tab,
            parent,
            attempt,
        };
        if state == "finalized" {
            launch
                .caller
                .svc
                .hold_agent_startup(&launch.parent, &launch.tab, LAUNCH)
                .unwrap();
        }
        let early = launch.ack().unwrap();
        assert_eq!(early["state"], "awaiting_runtime_ready");
        assert_eq!(early["attempt_id"], launch.attempt);
        launch
    }

    fn ack(&self) -> Result<Value, JsonRpcError> {
        crate::socket::server::dispatch_service_rpc(
            "agent.ack",
            Some(json!({
                "tab_id": self.tab, "session_binding_nonce": CALLER_NONCE,
            })),
            &self.caller.svc,
        )
    }

    fn ready(&self) {
        let mut mux = self.caller.svc.mux.write();
        let tab = mux.get_tab_mut(&self.tab).unwrap();
        tab.agent_booting = false;
        tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
        tab.agent_session_id = Some(CALLER_SESSION.into());
        tab.reported_session_binding_nonce = Some(CALLER_NONCE.into());
    }

    fn task(&self) -> crate::plan::Task {
        self.caller
            .svc
            .plan_store
            .show_task(&self.caller.task_id)
            .unwrap()
            .task
    }

    fn edit_task(&self, edit: impl FnOnce(&mut crate::plan::Task)) {
        self.caller
            .svc
            .plan_store
            .with_plan(&self.caller.plan_id, 12, |plan| {
                edit(
                    plan.tasks
                        .iter_mut()
                        .find(|task| task.id == self.caller.task_id)
                        .unwrap(),
                );
                Ok(())
            })
            .unwrap();
    }
}

impl Drop for LateLaunch {
    fn drop(&mut self) {
        self.caller.svc.cancel_agent_startup_supervision(&self.tab);
    }
}

#[test]
fn retained_launch_early_ack_completes_only_same_attempt_then_exact_load() {
    let launch = LateLaunch::new();
    let before = launch.task();
    assert!(before.execution_attempts[1].session_id.is_none());
    assert!(before.execution_attempts[1].session_binding_nonce.is_none());
    launch.ready();
    let response = launch.ack().unwrap();
    assert_eq!(response["attempt_id"], launch.attempt);
    assert_eq!(response["state"], "already_acknowledged");
    assert_eq!(response["accepted"], true);
    assert_eq!(response["already_acknowledged"], true);
    assert_eq!(response["plan_id"], launch.caller.plan_id);
    assert_eq!(response["task_id"], launch.caller.task_id);
    assert_eq!(response["tab_id"], launch.tab);
    assert_eq!(
        response["acknowledged_at"],
        json!(before.execution_attempts[1].acknowledged_at)
    );
    let mut expected = before.clone();
    expected.execution_attempts[1].session_id = Some(CALLER_SESSION.into());
    expected.execution_attempts[1].session_binding_nonce = Some(CALLER_NONCE.into());
    expected.interruption.owner_session_id = Some(CALLER_SESSION.into());
    assert_eq!(launch.task(), expected);
    assert_eq!(launch.ack().unwrap(), response);
    assert_eq!(launch.task(), expected);
    let mut read = launch.caller.production_params();
    read["attempt_id"] = json!(launch.attempt);
    let shown = launch.caller.show(read).unwrap();
    let digest = format!(
        "sha256:{:x}",
        sha2::Sha256::digest(shown["skill"]["body"].as_str().unwrap().as_bytes())
    );
    let loaded = launch.task();
    assert_eq!(loaded.execution_attempts[0], before.execution_attempts[0]);
    assert_eq!(loaded.execution_attempts[1].loaded_plan_skills.len(), 1);
    assert_eq!(
        loaded.execution_attempts[1].loaded_plan_skills[0].content_digest,
        digest
    );
    assert_eq!(
        loaded.execution_attempts[1].plan_skill_manifest,
        before.execution_attempts[1].plan_skill_manifest
    );
}

#[test]
fn retained_launch_refuses_missing_replaced_finalized_and_foreign_live_authority() {
    for variant in [
        "absent",
        "finalized",
        "pty",
        "nonce",
        "reported",
        "creator",
        "team",
        "session_claim",
        "attempt_claim",
        "assignment",
    ] {
        let launch = LateLaunch::start(variant);
        launch.ready();
        {
            let mut mux = launch.caller.svc.mux.write();
            let tab = mux.get_tab_mut(&launch.tab).unwrap();
            match variant {
                "pty" => tab.pty_id = Some("foreign-launch".into()),
                "nonce" => {
                    tab.session_binding_nonce = Some("foreign-nonce".into());
                    tab.reported_session_binding_nonce = Some("foreign-nonce".into());
                }
                "reported" => tab.reported_session_binding_nonce = None,
                "creator" => tab.parent_tab_id = Some("foreign-creator".into()),
                "team" => tab.team_id = Some("foreign-team".into()),
                "assignment" => {
                    tab.task_binding.as_mut().unwrap().attempt_id = Some("foreign-attempt".into())
                }
                _ => {}
            }
        }
        let before = launch.task();
        let result = if variant == "session_claim" || variant == "attempt_claim" {
            let mut params = json!({ "tab_id": launch.tab, "session_binding_nonce": CALLER_NONCE });
            params[if variant == "session_claim" {
                "caller_session_id"
            } else {
                "attempt_id"
            }] = json!("foreign-claim");
            crate::socket::server::dispatch_service_rpc(
                "agent.ack",
                Some(params),
                &launch.caller.svc,
            )
        } else {
            launch.ack()
        };
        assert!(result.is_err(), "{variant}");
        assert_eq!(launch.task(), before, "{variant}");
    }
}

#[test]
fn retained_launch_refuses_partial_foreign_ambiguous_retired_epoch_and_manifest() {
    for variant in [
        "partial_session",
        "partial_nonce",
        "foreign",
        "ambiguous",
        "retired",
        "closed",
        "owner_session",
        "epoch",
        "manifest",
        "digest",
    ] {
        let launch = LateLaunch::new();
        launch.ready();
        launch.edit_task(|task| {
            let attempt = &mut task.execution_attempts[1];
            match variant {
                "partial_session" => attempt.session_id = Some(CALLER_SESSION.into()),
                "partial_nonce" => attempt.session_binding_nonce = Some(CALLER_NONCE.into()),
                "foreign" => {
                    attempt.session_id = Some("foreign-session".into());
                    attempt.session_binding_nonce = Some("foreign-nonce".into());
                }
                "ambiguous" => {
                    let mut duplicate = attempt.clone();
                    duplicate.id = "ambiguous-open".into();
                    task.execution_attempts.push(duplicate);
                }
                "retired" => attempt.retired_at = Some(12),
                "closed" => attempt.closed_at = Some(12),
                "owner_session" => {
                    task.interruption.owner_session_id = Some("foreign-owner".into())
                }
                "epoch" => {
                    task.interruption.owner_epoch += 1;
                    attempt.interruption.owner_epoch = task.interruption.owner_epoch;
                }
                "manifest" => attempt.plan_skill_manifest.clear(),
                "digest" => attempt.plan_skill_manifest[0].content_digest = "sha256:changed".into(),
                _ => unreachable!(),
            }
        });
        let before = launch.task();
        assert!(launch.ack().is_err(), "{variant}");
        assert_eq!(launch.task(), before, "{variant}");
    }
}

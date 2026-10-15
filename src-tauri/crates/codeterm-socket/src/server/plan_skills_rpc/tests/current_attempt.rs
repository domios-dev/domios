use super::*;

fn assign_current(caller: &LiveCaller, now: u64) {
    let tab_id = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task
        .execution_attempts
        .last()
        .unwrap()
        .executor_tab_id
        .clone();
    let assignment = {
        let mux = caller.svc.mux.read();
        codeterm_service_core::service::task_assignment::prepare_existing_task_assignment(
            &mux, &tab_id, None, None,
        )
        .unwrap()
    };
    codeterm_service_core::service::task_assignment::persist_existing_task_assignment(
        &caller.svc.plan_store,
        assignment,
        &caller.plan_id,
        &caller.task_id,
        &tab_id,
        None,
        None,
        now,
    )
    .unwrap();
}

fn ack(caller: &LiveCaller) -> Value {
    crate::server::agent::dispatch(
        "agent.ack",
        &Some(json!({
            "tab_id": caller.tab_id(),
            "session_binding_nonce": CALLER_NONCE,
        })),
        &caller.svc,
    )
    .unwrap()
}

#[test]
fn current_attempt_reassignment_ack_announces_before_exact_skill_load() {
    let caller = LiveCaller::new();
    ack(&caller);
    caller.show(caller.production_params()).unwrap();
    caller
        .svc
        .plan_store
        .revert_task_dispatch(&caller.plan_id, &caller.task_id, 8)
        .unwrap();
    let closed = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task
        .execution_attempts[0]
        .clone();
    assert!(closed.closed_at.is_some());
    assert_eq!(closed.loaded_plan_skills.len(), 1);
    assign_current(&caller, 9);
    let current = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task
        .execution_attempts[1]
        .clone();
    assert!(current.plan_skill_manifest.is_empty());
    assert!(current.loaded_plan_skills.is_empty());
    let mut read = caller.production_params();
    read.as_object_mut().unwrap().remove("task_id");
    read.as_object_mut().unwrap().remove("attempt_id");
    assert!(caller.show(read.clone()).is_err());
    let response = ack(&caller);
    assert_eq!(response["attempt_id"], current.id);
    let announced: Vec<PlanSkillManifestEntry> =
        serde_json::from_value(response["plan_skill_manifest"].clone()).unwrap();
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0].slug, CALLER_SKILL);
    assert!(response["plan_skill_brief"]
        .as_str()
        .unwrap()
        .contains(&announced[0].content_digest));
    let shown = caller.show(read).unwrap();
    let digest = format!(
        "sha256:{:x}",
        sha2::Sha256::digest(shown["skill"]["body"].as_str().unwrap().as_bytes())
    );
    let after = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task;
    assert_eq!(after.execution_attempts[0], closed);
    assert_eq!(after.execution_attempts[1].plan_skill_manifest, announced);
    assert_eq!(after.execution_attempts[1].loaded_plan_skills.len(), 1);
    assert_eq!(
        after.execution_attempts[1].loaded_plan_skills[0].content_digest,
        digest
    );
    assert!(after.execution_attempts[1].acknowledged_at.is_some());
    let replay = ack(&caller);
    assert_eq!(
        replay["plan_skill_manifest"],
        response["plan_skill_manifest"]
    );
    assert_eq!(
        caller
            .svc
            .plan_store
            .show_task(&caller.task_id)
            .unwrap()
            .task,
        after
    );
}

#[test]
fn current_attempt_explicit_conflicts_and_ambiguity_refuse_without_receipts() {
    let caller = LiveCaller::new();
    for (field, value) in [
        ("task_id", "foreign-task"),
        ("attempt_id", "closed-attempt"),
        ("caller_session_id", "foreign-session"),
        ("session_binding_nonce", "foreign-nonce"),
    ] {
        let mut read = caller.production_params();
        read[field] = json!(value);
        assert!(caller.show(read).is_err());
    }
    let before = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task;
    assert!(before.execution_attempts[0].loaded_plan_skills.is_empty());
    caller
        .svc
        .plan_store
        .with_plan(&caller.plan_id, 8, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == caller.task_id)
                .unwrap();
            let mut duplicate = task.execution_attempts[0].clone();
            duplicate.id = "another-open-attempt".into();
            task.execution_attempts.push(duplicate);
            Ok(())
        })
        .unwrap();
    let ambiguous = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task;
    assert!(caller.show(caller.production_params()).is_err());
    assert_eq!(
        caller
            .svc
            .plan_store
            .show_task(&caller.task_id)
            .unwrap()
            .task,
        ambiguous
    );
}

#[test]
fn current_attempt_team_default_follows_changed_active_plan() {
    let caller = LiveCaller::new();
    let tab_id = caller.tab_id();
    let team_id = {
        let mut mux = caller.svc.mux.write();
        mux.set_orchestrator_role(&tab_id).unwrap();
        mux.resolve_or_mint_team_for_parent(&tab_id)
            .unwrap()
            .team_id
    };
    caller
        .svc
        .plan_store
        .with_plan(&caller.plan_id, 8, |plan| {
            plan.team_id = Some(team_id.clone());
            Ok(())
        })
        .unwrap();
    let mut params = caller.production_params();
    for key in ["plan_id", "task_id", "attempt_id"] {
        params.as_object_mut().unwrap().remove(key);
    }
    assert_eq!(
        caller.show(params.clone()).unwrap()["plan_id"],
        caller.plan_id
    );
    caller.svc.plan_store.archive(&caller.plan_id, 9).unwrap();
    let next = caller
        .svc
        .plan_store
        .create_plan_for_team(
            "Next active plan".into(),
            caller._project.path().display().to_string(),
            OrchestratorRef::from_tab(&tab_id),
            team_id.clone(),
            vec![],
            10,
        )
        .unwrap();
    create_plan_skill(
        &caller.svc.plan_store,
        &next.id,
        NewPlanSkill {
            slug: CALLER_SKILL.into(),
            title: Some("next-plan-token".into()),
            area: None,
            check: None,
            rationale: None,
        },
    )
    .unwrap();
    let shown = caller.show(params).unwrap();
    assert_eq!(shown["plan_id"], next.id);
    assert_eq!(shown["skill"]["title"], "next-plan-token");
    let other = caller
        .svc
        .plan_store
        .create_plan_for_team(
            "Explicit other active plan".into(),
            caller._project.path().display().to_string(),
            OrchestratorRef::from_tab("other-owner"),
            "explicit-other-team".into(),
            vec![],
            11,
        )
        .unwrap();
    create_plan_skill(
        &caller.svc.plan_store,
        &other.id,
        NewPlanSkill {
            slug: CALLER_SKILL.into(),
            title: Some("explicit-other-plan-token".into()),
            area: None,
            check: None,
            rationale: None,
        },
    )
    .unwrap();
    let original = caller.svc.plan_store.get(&caller.plan_id).unwrap();
    let active = caller.svc.plan_store.get(&next.id).unwrap();
    let other_before = caller.svc.plan_store.get(&other.id).unwrap();
    let explicit = caller
        .show(json!({
            "plan_id": other.id, "slug": CALLER_SKILL,
            "caller_tab_id": tab_id, "session_binding_nonce": CALLER_NONCE,
        }))
        .unwrap();
    assert_eq!(explicit["plan_id"], other.id);
    assert_eq!(explicit["skill"]["title"], "explicit-other-plan-token");
    assert_eq!(
        caller.svc.plan_store.get(&caller.plan_id).unwrap(),
        original
    );
    assert_eq!(caller.svc.plan_store.get(&next.id).unwrap(), active);
    assert_eq!(caller.svc.plan_store.get(&other.id).unwrap(), other_before);
    assert_eq!(
        caller
            .svc
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .team_id
            .as_deref(),
        Some(team_id.as_str())
    );
    assert_eq!(
        caller
            .svc
            .plan_store
            .active_plan_for_team(&team_id)
            .unwrap()
            .0,
        next.id
    );
}

#[test]
fn current_attempt_other_plan_read_does_not_rebind_or_record_a_load() {
    let caller = LiveCaller::new();
    let team_id = {
        let mut mux = caller.svc.mux.write();
        mux.set_orchestrator_role(&caller.tab_id()).unwrap();
        mux.resolve_or_mint_team_for_parent(&caller.tab_id())
            .unwrap()
            .team_id
    };
    caller
        .svc
        .plan_store
        .with_plan(&caller.plan_id, 8, |plan| {
            plan.team_id = Some(team_id.clone());
            Ok(())
        })
        .unwrap();
    let other = caller
        .svc
        .plan_store
        .create_plan_for_team(
            "Other".into(),
            caller._project.path().display().to_string(),
            OrchestratorRef::from_tab("other-owner"),
            "other-team".into(),
            vec![],
            8,
        )
        .unwrap();
    create_plan_skill(
        &caller.svc.plan_store,
        &other.id,
        NewPlanSkill {
            slug: CALLER_SKILL.into(),
            title: Some("other-plan-token".into()),
            area: None,
            check: None,
            rationale: None,
        },
    )
    .unwrap();
    let before = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task;
    let shown = caller
        .show(json!({
            "plan_id": other.id, "slug": CALLER_SKILL,
            "caller_tab_id": caller.tab_id(), "session_binding_nonce": CALLER_NONCE,
        }))
        .unwrap();
    assert_eq!(shown["skill"]["title"], "other-plan-token");
    assert_eq!(
        caller
            .svc
            .plan_store
            .show_task(&caller.task_id)
            .unwrap()
            .task,
        before
    );
    assert_eq!(
        caller
            .svc
            .plan_store
            .task_binding_for_tab(&caller.tab_id())
            .unwrap(),
        (caller.plan_id.clone(), caller.task_id.clone())
    );
    caller
        .svc
        .plan_store
        .with_plan(&caller.plan_id, 9, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == caller.task_id)
                .unwrap();
            task.execution_attempts[0].session_id = Some("previous-session".into());
            task.execution_attempts[0].plan_skill_manifest.clear();
            Ok(())
        })
        .unwrap();
    let without_receipt = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task;
    let read = json!({
        "plan_id": other.id, "slug": CALLER_SKILL,
        "caller_tab_id": caller.tab_id(), "session_binding_nonce": CALLER_NONCE,
    });
    assert_eq!(caller.show(read.clone()).unwrap()["plan_id"], other.id);
    let mut conflicting = read;
    conflicting["task_id"] = json!(caller.task_id);
    assert!(caller.show(conflicting).is_err());
    assert_eq!(
        caller
            .svc
            .plan_store
            .show_task(&caller.task_id)
            .unwrap()
            .task,
        without_receipt
    );
    assert_eq!(
        caller
            .svc
            .mux
            .read()
            .get_tab(&caller.tab_id())
            .unwrap()
            .team_id
            .as_deref(),
        Some(team_id.as_str())
    );
}

#[test]
fn current_attempt_ack_keeps_announced_digest_when_skill_bytes_change() {
    let caller = LiveCaller::new();
    let first = ack(&caller);
    let before = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task;
    let plan = caller.svc.plan_store.get(&caller.plan_id).unwrap();
    let path = plan_dir(plan_sidecar_root(&plan), &plan.status, &plan.id)
        .unwrap()
        .join("skills")
        .join(CALLER_SKILL)
        .join("SKILL.md");
    std::fs::write(path, "changed-body-token").unwrap();
    assert!(caller.show(caller.production_params()).is_err());
    let replay = ack(&caller);
    assert_eq!(replay["plan_skill_manifest"], first["plan_skill_manifest"]);
    assert_eq!(
        caller
            .svc
            .plan_store
            .show_task(&caller.task_id)
            .unwrap()
            .task,
        before
    );
}

#[test]
fn current_attempt_ack_rejects_foreign_nonce_before_announcement() {
    let caller = LiveCaller::new();
    caller
        .svc
        .plan_store
        .revert_task_dispatch(&caller.plan_id, &caller.task_id, 8)
        .unwrap();
    assign_current(&caller, 9);
    caller
        .svc
        .plan_store
        .acknowledge_live_task_attempt_for_tab(&caller.tab_id(), None, None, 10)
        .unwrap();
    let before = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task;
    let result = crate::server::agent::dispatch(
        "agent.ack",
        &Some(json!({ "tab_id": caller.tab_id(), "session_binding_nonce": "foreign-nonce" })),
        &caller.svc,
    );
    assert!(result.is_err());
    assert_eq!(
        caller
            .svc
            .plan_store
            .show_task(&caller.task_id)
            .unwrap()
            .task,
        before
    );
}

#[test]
fn current_attempt_startup_ack_keeps_existing_runtime_qualification() {
    let caller = LiveCaller::new();
    caller
        .svc
        .plan_store
        .with_plan(&caller.plan_id, 8, |plan| {
            let attempt = &mut plan
                .tasks
                .iter_mut()
                .find(|task| task.id == caller.task_id)
                .unwrap()
                .execution_attempts[0];
            attempt.session_id = None;
            attempt.session_binding_nonce = None;
            Ok(())
        })
        .unwrap();
    let before = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task
        .execution_attempts[0]
        .plan_skill_manifest
        .clone();
    let response = ack(&caller);
    assert_eq!(response["state"], "awaiting_runtime_ready");
    let announced: Vec<PlanSkillManifestEntry> =
        serde_json::from_value(response["plan_skill_manifest"].clone()).unwrap();
    assert_eq!(announced, before);
    assert!(caller.loaded_skills().is_empty());
}

#[test]
fn current_attempt_early_ack_late_ready_refuses_unproven_binding_without_mutation() {
    let caller = LiveCaller::new();
    let tab_id = caller.tab_id();
    ack(&caller);
    caller.show(caller.production_params()).unwrap();
    caller
        .svc
        .plan_store
        .revert_task_dispatch(&caller.plan_id, &caller.task_id, 8)
        .unwrap();
    let predecessor = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task
        .execution_attempts[0]
        .clone();
    assert!(predecessor.closed_at.is_some());
    assert_eq!(predecessor.loaded_plan_skills.len(), 1);
    {
        let mut mux = caller.svc.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_booting = true;
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
    }
    let attempt_id = caller
        .svc
        .plan_store
        .begin_task_dispatch(
            &caller.plan_id,
            &caller.task_id,
            &tab_id,
            "Worker",
            codeterm_service_core::plan::TaskLaneMetadata::default(),
            9,
        )
        .unwrap();
    let manifest = codeterm_service_core::service::plan::plan_skills::plan_skill_manifest(
        &caller.svc.plan_store,
        &caller.plan_id,
    )
    .unwrap();
    assert_eq!(manifest.len(), 1);
    caller
        .svc
        .plan_store
        .set_attempt_plan_skill_manifest(
            &caller.plan_id,
            &caller.task_id,
            &attempt_id,
            manifest.clone(),
            10,
        )
        .unwrap();
    let early = ack(&caller);
    assert_eq!(early["attempt_id"], attempt_id);
    assert_eq!(early["state"], "awaiting_runtime_ready");
    assert_eq!(early["plan_skill_manifest"], json!(manifest));
    let acknowledged = caller
        .svc
        .plan_store
        .show_task(&caller.task_id)
        .unwrap()
        .task;
    assert_eq!(acknowledged.execution_attempts[0], predecessor);
    let current = &acknowledged.execution_attempts[1];
    assert!(current.acknowledged_at.is_some());
    assert!(current.session_id.is_none());
    assert!(current.session_binding_nonce.is_none());
    assert!(current.closed_at.is_none());
    assert!(current.retired_at.is_none());
    assert!(current.loaded_plan_skills.is_empty());
    {
        let mut mux = caller.svc.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_booting = false;
        tab.agent_session_id = Some(CALLER_SESSION.into());
        tab.reported_session_binding_nonce = Some(CALLER_NONCE.into());
    }
    let error = crate::server::agent::dispatch(
        "agent.ack",
        &Some(json!({ "tab_id": tab_id, "session_binding_nonce": CALLER_NONCE })),
        &caller.svc,
    )
    .unwrap_err();
    let refusal = error.data.unwrap();
    assert_eq!(refusal["type"], "plan_skill_attempt_binding_missing");
    assert_eq!(refusal["plan_id"], caller.plan_id);
    assert_eq!(refusal["task_id"], caller.task_id);
    assert_eq!(refusal["attempt_id"], attempt_id);
    assert_eq!(refusal["acknowledged_at"], early["acknowledged_at"]);
    assert_eq!(
        refusal["recovery"],
        "orchestrator_launch_proven_binding_repair"
    );
    let mut read = caller.production_params();
    read["attempt_id"] = json!(attempt_id);
    assert!(caller.show(read).is_err());
    assert_eq!(
        caller
            .svc
            .plan_store
            .show_task(&caller.task_id)
            .unwrap()
            .task,
        acknowledged
    );
}

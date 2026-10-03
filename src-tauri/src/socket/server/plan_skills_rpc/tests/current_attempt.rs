use super::*;

fn assign_current(caller: &LiveCaller, now: u64) {
    let tab_id = caller.svc.plan_store.show_task(&caller.task_id).unwrap().task.execution_attempts.last().unwrap().executor_tab_id.clone();
    let assignment = {
        let mux = caller.svc.mux.read();
        crate::service::task_assignment::prepare_existing_task_assignment(
            &mux, &tab_id, None, None,
        ).unwrap()
    };
    crate::service::task_assignment::persist_existing_task_assignment(
        &caller.svc.plan_store, assignment, &caller.plan_id, &caller.task_id,
        &tab_id, None, None, now,
    ).unwrap();
}

fn ack(caller: &LiveCaller) -> Value {
    crate::socket::server::agent::dispatch(
        "agent.ack",
        &Some(json!({
            "tab_id": caller.tab_id(),
            "session_binding_nonce": CALLER_NONCE,
        })),
        &caller.svc.app_handle, &caller.svc,
    ).unwrap()
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn current_attempt_reassignment_ack_announces_before_exact_skill_load() {
    let caller = LiveCaller::new();
    ack(&caller);
    caller.show(caller.production_params()).unwrap();
    caller.svc.plan_store.revert_task_dispatch(&caller.plan_id, &caller.task_id, 8).unwrap();
    let closed = caller.svc.plan_store.show_task(&caller.task_id).unwrap().task.execution_attempts[0].clone();
    assert!(closed.closed_at.is_some());
    assert_eq!(closed.loaded_plan_skills.len(), 1);
    assign_current(&caller, 9);
    let current = caller.svc.plan_store.show_task(&caller.task_id).unwrap().task.execution_attempts[1].clone();
    assert!(current.plan_skill_manifest.is_empty());
    assert!(current.loaded_plan_skills.is_empty());
    let mut read = caller.production_params();
    read.as_object_mut().unwrap().remove("task_id");
    read.as_object_mut().unwrap().remove("attempt_id");
    assert!(caller.show(read.clone()).is_err());
    let response = ack(&caller);
    assert_eq!(response["attempt_id"], current.id);
    let announced: Vec<PlanSkillManifestEntry> = serde_json::from_value(response["plan_skill_manifest"].clone()).unwrap();
    assert_eq!(announced.len(), 1);
    assert_eq!(announced[0].slug, CALLER_SKILL);
    assert!(response["plan_skill_brief"].as_str().unwrap().contains(&announced[0].content_digest));
    let shown = caller.show(read).unwrap();
    let digest = format!("sha256:{:x}", sha2::Sha256::digest(shown["skill"]["body"].as_str().unwrap().as_bytes()));
    let after = caller.svc.plan_store.show_task(&caller.task_id).unwrap().task;
    assert_eq!(after.execution_attempts[0], closed);
    assert_eq!(after.execution_attempts[1].plan_skill_manifest, announced);
    assert_eq!(after.execution_attempts[1].loaded_plan_skills.len(), 1);
    assert_eq!(after.execution_attempts[1].loaded_plan_skills[0].content_digest, digest);
    assert!(after.execution_attempts[1].acknowledged_at.is_some());
    let replay = ack(&caller);
    assert_eq!(replay["plan_skill_manifest"], response["plan_skill_manifest"]);
    assert_eq!(caller.svc.plan_store.show_task(&caller.task_id).unwrap().task, after);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn current_attempt_explicit_conflicts_and_ambiguity_refuse_without_receipts() {
    let caller = LiveCaller::new();
    for (field, value) in [("task_id", "foreign-task"), ("attempt_id", "closed-attempt"), ("caller_session_id", "foreign-session"), ("session_binding_nonce", "foreign-nonce")] {
        let mut read = caller.production_params();
        read[field] = json!(value);
        assert!(caller.show(read).is_err());
    }
    let before = caller.svc.plan_store.show_task(&caller.task_id).unwrap().task;
    assert!(before.execution_attempts[0].loaded_plan_skills.is_empty());
    caller.svc.plan_store.with_plan(&caller.plan_id, 8, |plan| {
        let task = plan.tasks.iter_mut().find(|task| task.id == caller.task_id).unwrap();
        let mut duplicate = task.execution_attempts[0].clone();
        duplicate.id = "another-open-attempt".into();
        task.execution_attempts.push(duplicate);
        Ok(())
    }).unwrap();
    let ambiguous = caller.svc.plan_store.show_task(&caller.task_id).unwrap().task;
    assert!(caller.show(caller.production_params()).is_err());
    assert_eq!(caller.svc.plan_store.show_task(&caller.task_id).unwrap().task, ambiguous);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn current_attempt_team_default_follows_changed_active_plan() {
    let caller = LiveCaller::new();
    let tab_id = caller.tab_id();
    let team_id = {
        let mut mux = caller.svc.mux.write();
        mux.set_orchestrator_role(&tab_id).unwrap();
        mux.resolve_or_mint_team_for_parent(&tab_id).unwrap().team_id
    };
    caller.svc.plan_store.with_plan(&caller.plan_id, 8, |plan| { plan.team_id = Some(team_id.clone()); Ok(()) }).unwrap();
    let mut params = caller.production_params();
    for key in ["plan_id", "task_id", "attempt_id"] { params.as_object_mut().unwrap().remove(key); }
    assert_eq!(caller.show(params.clone()).unwrap()["plan_id"], caller.plan_id);
    caller.svc.plan_store.archive(&caller.plan_id, 9).unwrap();
    let next = caller.svc.plan_store.create_plan_for_team(
        "Next active plan".into(), caller._project.path().display().to_string(),
        OrchestratorRef::from_tab(&tab_id), team_id, vec![], 10,
    ).unwrap();
    create_plan_skill(&caller.svc.plan_store, &next.id, NewPlanSkill {
        slug: CALLER_SKILL.into(), title: Some("next-plan-token".into()), area: None,
        check: None, rationale: None,
    }).unwrap();
    let shown = caller.show(params).unwrap();
    assert_eq!(shown["plan_id"], next.id);
    assert_eq!(shown["skill"]["title"], "next-plan-token");
    let explicit = caller.show(json!({
        "plan_id": caller.plan_id, "slug": CALLER_SKILL,
        "caller_tab_id": tab_id, "session_binding_nonce": CALLER_NONCE,
    })).unwrap();
    assert_eq!(explicit["plan_id"], caller.plan_id);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn current_attempt_other_plan_read_does_not_rebind_or_record_a_load() {
    let caller = LiveCaller::new();
    let other = caller.svc.plan_store.create_plan_for_team(
        "Other".into(), caller._project.path().display().to_string(),
        OrchestratorRef::from_tab("other-owner"), "other-team".into(), vec![], 8,
    ).unwrap();
    create_plan_skill(&caller.svc.plan_store, &other.id, NewPlanSkill {
        slug: CALLER_SKILL.into(), title: Some("other-plan-token".into()), area: None,
        check: None, rationale: None,
    }).unwrap();
    let before = caller.svc.plan_store.show_task(&caller.task_id).unwrap().task;
    let shown = caller.show(json!({
        "plan_id": other.id, "slug": CALLER_SKILL,
        "caller_tab_id": caller.tab_id(), "session_binding_nonce": CALLER_NONCE,
    })).unwrap();
    assert_eq!(shown["skill"]["title"], "other-plan-token");
    assert_eq!(caller.svc.plan_store.show_task(&caller.task_id).unwrap().task, before);
    assert_eq!(caller.svc.plan_store.task_binding_for_tab(&caller.tab_id()).unwrap(), (caller.plan_id.clone(), caller.task_id.clone()));
}

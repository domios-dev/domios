use super::{
    is_plan_owner_authority_method, is_recovery_method, plan_owner_may_recover,
    verify_owner_epoch_for_task_mutation, JsonRpcError, TaskAuthorityRefusal,
    PLAN_OWNER_AUTHORITY_METHODS, RECOVERY_METHODS,
};
use crate::multiplexer::state::MultiplexerState;
use crate::plan::{
    AgentRef, ExecutionIntent, OrchestratorRef, Plan, ProviderAllowance, Task, TaskLaneMetadata,
    TaskStatus,
};
use crate::service::plan::{task_attempts::BoundWorkerReport, NewTask};
use crate::service::test_fixture::ServiceFixture;
use serde_json::json;
use std::sync::Arc;

/// The three verbs the recovery list omitted, which is why `close` recovered a
/// tab-gone task live while these refused.
const TAB_GONE_RECOVERY_VERBS: &[&str] =
    &["plan.task.dispatch", "plan.task.pause", "plan.task.resume"];

const GONE_OWNER_TAB: &str = "gone-worker-tab";
const GONE_OWNER_SESSION: &str = "gone-worker-session";
const OWNER_EPOCH: u64 = 5;

struct AuthorityFixture {
    fixture: ServiceFixture,
    plan_id: String,
    task_id: String,
    owner_tab: String,
}

/// Plan owned by a live Orchestrator tab whose binding nonce it has reported
/// back — the exact live-owner triple `classify_planning_caller` authorizes.
fn authority_fixture() -> AuthorityFixture {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner_tab = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner_tab).unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&owner_tab)
        .unwrap()
        .team_id;
    {
        let tab = mux.get_tab_mut(&owner_tab).unwrap();
        tab.agent_session_id = Some("owner-session".into());
        tab.session_binding_nonce = Some("owner-nonce".into());
        tab.reported_session_binding_nonce = Some("owner-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "Recovery authority".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(owner_tab.clone()),
            session_id: Some("owner-session".into()),
            title: None,
        },
    );
    plan.team_id = Some(team_id);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Recoverable".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    AuthorityFixture {
        fixture,
        plan_id: plan.id,
        task_id,
        owner_tab,
    }
}

fn live_tab(fixture: &ServiceFixture, session_id: &str, nonce: &str) -> String {
    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("live".into(), None)
        .id;
    let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_session_id = Some(session_id.into());
    tab.session_binding_nonce = Some(nonce.into());
    tab.reported_session_binding_nonce = Some(nonce.into());
    tab_id
}

/// The in_progress + bound-owner shape reconcile flags `in_progress_tab_gone`
/// once the owning tab leaves the multiplexer.
fn bind_owner(
    fixture: &ServiceFixture,
    plan_id: &str,
    task_id: &str,
    owner_tab_id: &str,
    owner_session_id: &str,
) {
    let mut plan = fixture.service.plan_store.get(plan_id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = TaskStatus::InProgress;
    task.assigned_tab_id = Some(owner_tab_id.to_string());
    task.interruption.owner_epoch = OWNER_EPOCH;
    task.interruption.owner_tab_id = Some(owner_tab_id.to_string());
    task.interruption.owner_session_id = Some(owner_session_id.to_string());
    fixture.service.plan_store.save(&mut plan, 5).unwrap();
}

/// Move the recorded owner session without touching the live tab — what a
/// restore, compaction or provider relaunch leaves behind.
fn move_recorded_owner_session(fixture: &ServiceFixture, plan_id: &str, moved_to: &str) {
    let mut plan = fixture.service.plan_store.get(plan_id).unwrap();
    plan.owner.session_id = Some(moved_to.to_string());
    fixture.service.plan_store.save(&mut plan, 6).unwrap();
}

fn move_recorded_task_session(
    fixture: &ServiceFixture,
    plan_id: &str,
    task_id: &str,
    moved_to: &str,
) {
    let mut plan = fixture.service.plan_store.get(plan_id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.interruption.owner_session_id = Some(moved_to.to_string());
    fixture.service.plan_store.save(&mut plan, 6).unwrap();
}

fn mutation_authority(
    tab_id: &str,
    session_id: &str,
    nonce: &str,
) -> crate::service::plan::interruption::TaskMutationAuthority {
    mutation_authority_at_epoch(tab_id, session_id, nonce, OWNER_EPOCH)
}

fn mutation_authority_at_epoch(
    tab_id: &str,
    session_id: &str,
    nonce: &str,
    owner_epoch: u64,
) -> crate::service::plan::interruption::TaskMutationAuthority {
    crate::service::plan::interruption::TaskMutationAuthority {
        tab_id: tab_id.to_string(),
        session_id: session_id.to_string(),
        session_binding_nonce: nonce.to_string(),
        owner_epoch,
    }
}

fn task_record(fixture: &ServiceFixture, plan_id: &str, task_id: &str) -> Task {
    fixture
        .service
        .plan_store
        .get(plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .unwrap()
}

fn caller_params(
    tab_id: &str,
    session_id: &str,
    nonce: &str,
    plan_id: &str,
    task_id: &str,
    owner_epoch: u64,
) -> Option<serde_json::Value> {
    Some(json!({
        "plan_id": plan_id,
        "task_id": task_id,
        "caller_tab_id": tab_id,
        "session_id": session_id,
        "session_binding_nonce": nonce,
        "owner_epoch": owner_epoch,
    }))
}

fn refusal_type(error: &JsonRpcError) -> String {
    error
        .data
        .as_ref()
        .and_then(|data| data.get("type"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[test]
fn recovery_authority_recovery_verbs_are_a_subset_of_plan_owner_authority() {
    for method in RECOVERY_METHODS {
        assert!(
            is_plan_owner_authority_method(method),
            "{method} recovers but is not a Plan-owner authority verb"
        );
    }
    for method in TAB_GONE_RECOVERY_VERBS {
        assert!(is_recovery_method(method), "{method} must be recoverable");
    }
    assert!(!PLAN_OWNER_AUTHORITY_METHODS.contains(&"agent.report"));
    assert!(!RECOVERY_METHODS.contains(&"agent.report"));
}

/// Mechanism 1: the tab-gone escape hatch fires for dispatch AND pause AND
/// resume, not only for the four verbs the list happened to name.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn recovery_authority_recovers_a_tab_gone_task_for_dispatch_pause_and_resume() {
    for method in TAB_GONE_RECOVERY_VERBS {
        let harness = authority_fixture();
        bind_owner(
            &harness.fixture,
            &harness.plan_id,
            &harness.task_id,
            GONE_OWNER_TAB,
            GONE_OWNER_SESSION,
        );
        let service = Arc::new(harness.fixture.service.clone());
        let params = caller_params(
            &harness.owner_tab,
            "owner-session",
            "owner-nonce",
            &harness.plan_id,
            &harness.task_id,
            OWNER_EPOCH,
        );

        assert!(
            plan_owner_may_recover(
                method,
                &params,
                &service,
                &harness.plan_id,
                Some(GONE_OWNER_TAB),
                Some(GONE_OWNER_SESSION),
            ),
            "{method} must reach the tab-gone escape hatch"
        );
        verify_owner_epoch_for_task_mutation(method, &params, &service)
            .unwrap_or_else(|error| panic!("{method} refused recovery: {}", error.message));

        let after = task_record(&harness.fixture, &harness.plan_id, &harness.task_id);
        assert!(after.interruption.owner_tab_id.is_none(), "{method}");
        assert!(after.interruption.owner_session_id.is_none(), "{method}");
        assert!(after.interruption.owner_epoch > OWNER_EPOCH, "{method}");
        assert!(after.assigned_tab_id.is_none(), "{method}");
    }
}

/// Mechanism 2 (negative control): the same verbs, the same tab-gone task, a
/// caller that is not the Plan owner — refused, and nothing is retired.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn recovery_authority_still_refuses_a_foreign_caller_for_the_same_verbs() {
    for method in TAB_GONE_RECOVERY_VERBS {
        let harness = authority_fixture();
        bind_owner(
            &harness.fixture,
            &harness.plan_id,
            &harness.task_id,
            GONE_OWNER_TAB,
            GONE_OWNER_SESSION,
        );
        let foreign_tab = live_tab(&harness.fixture, "foreign-session", "foreign-nonce");
        let service = Arc::new(harness.fixture.service.clone());
        let params = caller_params(
            &foreign_tab,
            "foreign-session",
            "foreign-nonce",
            &harness.plan_id,
            &harness.task_id,
            OWNER_EPOCH,
        );

        assert!(
            !plan_owner_may_recover(
                method,
                &params,
                &service,
                &harness.plan_id,
                Some(GONE_OWNER_TAB),
                Some(GONE_OWNER_SESSION),
            ),
            "{method} handed recovery to a foreign caller"
        );
        let error = verify_owner_epoch_for_task_mutation(method, &params, &service)
            .expect_err("a foreign caller must not recover a tab-gone task");
        assert_eq!(
            refusal_type(&error),
            TaskAuthorityRefusal::CallerNotTaskOwner.discriminant(),
            "{method}"
        );

        let after = task_record(&harness.fixture, &harness.plan_id, &harness.task_id);
        assert_eq!(after.interruption.owner_epoch, OWNER_EPOCH, "{method}");
        assert_eq!(
            after.interruption.owner_tab_id.as_deref(),
            Some(GONE_OWNER_TAB),
            "{method}"
        );
    }
}

/// Mechanism 3: every refusal carries the discriminant of the state that
/// actually blocked it, so a tab-gone task never reports a stale epoch.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn recovery_authority_refusals_name_the_real_condition_by_discriminant() {
    let harness = authority_fixture();
    let live_worker = live_tab(&harness.fixture, "worker-session", "worker-nonce");
    bind_owner(
        &harness.fixture,
        &harness.plan_id,
        &harness.task_id,
        &live_worker,
        "worker-session",
    );
    let service = Arc::new(harness.fixture.service.clone());

    let wrong_epoch = caller_params(
        &harness.owner_tab,
        "owner-session",
        "owner-nonce",
        &harness.plan_id,
        &harness.task_id,
        OWNER_EPOCH + 1,
    );
    let error = verify_owner_epoch_for_task_mutation("plan.task.update", &wrong_epoch, &service)
        .expect_err("a mismatched epoch must be refused");
    assert_eq!(
        refusal_type(&error),
        TaskAuthorityRefusal::OwnerEpochStale.discriminant()
    );

    let unreported_nonce = caller_params(
        &harness.owner_tab,
        "owner-session",
        "never-reported-nonce",
        &harness.plan_id,
        &harness.task_id,
        OWNER_EPOCH,
    );
    let error =
        verify_owner_epoch_for_task_mutation("plan.task.update", &unreported_nonce, &service)
            .expect_err("an unproven binding must be refused");
    assert_eq!(
        refusal_type(&error),
        TaskAuthorityRefusal::CallerBindingStale.discriminant()
    );
}

/// Mechanism 3, the case the live Orchestrator hit: owner gone, caller IS the
/// Plan owner, verb outside the recovery list — named as such, not as an epoch.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn recovery_authority_names_verb_ineligibility_on_a_tab_gone_task() {
    let harness = authority_fixture();
    bind_owner(
        &harness.fixture,
        &harness.plan_id,
        &harness.task_id,
        GONE_OWNER_TAB,
        GONE_OWNER_SESSION,
    );
    let service = Arc::new(harness.fixture.service.clone());
    let params = caller_params(
        &harness.owner_tab,
        "owner-session",
        "owner-nonce",
        &harness.plan_id,
        &harness.task_id,
        OWNER_EPOCH,
    );

    let error = verify_owner_epoch_for_task_mutation("agent.report", &params, &service)
        .expect_err("a non-recovery verb must not ride the escape hatch");
    assert_eq!(
        refusal_type(&error),
        TaskAuthorityRefusal::OwnerGoneVerbNotRecoverable.discriminant()
    );
}

/// Mechanism 4: the Plan owner is recognised from the LIVE tab, so a session
/// that moved without an agent restart no longer locks the owner out; a foreign
/// tab and an unproven binding stay refused against the same moved record.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn recovery_authority_admits_a_moved_owner_session_but_not_a_foreign_tab() {
    let harness = authority_fixture();
    let live_worker = live_tab(&harness.fixture, "worker-session", "worker-nonce");
    bind_owner(
        &harness.fixture,
        &harness.plan_id,
        &harness.task_id,
        &live_worker,
        "worker-session",
    );
    move_recorded_owner_session(
        &harness.fixture,
        &harness.plan_id,
        "session-before-the-move",
    );
    let service = Arc::new(harness.fixture.service.clone());

    let owner = caller_params(
        &harness.owner_tab,
        "owner-session",
        "owner-nonce",
        &harness.plan_id,
        &harness.task_id,
        OWNER_EPOCH,
    );
    verify_owner_epoch_for_task_mutation("plan.task.update", &owner, &service)
        .expect("the live Plan owner must not be locked out by a moved owner record");

    let foreign_tab = live_tab(&harness.fixture, "foreign-session", "foreign-nonce");
    let foreign = caller_params(
        &foreign_tab,
        "foreign-session",
        "foreign-nonce",
        &harness.plan_id,
        &harness.task_id,
        OWNER_EPOCH,
    );
    let error = verify_owner_epoch_for_task_mutation("plan.task.update", &foreign, &service)
        .expect_err("a foreign tab is not the Plan owner however the record moved");
    assert_eq!(
        refusal_type(&error),
        TaskAuthorityRefusal::CallerNotTaskOwner.discriminant()
    );

    let after = task_record(&harness.fixture, &harness.plan_id, &harness.task_id);
    assert_eq!(after.interruption.owner_epoch, OWNER_EPOCH);
    assert_eq!(
        after.interruption.owner_tab_id.as_deref(),
        Some(live_worker.as_str())
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn task_authority_live_binding_orchestrator_can_close_after_session_move() {
    let harness = authority_fixture();
    let worker = live_tab(&harness.fixture, "worker-session", "worker-nonce");
    bind_owner(
        &harness.fixture,
        &harness.plan_id,
        &harness.task_id,
        &worker,
        "worker-session",
    );
    move_recorded_owner_session(
        &harness.fixture,
        &harness.plan_id,
        "session-before-the-move",
    );
    let service = Arc::new(harness.fixture.service.clone());
    let params = caller_params(
        &harness.owner_tab,
        "owner-session",
        "owner-nonce",
        &harness.plan_id,
        &harness.task_id,
        OWNER_EPOCH,
    );

    verify_owner_epoch_for_task_mutation("plan.task.close", &params, &service).unwrap();
    harness
        .fixture
        .service
        .plan_store
        .close_task_authorized(
            &harness.plan_id,
            &harness.task_id,
            None,
            AgentRef::from_tab(&harness.owner_tab),
            &mutation_authority(&harness.owner_tab, "owner-session", "owner-nonce"),
            7,
        )
        .unwrap();
    assert_eq!(
        task_record(&harness.fixture, &harness.plan_id, &harness.task_id).status,
        TaskStatus::Done
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn task_authority_live_binding_worker_can_close_after_session_move() {
    let harness = authority_fixture();
    let worker = live_tab(&harness.fixture, "worker-session", "worker-nonce");
    bind_owner(
        &harness.fixture,
        &harness.plan_id,
        &harness.task_id,
        &worker,
        "worker-session",
    );
    move_recorded_task_session(
        &harness.fixture,
        &harness.plan_id,
        &harness.task_id,
        "session-before-the-move",
    );
    let service = Arc::new(harness.fixture.service.clone());
    let params = caller_params(
        &worker,
        "worker-session",
        "worker-nonce",
        &harness.plan_id,
        &harness.task_id,
        OWNER_EPOCH,
    );

    verify_owner_epoch_for_task_mutation("plan.task.close", &params, &service).unwrap();
    harness
        .fixture
        .service
        .plan_store
        .close_task_authorized(
            &harness.plan_id,
            &harness.task_id,
            None,
            AgentRef::from_tab(&worker),
            &mutation_authority(&worker, "worker-session", "worker-nonce"),
            7,
        )
        .unwrap();
    assert_eq!(
        task_record(&harness.fixture, &harness.plan_id, &harness.task_id).status,
        TaskStatus::Done
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn task_authority_live_binding_stale_tab_is_refused_without_mutation() {
    let harness = authority_fixture();
    let worker = live_tab(&harness.fixture, "worker-session", "worker-nonce");
    bind_owner(
        &harness.fixture,
        &harness.plan_id,
        &harness.task_id,
        &worker,
        "worker-session",
    );
    let stale = live_tab(&harness.fixture, "stale-session", "stale-nonce");
    let service = Arc::new(harness.fixture.service.clone());
    let params = caller_params(
        &stale,
        "stale-session",
        "stale-nonce",
        &harness.plan_id,
        &harness.task_id,
        OWNER_EPOCH,
    );

    let error = verify_owner_epoch_for_task_mutation("plan.task.close", &params, &service)
        .expect_err("a different live tab must not mutate this task");
    assert_eq!(
        refusal_type(&error),
        TaskAuthorityRefusal::CallerNotTaskOwner.discriminant()
    );
    assert_eq!(
        task_record(&harness.fixture, &harness.plan_id, &harness.task_id).status,
        TaskStatus::InProgress
    );
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn task_authority_live_binding_refusal_names_both_compared_identities() {
    let harness = authority_fixture();
    let worker = live_tab(&harness.fixture, "worker-session", "worker-nonce");
    bind_owner(
        &harness.fixture,
        &harness.plan_id,
        &harness.task_id,
        &worker,
        "worker-session",
    );
    let error = harness
        .fixture
        .service
        .plan_store
        .close_task_authorized(
            &harness.plan_id,
            &harness.task_id,
            None,
            AgentRef::from_tab("stale-tab"),
            &mutation_authority("stale-tab", "stale-session", "stale-nonce"),
            7,
        )
        .expect_err("a different tab must stay refused");
    let message = error.to_string();
    for id in [
        "stale-tab",
        "stale-session",
        worker.as_str(),
        "worker-session",
    ] {
        assert!(message.contains(id), "missing {id} from {message}");
    }
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn task_authority_live_binding_plan_owner_methods_converge_after_session_move() {
    let harness = authority_fixture();
    let worker = live_tab(&harness.fixture, "worker-session", "worker-nonce");
    bind_owner(
        &harness.fixture,
        &harness.plan_id,
        &harness.task_id,
        &worker,
        "worker-session",
    );
    move_recorded_owner_session(
        &harness.fixture,
        &harness.plan_id,
        "session-before-the-move",
    );
    let service = Arc::new(harness.fixture.service.clone());
    let params = caller_params(
        &harness.owner_tab,
        "owner-session",
        "owner-nonce",
        &harness.plan_id,
        &harness.task_id,
        OWNER_EPOCH,
    );

    for method in ["plan.task.assign", "plan.task.close", "plan.task.update"] {
        verify_owner_epoch_for_task_mutation(method, &params, &service)
            .unwrap_or_else(|error| panic!("{method} refused live owner: {}", error.message));
    }
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn task_authority_live_binding_report_path_accepts_a_moved_worker_session() {
    let harness = authority_fixture();
    let worker = live_tab(&harness.fixture, "worker-session", "worker-nonce");
    let attempt_id = harness
        .fixture
        .service
        .plan_store
        .begin_task_dispatch(
            &harness.plan_id,
            &harness.task_id,
            &worker,
            "Worker",
            TaskLaneMetadata::default(),
            3,
        )
        .unwrap();
    harness
        .fixture
        .service
        .plan_store
        .record_dispatch_policy(
            &harness.plan_id,
            &harness.task_id,
            &attempt_id,
            ExecutionIntent::Mutating,
            Some(ProviderAllowance {
                remaining_pct: 90.0,
                observed_at: 3,
                stale_after_ms: 100,
                reserve_pct: 10.0,
            }),
            3,
        )
        .unwrap();
    harness
        .fixture
        .service
        .plan_store
        .finish_task_dispatch(
            &harness.plan_id,
            &harness.task_id,
            &worker,
            &attempt_id,
            "session-before-the-move",
            "nonce-before-the-move",
            TaskLaneMetadata::default(),
            4,
        )
        .unwrap();

    harness
        .fixture
        .service
        .plan_store
        .ingest_bound_worker_report(
            &harness.plan_id,
            &harness.task_id,
            &AgentRef::from_tab(&worker),
            BoundWorkerReport {
                message: "still writing".into(),
                authority: Some(mutation_authority_at_epoch(
                    &worker,
                    "worker-session",
                    "worker-nonce",
                    1,
                )),
                ..Default::default()
            },
            7,
        )
        .expect("a live worker must report after its session moves");
    let task = task_record(&harness.fixture, &harness.plan_id, &harness.task_id);
    assert!(task
        .milestones
        .iter()
        .any(|milestone| milestone.text.contains("still writing")));
}

/// Mechanism 5: the widened hatch is not a laundering path. A verb that asserts
/// the CALLER's own task binding gets neither recovery nor the Plan-owner
/// shortcut, so a stale binding transitions nothing on either owner state.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn recovery_authority_does_not_launder_a_stale_binding_report() {
    for (owner_tab, owner_session, expected) in [
        (
            GONE_OWNER_TAB.to_string(),
            GONE_OWNER_SESSION.to_string(),
            TaskAuthorityRefusal::OwnerGoneVerbNotRecoverable,
        ),
        (
            String::new(),
            "worker-session".to_string(),
            TaskAuthorityRefusal::CallerNotTaskOwner,
        ),
    ] {
        let harness = authority_fixture();
        let owner_tab = if owner_tab.is_empty() {
            live_tab(&harness.fixture, &owner_session, "worker-nonce")
        } else {
            owner_tab
        };
        bind_owner(
            &harness.fixture,
            &harness.plan_id,
            &harness.task_id,
            &owner_tab,
            &owner_session,
        );
        let service = Arc::new(harness.fixture.service.clone());
        let params = caller_params(
            &harness.owner_tab,
            "owner-session",
            "owner-nonce",
            &harness.plan_id,
            &harness.task_id,
            OWNER_EPOCH,
        );

        assert!(!is_recovery_method("agent.report"));
        assert!(!is_plan_owner_authority_method("agent.report"));
        assert!(!plan_owner_may_recover(
            "agent.report",
            &params,
            &service,
            &harness.plan_id,
            Some(owner_tab.as_str()),
            Some(owner_session.as_str()),
        ));
        let error = verify_owner_epoch_for_task_mutation("agent.report", &params, &service)
            .expect_err("a caller not bound to the task must not report against it");
        assert_eq!(refusal_type(&error), expected.discriminant());

        let after = task_record(&harness.fixture, &harness.plan_id, &harness.task_id);
        assert_eq!(after.interruption.owner_epoch, OWNER_EPOCH);
        assert_eq!(after.status, TaskStatus::InProgress);
        assert_eq!(
            after.interruption.owner_tab_id.as_deref(),
            Some(&*owner_tab)
        );
    }
}

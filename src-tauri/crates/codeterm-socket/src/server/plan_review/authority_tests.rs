use serde_json::json;

use codeterm_sentinel_host::SentinelExt;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::multiplexer::types::AgentRole;
use codeterm_service_core::plan::{OrchestratorRef, Plan};
use codeterm_service_core::service::test_fixture::ServiceFixture;

use super::{authorized_orchestrator_actor, tests::socket_bounce_fixture};

const NONCE: &str = "owner-nonce";
const LIVE_SESSION: &str = "live-session";

/// `stored_owner_session` is what the plan recorded when it was created; the tab's
/// live session is always LIVE_SESSION, so `None`/rotated values model a restart.
fn fixture(
    stored_owner_session: Option<&str>,
    team_bound: bool,
) -> (ServiceFixture, String, String) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner).unwrap();
    let team_id = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    {
        let tab = mux.get_tab_mut(&owner).unwrap();
        tab.agent_session_id = Some(LIVE_SESSION.into());
        tab.session_binding_nonce = Some(NONCE.into());
        tab.reported_session_binding_nonce = Some(NONCE.into());
        tab.team_id = team_bound.then(|| team_id.clone());
    }
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "review authority".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(owner.clone()),
            session_id: stored_owner_session.map(str::to_string),
            title: None,
        },
    );
    plan.team_id = team_bound.then_some(team_id);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    (fixture, plan.id, owner)
}

fn caller(tab_id: &str, session_id: &str, nonce: &str) -> serde_json::Value {
    json!({
        "caller_tab_id": tab_id,
        "session_id": session_id,
        "session_binding_nonce": nonce,
    })
}

/// The reproduction: owner tab and nonce match, the tab is a live Team-bound
/// Orchestrator, but the plan's STORED owner session is stale or absent.
#[test]
fn live_orchestrator_with_rotated_owner_session_is_authorized() {
    for stored in [None, Some("rotated-away")] {
        let (fixture, plan_id, owner) = fixture(stored, true);
        let params = caller(&owner, LIVE_SESSION, NONCE);

        let actor = authorized_orchestrator_actor(&fixture.service, &plan_id, Some(&params))
            .expect("a live bound Plan Orchestrator must keep review authority");

        assert_eq!(actor.tab_id.as_deref(), Some(owner.as_str()));
        assert_eq!(actor.session_id.as_deref(), Some(LIVE_SESSION));
    }
}

#[test]
fn a_matching_stored_owner_session_is_still_authorized() {
    let (fixture, plan_id, owner) = fixture(Some(LIVE_SESSION), true);
    let params = caller(&owner, LIVE_SESSION, NONCE);

    assert!(authorized_orchestrator_actor(&fixture.service, &plan_id, Some(&params)).is_ok());
}

/// An omitted session id is resolved from the live tab, never guessed.
#[test]
fn an_omitted_session_id_resolves_from_the_live_tab() {
    let (fixture, plan_id, owner) = fixture(None, true);
    let params = json!({
        "caller_tab_id": owner,
        "session_binding_nonce": NONCE,
    });

    let actor = authorized_orchestrator_actor(&fixture.service, &plan_id, Some(&params)).unwrap();

    assert_eq!(actor.session_id.as_deref(), Some(LIVE_SESSION));
}

#[test]
fn wrong_tab_wrong_nonce_and_wrong_live_session_are_all_refused() {
    let (fixture, plan_id, owner) = fixture(Some(LIVE_SESSION), true);
    for params in [
        caller("not-the-owner", LIVE_SESSION, NONCE),
        caller(&owner, LIVE_SESSION, "forged-nonce"),
        caller(&owner, "not-the-live-session", NONCE),
        caller("", LIVE_SESSION, NONCE),
        caller(&owner, LIVE_SESSION, ""),
    ] {
        let error = authorized_orchestrator_actor(&fixture.service, &plan_id, Some(&params))
            .expect_err("only the live bound Plan Orchestrator may act");
        assert_eq!(error.code, -32003);
    }
}

#[test]
fn a_non_team_orchestrator_is_refused() {
    let (fixture, plan_id, owner) = fixture(None, false);
    let params = caller(&owner, LIVE_SESSION, NONCE);

    let error = authorized_orchestrator_actor(&fixture.service, &plan_id, Some(&params))
        .expect_err("a plan is Team-bound; a team-less Orchestrator has no authority");

    assert_eq!(error.code, -32003);
}

#[test]
fn a_worker_and_a_draft_planner_are_refused() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner).unwrap();
    let team_id = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    let planner = mux.create_tab(&workspace).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    for (tab_id, session) in [
        (&owner, LIVE_SESSION),
        (&planner, "planner-session"),
        (&worker, "worker-session"),
    ] {
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.agent_session_id = Some(session.into());
        tab.session_binding_nonce = Some(NONCE.into());
        tab.reported_session_binding_nonce = Some(NONCE.into());
        tab.team_id = Some(team_id.clone());
    }
    {
        let tab = mux.get_tab_mut(&planner).unwrap();
        tab.role = Some(AgentRole::Manager);
        tab.role_profile = Some("planner".into());
        tab.parent_tab_id = Some(owner.clone());
    }
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "review authority".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(owner.clone()),
            session_id: None,
            title: None,
        },
    );
    plan.team_id = Some(team_id);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();

    for (tab_id, session) in [(&planner, "planner-session"), (&worker, "worker-session")] {
        let params = caller(tab_id, session, NONCE);
        let error = authorized_orchestrator_actor(&fixture.service, &plan.id, Some(&params))
            .expect_err("only the Plan Orchestrator grants review capacity");
        assert_eq!(error.code, -32003);
    }
    let params = caller(&owner, LIVE_SESSION, NONCE);
    assert!(authorized_orchestrator_actor(&fixture.service, &plan.id, Some(&params)).is_ok());
}

#[test]
fn acceptance_removes_only_the_assigned_workers_completion_watches() {
    let (fixture, plan_id, owner) = fixture(Some(LIVE_SESSION), true);
    let workspace = fixture
        .service
        .mux
        .read()
        .find_workspace_for_tab(&owner)
        .unwrap();
    let team = fixture
        .service
        .mux
        .read()
        .get_tab(&owner)
        .unwrap()
        .team_id
        .clone();
    let worker = {
        let mut mux = fixture.service.mux.write();
        let worker = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&worker).unwrap();
        tab.team_id = team;
        tab.cwd = Some(fixture.temp.path().display().to_string());
        mux.get_tab_mut(&owner).unwrap().cwd = Some(fixture.temp.path().display().to_string());
        worker
    };
    let handle = codeterm_sentinel_host::scheduler::fixture_handle_with_ui(
        codeterm_sentinel_host::store::Store::new(
            fixture.temp.path().join("sentinels.json"),
            codeterm_service_core::cron::clock::MockClock::starting_at(1000),
        ),
        fixture.service.ui.clone(),
    );
    codeterm_sentinel_host::manage_fixture_ports(&fixture.service, &handle);
    let creator = fixture.service.sentinel_caller(&worker, None).unwrap();
    let create = |until: Option<&str>| {
        json!({"schema_version":2,"kind":"watch","name":if until.is_some(){"completion-watch"}else{"recurring-watch"},"steps":[
            {"id":"source","uses":"probe","with":{"command":{"lit":"status"}},"every":{"lit":"10s"},"timeout":{"lit":"1h"}},
            {"id":"decide","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":["done"],"matches":{"done":{"lit":"done"}},"until":until,"routes":{"done":"notice"}},
            {"id":"notice","uses":"notify"}
        ]})
    };
    use codeterm_sentinel_host::BlueprintApplyExt;
    let applied = fixture
        .service
        .blueprint_apply(&creator, create(Some("done")), false)
        .unwrap();
    let completion = fixture
        .service
        .sentinel_show(&creator, applied["id"].as_str().unwrap())
        .unwrap();
    let applied = fixture
        .service
        .blueprint_apply(&creator, create(None), false)
        .unwrap();
    let recurring = fixture
        .service
        .sentinel_show(&creator, applied["id"].as_str().unwrap())
        .unwrap();
    let owner_caller = fixture
        .service
        .sentinel_caller(&owner, Some(NONCE))
        .unwrap();
    assert_eq!(
        fixture.service.sentinel_list(&owner_caller).unwrap().len(),
        2
    );
    let store = &fixture.service.plan_store;
    let task_id = store
        .add_task(
            &plan_id,
            codeterm_service_core::service::plan::NewTask {
                name: "watch result".into(),
                acceptance_criteria: vec!["complete".into()],
                test_plan: Some("verify persisted result".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan_id, &task_id, &worker, None, 3)
        .unwrap();
    let mut plan = store.get(&plan_id).unwrap();
    plan.tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap()
        .status = codeterm_service_core::plan::TaskStatus::InReview;
    store.save(&mut plan, 4).unwrap();
    let params = Some(
        json!({"plan_id":plan_id,"task_id":task_id,"note":"verified",
        "caller_tab_id":owner,"session_id":LIVE_SESSION,"session_binding_nonce":NONCE}),
    );
    let svc = std::sync::Arc::new(fixture.service.clone());
    super::dispatch("plan.review.accept", &params, &svc).unwrap();
    assert_eq!(
        store.show_task(&task_id).unwrap().task.status,
        codeterm_service_core::plan::TaskStatus::Done
    );
    assert!(handle.store.lock().get(&completion.id).is_none());
    assert!(handle.store.lock().get(&recurring.id).is_some());
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.temp.path().join("sentinels.json")).unwrap())
            .unwrap();
    assert_eq!(persisted.as_array().unwrap().len(), 1);
    assert_eq!(persisted[0]["id"], recurring.id);
}

fn add_in_review_task(
    fixture: &ServiceFixture,
    plan_id: &str,
    worker_id: &str,
    name: &str,
) -> String {
    let store = &fixture.service.plan_store;
    let task_id = store
        .add_task(
            plan_id,
            codeterm_service_core::service::plan::NewTask {
                name: name.into(),
                acceptance_criteria: vec!["verified".into()],
                test_plan: Some("observe the far-end result".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(plan_id, &task_id, worker_id, None, 3)
        .unwrap();
    let mut plan = store.get(plan_id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = codeterm_service_core::plan::TaskStatus::InReview;
    let attempt = task.execution_attempts.last_mut().unwrap();
    attempt.session_id = Some(format!("{worker_id}-session"));
    attempt.acknowledged_at = Some(3);
    attempt.reported_at = Some(4);
    attempt.report = Some(Default::default());
    store.save(&mut plan, 4).unwrap();
    task_id
}

fn qa_child(fixture: &ServiceFixture, owner_tab_id: &str) -> String {
    let (workspace, team) = {
        let mux = fixture.service.mux.read();
        let workspace = mux.find_workspace_for_tab(owner_tab_id).unwrap();
        let team = mux.get_tab(owner_tab_id).unwrap().team_id.clone();
        (workspace, team)
    };
    let mut mux = fixture.service.mux.write();
    let tab_id = mux.create_tab(&workspace).unwrap();
    let qa = mux.get_tab_mut(&tab_id).unwrap();
    qa.role = Some(AgentRole::Worker);
    qa.role_profile = Some("qa".into());
    qa.parent_tab_id = Some(owner_tab_id.into());
    qa.report_target_tab_id = Some(owner_tab_id.into());
    qa.team_id = team;
    qa.agent_session_id = Some("qa-session".into());
    qa.session_binding_nonce = Some("qa-nonce".into());
    qa.reported_session_binding_nonce = Some("qa-nonce".into());
    tab_id
}

fn qa_caller(tab_id: &str) -> serde_json::Value {
    json!({
        "caller_tab_id": tab_id,
        "session_id": "qa-session",
        "session_binding_nonce": "qa-nonce",
        "from": { "tab_id": tab_id, "session_id": "qa-session" },
    })
}

#[test]
fn parent_can_hand_one_in_review_task_to_a_direct_qa_child() {
    let (fixture, plan_id, owner) = fixture(None, true);
    let qa = qa_child(&fixture, &owner);
    let task_id = add_in_review_task(&fixture, &plan_id, "author", "delegated task");
    let mut params = caller(&owner, LIVE_SESSION, NONCE);
    params["plan_id"] = json!(plan_id);
    params["task_id"] = json!(task_id);
    params["qa_tab_id"] = json!(qa);

    let result = super::dispatch(
        "plan.review.delegate",
        &Some(params),
        &std::sync::Arc::new(fixture.service.clone()),
    )
    .expect("Plan owner may hand off a review task");

    assert_eq!(result["delegated"], true);
    let mux = fixture.service.mux.read();
    let qa_tab = mux.get_tab(&qa).unwrap();
    assert_eq!(
        qa_tab.taskless_authorized_review_tasks,
        vec![format!("{plan_id}/{task_id}")]
    );
}

#[test]
fn qa_can_accept_only_a_handed_task_with_an_evidence_note() {
    let (fixture, plan_id, owner) = fixture(None, true);
    let qa = qa_child(&fixture, &owner);
    let delegated = add_in_review_task(&fixture, &plan_id, "author-one", "delegated task");
    let other = add_in_review_task(&fixture, &plan_id, "author-two", "other task");
    let svc = std::sync::Arc::new(fixture.service.clone());
    let mut handoff = caller(&owner, LIVE_SESSION, NONCE);
    handoff["plan_id"] = json!(plan_id);
    handoff["task_id"] = json!(delegated);
    handoff["qa_tab_id"] = json!(qa);
    super::dispatch("plan.review.delegate", &Some(handoff), &svc).unwrap();

    let mut unauthorized = qa_caller(&qa);
    unauthorized["plan_id"] = json!(plan_id);
    unauthorized["task_id"] = json!(other);
    unauthorized["note"] = json!("I have no handoff for this task");
    let error = super::dispatch("plan.review.accept", &Some(unauthorized), &svc)
        .expect_err("a QA cannot accept a task it was not handed");
    assert_eq!(error.code, -32003);
    assert_eq!(
        fixture
            .service
            .plan_store
            .show_task(&other)
            .unwrap()
            .task
            .status,
        codeterm_service_core::plan::TaskStatus::InReview
    );

    let mut missing_note = qa_caller(&qa);
    missing_note["plan_id"] = json!(plan_id);
    missing_note["task_id"] = json!(delegated);
    let error = super::dispatch("plan.review.accept", &Some(missing_note), &svc)
        .expect_err("a QA verdict must carry evidence");
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|data| data.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("qa_note_required")
    );

    let mut accept = qa_caller(&qa);
    accept["plan_id"] = json!(plan_id);
    accept["task_id"] = json!(delegated);
    accept["note"] = json!("PASS: the observable result and recorded row match the criteria");
    super::dispatch("plan.review.accept", &Some(accept), &svc)
        .expect("a handed task with evidence may be accepted");

    let accepted = fixture
        .service
        .plan_store
        .show_task(&delegated)
        .unwrap()
        .task;
    assert_eq!(
        accepted.status,
        codeterm_service_core::plan::TaskStatus::Done
    );
    assert_eq!(
        accepted.review_evidence.last().unwrap().note,
        "PASS: the observable result and recorded row match the criteria"
    );
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&qa)
        .unwrap()
        .taskless_authorized_review_tasks
        .is_empty());
}

#[test]
fn qa_cannot_hold_a_handed_review_or_accept_without_its_live_binding() {
    let (fixture, plan_id, owner) = fixture(None, true);
    let qa = qa_child(&fixture, &owner);
    let task_id = add_in_review_task(&fixture, &plan_id, "author", "delegated task");
    let svc = std::sync::Arc::new(fixture.service.clone());
    let mut handoff = caller(&owner, LIVE_SESSION, NONCE);
    handoff["plan_id"] = json!(plan_id);
    handoff["task_id"] = json!(task_id);
    handoff["qa_tab_id"] = json!(qa);
    super::dispatch("plan.review.delegate", &Some(handoff), &svc).unwrap();

    let mut hold = qa_caller(&qa);
    hold["plan_id"] = json!(plan_id);
    hold["task_id"] = json!(task_id);
    hold["note"] = json!("hold");
    assert_eq!(
        super::dispatch("plan.review.hold", &Some(hold), &svc)
            .expect_err("qa has no hold authority")
            .code,
        -32003
    );

    let mut forged = qa_caller(&qa);
    forged["plan_id"] = json!(plan_id);
    forged["task_id"] = json!(task_id);
    forged["note"] = json!("looks verified");
    forged["session_binding_nonce"] = json!("foreign-nonce");
    assert_eq!(
        super::dispatch("plan.review.accept", &Some(forged), &svc)
            .expect_err("a stale QA binding is refused")
            .code,
        -32003
    );
    assert_eq!(
        fixture
            .service
            .plan_store
            .show_task(&task_id)
            .unwrap()
            .task
            .status,
        codeterm_service_core::plan::TaskStatus::InReview
    );
}

#[test]
fn qa_bounce_accepts_nonce_only_cli_identity_and_refuses_an_unhanded_tab() {
    let (fixture, plan_id, task_id, _) = socket_bounce_fixture(false);
    let owner = fixture
        .service
        .plan_store
        .get(&plan_id)
        .and_then(|plan| plan.owner.tab_id)
        .unwrap();
    let (owner_session, owner_nonce) = {
        let mux = fixture.service.mux.read();
        let owner = mux.get_tab(&owner).unwrap();
        (
            owner.agent_session_id.clone().unwrap(),
            owner.session_binding_nonce.clone().unwrap(),
        )
    };
    let qa = qa_child(&fixture, &owner);
    let svc = std::sync::Arc::new(fixture.service.clone());
    let mut handoff = caller(&owner, &owner_session, &owner_nonce);
    handoff["plan_id"] = json!(plan_id);
    handoff["task_id"] = json!(task_id);
    handoff["qa_tab_id"] = json!(qa);
    super::dispatch("plan.review.delegate", &Some(handoff), &svc).unwrap();

    let unhanded_qa = qa_child(&fixture, &owner);
    let mut unhanded_bounce = qa_caller(&unhanded_qa);
    unhanded_bounce["plan_id"] = json!(plan_id);
    unhanded_bounce["task_id"] = json!(task_id);
    unhanded_bounce["note"] = json!("An unhanded tab must be rejected");
    let error = super::dispatch("plan.review.bounce", &Some(unhanded_bounce), &svc)
        .expect_err("only the handed QA tab may bounce the task");
    assert_eq!(error.code, -32003);

    let mut bounce = qa_caller(&qa);
    bounce.as_object_mut().unwrap().remove("session_id");
    bounce["plan_id"] = json!(plan_id);
    bounce["task_id"] = json!(task_id);
    bounce["note"] = json!("FAIL: expected persisted row was absent");
    super::dispatch("plan.review.bounce", &Some(bounce.clone()), &svc)
        .expect("a handed failing task can be bounced");
    assert_eq!(
        fixture
            .service
            .plan_store
            .show_task(&task_id)
            .unwrap()
            .task
            .status,
        codeterm_service_core::plan::TaskStatus::Pending
    );
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&qa)
        .unwrap()
        .taskless_authorized_review_tasks
        .is_empty());
    assert!(super::dispatch("plan.review.bounce", &Some(bounce), &svc).is_err());
}

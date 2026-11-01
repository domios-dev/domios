use super::*;
use codeterm_service_core::cron::store::{
    commit, CronChange, CronRunInfo, CronRunStatus, Prepared,
};
use codeterm_service_core::plan::{CuratorRun, OrchestratorRef, Plan};
use codeterm_service_core::service::role_policy;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use codeterm_service_core::service::AuthenticatedPlanCaller;

const PROVIDER: &str = "curator-hook-fixture";
const SESSION: &str = "curator-own-session";
const NONCE: &str = "curator-own-nonce";

async fn journey(finish: bool, deadline: bool) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.role = Some(codeterm_service_core::multiplexer::types::AgentRole::Manager);
    tab.role_profile = Some("curator".into());
    tab.agent_type = Some(PROVIDER.into());
    tab.session_binding_nonce = Some(NONCE.into());
    tab.pty_id = Some("fixture-pty".into());
    let fixture = ServiceFixture::new(mux);
    let storage = crate::cron_routes::off_runtime_gate::StorageCron::open();
    let mut state = crate::test_support::app_state(&fixture);
    state.cron = storage.cron.clone();
    state.cron_wake = storage.wake.clone();
    assert!(fixture.service.set_ext(state.cron.clone()).is_ok());
    assert!(fixture.service.set_ext(state.cron_wake.clone()).is_ok());
    fixture.service.start_curator_events().unwrap();
    std::fs::write(fixture.temp.path().join(format!("{SESSION}.jsonl")), "").unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(HookProvider {
            id: PROVIDER,
            sessions: [SESSION].into_iter().collect(),
            materialization_root: Some(fixture.temp.path().to_owned()),
        }));
    let now = fixture.service.curator_now().unwrap();
    let mut plan = Plan::new(
        "Hook journey".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.team_id = Some("fixture-team".into());
    plan.control_loop.running = Some(CuratorRun {
        number: 1,
        started_at: now,
        ..Default::default()
    });
    fixture.service.plan_store.save(&mut plan, now).unwrap();
    let mut job = storage.create("Curator hook journey", &workspace);
    job.role = Some("curator".into());
    job.task = plan.id.clone();
    commit(&state.cron, |_| {
        Ok(Prepared::new(CronChange::Patch(job.clone()), ()))
    })
    .unwrap();
    let run = CronRunInfo {
        cancel_reason: None,
        run_id: "fixture-run".into(),
        started_at: now,
        finished_at: None,
        status: CronRunStatus::Running,
        tab_id: Some(tab_id.clone()),
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    };
    commit(&state.cron, |store| {
        store.prepare_record_run(&job.id, run.clone())
    })
    .unwrap();
    state
        .cron
        .lock()
        .register_tab(&tab_id, &job.id, "fixture-run");
    let policy = role_policy::resolve(Some("curator")).unwrap();
    policy
        .spawned(&fixture.service, &job, &tab_id, "fixture-run", now)
        .unwrap();
    fixture
        .service
        .agent_run_pty_map
        .lock()
        .insert(tab_id.clone(), "fixture-pty".into());
    fixture.service.run_watcher.register(
        fixture.service.clone(),
        "fixture-pty".into(),
        tab_id.clone(),
        codeterm_service_core::agent::run_watcher::AgentRunKind::Cron {
            job_id: job.id.clone(),
            run_id: "fixture-run".into(),
        },
        900,
        None,
    );
    let response = hook_session_start(AxumState(state.clone()), Path(tab_id.clone()), HeaderMap::new(),
        OptionalJson(Some(serde_json::json!({"provider_id":PROVIDER,"session_id":SESSION,"binding_nonce":NONCE,"source":"startup"})))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let claimed = fixture
        .service
        .plan_store
        .get(&plan.id)
        .unwrap()
        .control_loop
        .running
        .unwrap();
    assert_eq!(claimed.session_id.as_deref(), Some(SESSION));
    assert_eq!(claimed.binding_nonce.as_deref(), Some(NONCE));
    let mut caller = AuthenticatedPlanCaller {
        tab_id: Some(tab_id.clone()),
        session_id: None,
        binding_nonce: Some(NONCE.into()),
    };
    caller.binding_nonce = Some("foreign".into());
    assert!(fixture.service.curator_identity(&caller).is_err());
    caller.binding_nonce = Some(NONCE.into());
    caller.session_id = Some("foreign-session".into());
    assert!(fixture.service.curator_identity(&caller).is_err());
    caller.session_id = None;
    if deadline {
        let (owner, team) = {
            let mut mux = fixture.service.mux.write();
            let owner = mux.create_tab(&workspace).unwrap();
            mux.set_orchestrator_role(&owner).unwrap();
            let team = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
            let tab = mux.get_tab_mut(&owner).unwrap();
            tab.agent_type = Some(PROVIDER.into());
            tab.agent_session_id = Some("owner-session".into());
            tab.agent_session_model = Some("owner-model".into());
            tab.session_binding_nonce = Some("owner-nonce".into());
            tab.reported_session_binding_nonce = Some("owner-nonce".into());
            tab.pty_id = Some("owner-pty".into());
            tab.chat_view = true;
            assert!(codeterm_service_core::service::plan_reconcile::tab_has_live_agent(tab));
            mux.get_tab_mut(&tab_id).unwrap().team_id = Some(team.clone());
            (owner, team)
        };
        let mut job = state.cron.lock().get(&job.id).unwrap().clone();
        job.team_id = Some(team.clone());
        job.protected = true;
        job.oneshot = true;
        commit(&state.cron, |_| {
            Ok(Prepared::new(CronChange::Patch(job.clone()), ()))
        })
        .unwrap();
        fixture
            .service
            .plan_store
            .with_plan(&plan.id, now, |plan| {
                plan.mode = codeterm_service_core::plan::PlanMode::Execution;
                plan.team_id = Some(team.clone());
                plan.owner = OrchestratorRef::from_tab(&owner);
                plan.owner.session_id = Some("owner-session".into());
                plan.control_loop.cron_id = Some(job.id.clone());
                plan.control_loop.running.as_mut().unwrap().started_at = now - 900_000;
                Ok(())
            })
            .unwrap();
        let settled = policy
            .settle_cron(&fixture.service, &job, &tab_id, "fixture-run", now)
            .unwrap();
        assert!(!settled.success);
        assert_eq!(settled.summary, "partial");
    } else if finish {
        fixture.service.curator_finish(&caller, "NEXT: 2h | bounded fixture\nMOVE: inspect fixture [task_fixture]\nRETRO: + observed fixture [task_fixture]\nASKED: none", 7200, now).unwrap();
        let finished = fixture.service.plan_store.get(&plan.id).unwrap();
        assert!(finished.control_loop.running.is_none());
        assert_eq!(finished.control_loop.next_at, Some(now + 7_200_000));
        assert_eq!(
            finished
                .control_loop
                .last_round
                .unwrap()
                .outcome
                .to_string(),
            "done"
        );
    } else {
        assert!(fixture
            .service
            .curator_finish(&caller, "invalid", 7200, now)
            .is_err());
    }
    fixture.service.close_tab(&tab_id).unwrap();
    if deadline {
        fixture.service.reconcile_curator_for_test(&plan.id, now);
    }
    let closed = fixture.service.plan_store.get(&plan.id).unwrap();
    assert!(closed.control_loop.running.is_none());
    let round = closed.control_loop.last_round.unwrap();
    assert_eq!(
        round.outcome.to_string(),
        if finish { "done" } else { "partial" }
    );
    let cron_run = state
        .cron
        .lock()
        .get(&job.id)
        .unwrap()
        .last_run
        .clone()
        .unwrap();
    assert_eq!(
        cron_run.status,
        if finish {
            CronRunStatus::Success
        } else {
            CronRunStatus::Failed
        }
    );
    assert_eq!(cron_run.session_id.as_deref(), Some(SESSION));
    if deadline {
        assert_eq!(round.reason, "deadline");
        assert_eq!(closed.control_loop.next_at, Some(now + 2_700_000));
        let cron = state.cron.lock();
        let job = cron.get(&job.id).unwrap();
        assert!(job.enabled);
        assert_eq!(job.next_run, closed.control_loop.next_at);
        assert!(fixture.service.mux.read().get_tab(&tab_id).is_none());
    } else if !finish {
        assert_eq!(round.reason, "tab_closed");
        assert_eq!(
            state
                .cron
                .lock()
                .get(&job.id)
                .unwrap()
                .last_run
                .as_ref()
                .unwrap()
                .status,
            CronRunStatus::Failed
        );
    }
    fixture
        .service
        .run_watcher
        .mark_completed_externally("fixture-pty");
}

#[tokio::test]
async fn curator_hook_journey_binds_claim_and_finishes_without_session_env() {
    journey(true, false).await;
}

#[tokio::test]
async fn curator_hook_journey_settles_closed_tab_after_refused_finish() {
    journey(false, false).await;
}

#[tokio::test]
async fn curator_hook_journey_deadline_settles_partial_closes_and_rearms() {
    journey(false, true).await;
}

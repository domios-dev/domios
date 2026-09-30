use std::fs;
use std::sync::Arc;

use parking_lot::Mutex;

use super::CronRunEnd;
use crate::agent::provider::ModelInfo;
use crate::agent::test_provider::MockProvider;
use crate::cron::store::{
    CronCreateParams, CronRunInfo, CronRunStatus, CronStore, CronUpdateParams,
};
use crate::service::test_fixture::ServiceFixture;

fn fixture() -> (
    ServiceFixture,
    crate::CronState,
    crate::CronWake,
    std::path::PathBuf,
) {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_reasoning_efforts("tiered", &["low", "medium", "high"]),
    ));
    let path = fixture.temp.path().join("crons.json");
    let cron = Arc::new(Mutex::new(CronStore::new_with_path(path.clone())));
    let wake = Arc::new(std::sync::Condvar::new());
    (fixture, cron, wake, path)
}

fn create_params(name: &str, effort: Option<&str>) -> CronCreateParams {
    CronCreateParams {
        name: name.into(),
        workspace_id: "workspace".into(),
        team_id: None,
        detach_from_team: false,
        schedule: "0 * * * *".into(),
        task: "run the job".into(),
        description: None,
        agent: Some("tiered".into()),
        model: None,
        reasoning_effort: effort.map(str::to_string),
        cwd: None,
        role: None,
        skill: None,
        creator_tab_id: None,
        report_target_tab_id: None,
        enabled: Some(false),
        max_history: None,
        oneshot: None,
        jitter_max_ms: Some(0),
        tz: None,
        timeout_seconds: None,
    }
}

fn effort_patch(effort: &str) -> CronUpdateParams {
    CronUpdateParams {
        name: None,
        description: None,
        schedule: None,
        task: None,
        cwd: None,
        role: None,
        skill: None,
        enabled: None,
        max_history: None,
        agent: None,
        model: None,
        reasoning_effort: Some(effort.into()),
        jitter_max_ms: None,
        tz: None,
        timeout_seconds: None,
    }
}

fn empty_patch() -> CronUpdateParams {
    CronUpdateParams {
        name: None,
        description: None,
        schedule: None,
        task: None,
        cwd: None,
        role: None,
        skill: None,
        enabled: None,
        max_history: None,
        agent: None,
        model: None,
        reasoning_effort: None,
        jitter_max_ms: None,
        tz: None,
        timeout_seconds: None,
    }
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn cron_reasoning_effort_round_trips_for_advertised_levels() {
    let (fixture, cron, wake, path) = fixture();
    for effort in ["low", "medium", "high"] {
        let job = fixture
            .service
            .cron_create(&cron, &wake, create_params(effort, Some(effort)))
            .unwrap();
        assert_eq!(job.reasoning_effort.as_deref(), Some(effort));
    }

    let reloaded = CronStore::new_with_path(path);
    let persisted: Vec<_> = reloaded
        .list()
        .iter()
        .map(|job| job.reasoning_effort.as_deref())
        .collect();
    assert_eq!(persisted, vec![Some("low"), Some("medium"), Some("high")]);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn cron_create_rejects_unsupported_effort_without_write() {
    let (fixture, cron, wake, path) = fixture();
    fixture
        .service
        .cron_create(&cron, &wake, create_params("existing", Some("low")))
        .unwrap();
    let before = fs::read(&path).unwrap();

    let error = fixture
        .service
        .cron_create(&cron, &wake, create_params("invalid", Some("ultra")))
        .expect_err("unsupported effort must be rejected before persistence");

    assert!(error.contains("tiered"), "provider name missing: {error}");
    assert!(
        error.contains("low, medium, high"),
        "advertised levels missing: {error}"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(cron.lock().list().len(), 1);
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn cron_edit_rejects_unsupported_effort_without_write() {
    let (fixture, cron, wake, path) = fixture();
    let job = fixture
        .service
        .cron_create(&cron, &wake, create_params("existing", Some("low")))
        .unwrap();
    let before = fs::read(&path).unwrap();

    let error = fixture
        .service
        .cron_update(&cron, &wake, &job.id, effort_patch("ultra"))
        .expect_err("unsupported effort must be rejected before persistence");

    assert!(error.contains("tiered"), "provider name missing: {error}");
    assert!(
        error.contains("low, medium, high"),
        "advertised levels missing: {error}"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(
        cron.lock()
            .get(&job.id)
            .unwrap()
            .reasoning_effort
            .as_deref(),
        Some("low")
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn cron_empty_reasoning_effort_clears_override() {
    let (fixture, cron, wake, path) = fixture();
    let job = fixture
        .service
        .cron_create(&cron, &wake, create_params("clearable", Some("high")))
        .unwrap();

    let updated = fixture
        .service
        .cron_update(&cron, &wake, &job.id, effort_patch(""))
        .unwrap();

    assert_eq!(updated.reasoning_effort, None);
    let raw = fs::read_to_string(path).unwrap();
    assert!(!raw.contains("reasoning_effort"));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn cron_create_validates_agent_model_cwd() {
    let (fixture, cron, wake, path) = fixture();
    let models = vec![ModelInfo {
        id: "fixture-model".into(),
        display_name: "Fixture model".into(),
        ..ModelInfo::default()
    }];
    crate::plugin::model_catalog::register(
        "cron-catalogue",
        crate::plugin::model_catalog::RegisteredDefaults {
            models: models.clone(),
            default: Some("fixture-model".into()),
        },
    );
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new("cron-catalogue")
            .with_models(models)
            .with_authoritative_model_catalogue(),
    ));
    let valid = fixture
        .service
        .cron_create(&cron, &wake, create_params("baseline", None))
        .unwrap();
    let model_job = fixture
        .service
        .cron_create(&cron, &wake, {
            let mut params = create_params("catalogue-job", None);
            params.agent = Some("cron-catalogue".into());
            params.model = Some("fixture-model".into());
            params
        })
        .unwrap();
    let initial = fs::read(&path).unwrap();
    let missing_cwd = fixture
        .temp
        .path()
        .join("missing-cron-cwd")
        .display()
        .to_string();

    let mut missing_creator = create_params("missing-creator", None);
    missing_creator.creator_tab_id = Some("missing-tab".into());
    let error = fixture
        .service
        .cron_create(&cron, &wake, missing_creator)
        .unwrap_err();
    assert!(error.starts_with("creator_tab_id:"), "{error}");

    let mut invalid_agent = create_params("invalid-agent", None);
    invalid_agent.agent = Some("missing-agent".into());
    let error = fixture
        .service
        .cron_create(&cron, &wake, invalid_agent)
        .unwrap_err();
    assert!(error.starts_with("agent:"), "{error}");

    let mut invalid_model = create_params("invalid-model", None);
    invalid_model.agent = Some("cron-catalogue".into());
    invalid_model.model = Some("missing-model".into());
    let error = fixture
        .service
        .cron_create(&cron, &wake, invalid_model)
        .unwrap_err();
    assert!(error.starts_with("model:"), "{error}");

    let mut invalid_cwd = create_params("invalid-cwd", None);
    invalid_cwd.cwd = Some(missing_cwd.clone());
    let error = fixture
        .service
        .cron_create(&cron, &wake, invalid_cwd)
        .unwrap_err();
    assert!(error.starts_with("cwd:"), "{error}");

    let agent_error = fixture
        .service
        .cron_update(
            &cron,
            &wake,
            &valid.id,
            CronUpdateParams {
                agent: Some("missing-agent".into()),
                ..empty_patch()
            },
        )
        .unwrap_err();
    assert!(agent_error.starts_with("agent:"), "{agent_error}");

    let model_error = fixture
        .service
        .cron_update(
            &cron,
            &wake,
            &model_job.id,
            CronUpdateParams {
                model: Some("missing-model".into()),
                ..empty_patch()
            },
        )
        .unwrap_err();
    assert!(model_error.starts_with("model:"), "{model_error}");

    let cwd_error = fixture
        .service
        .cron_update(
            &cron,
            &wake,
            &valid.id,
            CronUpdateParams {
                cwd: Some(missing_cwd),
                ..empty_patch()
            },
        )
        .unwrap_err();
    assert!(cwd_error.starts_with("cwd:"), "{cwd_error}");
    assert_eq!(fs::read(&path).unwrap(), initial);
    assert_eq!(cron.lock().list().len(), 2);
    assert_eq!(cron.lock().get(&valid.id).unwrap().agent, "tiered");
    assert_eq!(cron.lock().get(&valid.id).unwrap().cwd, None);
    assert_eq!(
        cron.lock().get(&model_job.id).unwrap().model.as_deref(),
        Some("fixture-model")
    );
}

fn started_job(cron: &crate::CronState, oneshot: bool, report_target: Option<&str>) -> String {
    let mut params = create_params("settle", None);
    params.schedule = if oneshot { "in 1h" } else { "0 * * * *" }.into();
    params.oneshot = Some(oneshot);
    params.report_target_tab_id = report_target.map(str::to_string);
    let mut store = cron.lock();
    let job = store.create(params).unwrap();
    store
        .record_run(
            &job.id,
            CronRunInfo {
                run_id: "settle-run".into(),
                started_at: 1,
                finished_at: None,
                status: CronRunStatus::Running,
                tab_id: Some("settle-run-tab".into()),
                session_id: None,
                cwd: None,
                summary: None,
                report_delivery: None,
            },
        )
        .unwrap();
    store
        .update_run_status(&job.id, "settle-run", CronRunStatus::Success)
        .unwrap();
    job.id
}

fn settle(
    fixture: &ServiceFixture,
    cron: &crate::CronState,
    wake: &crate::CronWake,
    job_id: &str,
    delivery: Result<(), String>,
) -> Vec<(String, String)> {
    let mut delivered = Vec::new();
    fixture.service.cron_settle_run_end_with(
        cron,
        Some(wake),
        CronRunEnd {
            job_id,
            run_id: "settle-run",
            status: CronRunStatus::Success,
            summary: "settle-summary-token",
        },
        |tab_id, text| {
            delivered.push((tab_id.to_string(), text.to_string()));
            delivery
        },
    );
    delivered
}

#[test]
fn submit_unconfirmed_report_delivery_is_not_classified_as_failed() {
    assert_eq!(
        super::cron_report_delivery_label(Err(
            "submit_unconfirmed: submit key was not acknowledged".into()
        )),
        "delivered_unconfirmed"
    );
    assert_eq!(
        super::cron_report_delivery_label(Err("target gone".into())),
        "failed: target gone"
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn completed_oneshot_reports_and_keeps_history() {
    let (fixture, cron, wake, _) = fixture();
    let job_id = started_job(&cron, true, Some("report-tab"));

    let delivered = settle(&fixture, &cron, &wake, &job_id, Ok(()));

    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].0, "report-tab");
    assert!(delivered[0].1.contains("settle-summary-token"));
    let store = cron.lock();
    assert!(store.get(&job_id).is_none());
    let history = store.get_history(&job_id);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, CronRunStatus::Success);
    assert_eq!(history[0].summary.as_deref(), Some("settle-summary-token"));
    assert_eq!(history[0].report_delivery.as_deref(), Some("delivered"));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn recurring_run_end_reports_without_retiring_the_job() {
    let (fixture, cron, wake, _) = fixture();
    let job_id = started_job(&cron, false, Some("report-tab"));

    let delivered = settle(&fixture, &cron, &wake, &job_id, Err("target gone".into()));

    assert_eq!(delivered.len(), 1);
    let store = cron.lock();
    assert!(
        store.get(&job_id).is_some(),
        "a recurring job survives its run"
    );
    let history = store.get_history(&job_id);
    assert_eq!(
        history[0].report_delivery.as_deref(),
        Some("failed: target gone")
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn cron_complete_requires_run_owner() {
    let (mut mux, owner_tab) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner_tab).unwrap();
    let foreign_tab = mux.create_tab(&workspace).unwrap();
    mux.get_tab_mut(&owner_tab).unwrap().session_binding_nonce = Some("owner-nonce".into());
    mux.get_tab_mut(&foreign_tab).unwrap().session_binding_nonce = Some("foreign-nonce".into());
    let fixture = ServiceFixture::new(mux);
    let cron = Arc::new(Mutex::new(CronStore::new_with_path(
        fixture.temp.path().join("crons.json"),
    )));
    let wake = Arc::new(std::sync::Condvar::new());
    let run_id = "owner-run";
    let job_id = {
        let mut store = cron.lock();
        let job = store.create(create_params("owner-check", None)).unwrap();
        store
            .record_run(
                &job.id,
                CronRunInfo {
                    run_id: run_id.into(),
                    started_at: 1,
                    finished_at: None,
                    status: CronRunStatus::Running,
                    tab_id: Some(owner_tab.clone()),
                    session_id: None,
                    cwd: None,
                    summary: None,
                    report_delivery: None,
                },
            )
            .unwrap();
        store.register_tab(&owner_tab, &job.id, run_id);
        job.id
    };

    assert!(fixture
        .service
        .cron_complete(
            &cron,
            &wake,
            run_id,
            Some("human-summary"),
            None,
            None,
            false
        )
        .is_err());

    for (caller_tab_id, binding_nonce) in [
        (foreign_tab.as_str(), "foreign-nonce"),
        (owner_tab.as_str(), "foreign-nonce"),
    ] {
        assert!(fixture
            .service
            .cron_complete(
                &cron,
                &wake,
                run_id,
                Some("foreign-summary"),
                Some(caller_tab_id),
                Some(binding_nonce),
                false,
            )
            .is_err());
    }

    {
        let store = cron.lock();
        let run = store
            .get_history(&job_id)
            .into_iter()
            .find(|run| run.run_id == run_id)
            .expect("active run remains recorded");
        assert_eq!(run.status, CronRunStatus::Running);
        assert_eq!(run.summary, None);
    }

    assert!(fixture
        .service
        .cron_complete(
            &cron,
            &wake,
            run_id,
            Some("owner-summary"),
            Some(&owner_tab),
            Some("owner-nonce"),
            false,
        )
        .is_ok());
    let store = cron.lock();
    let run = store
        .get_history(&job_id)
        .into_iter()
        .find(|run| run.run_id == run_id)
        .expect("owner-completed run remains recorded");
    assert_eq!(run.status, CronRunStatus::Success);
}

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::CronRunEnd;
use crate::agent::provider::ModelInfo;
use crate::agent::test_provider::MockProvider;
use crate::cron::clock::MockClock;
use crate::cron::scheduler::{record_fire_start, FireStart};
use crate::cron::store::{
    commit, CronCreateParams, CronRunInfo, CronRunStatus, CronStorageOptions, CronStore,
    CronUpdateParams,
};
use crate::service::test_fixture::ServiceFixture;
use crate::storage::{Fault, Storage, StorageConfig};

#[test]
fn cron_fired_payload_identifies_the_recorded_run_tab() {
    assert_eq!(
        super::cron_fired_payload("cron-nightly", "run-42", "tab-42"),
        serde_json::json!({
            "cron_id": "cron-nightly",
            "run_id": "run-42",
            "tab_id": "tab-42"
        })
    );
}

fn fixture() -> (
    ServiceFixture,
    crate::CronState,
    crate::CronWake,
    std::path::PathBuf,
) {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(tiered_provider()));
    let path = fixture.temp.path().join("crons.json");
    let cron = Arc::new(Mutex::new(CronStore::new_with_path(path.clone())));
    let wake = Arc::new(std::sync::Condvar::new());
    (fixture, cron, wake, path)
}

// Launch preflight refuses a provider whose CLI is missing, so the fixture launches a real shell.
fn launchable(provider: MockProvider) -> MockProvider {
    provider.with_launch_binary(if cfg!(windows) { "cmd.exe" } else { "sh" })
}

fn tiered_provider() -> MockProvider {
    launchable(MockProvider::with_reasoning_efforts(
        "tiered",
        &["low", "medium", "high"],
    ))
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
fn storage_cutover_cron_reads_error_when_storage_is_unavailable() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let logs = tempfile::tempdir().expect("cron log root");
    let clock: crate::cron::clock::SharedClock = MockClock::starting_at(0);
    let cron = Arc::new(Mutex::new(CronStore::unavailable(
        "database unavailable".into(),
        logs.path().to_path_buf(),
        clock,
    )));

    assert!(fixture
        .service
        .cron_list(&cron, None)
        .expect_err("list must not become an empty success")
        .contains("cron storage unavailable"));
    assert!(fixture
        .service
        .cron_get(&cron, "missing")
        .expect_err("get must report unavailable")
        .contains("cron storage unavailable"));
    assert!(fixture
        .service
        .cron_history(&cron, "missing")
        .expect_err("history must report unavailable")
        .contains("cron storage unavailable"));
}

#[test]
fn cron_service_mutations_and_run_lifecycle_survive_storage_reopen() {
    let (mut mux, owner_tab) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&owner_tab)
        .expect("owner tab")
        .session_binding_nonce = Some("storage-owner-nonce".into());
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(tiered_provider()));
    let wake = Arc::new(std::sync::Condvar::new());
    let instance = fixture.temp.path().join("cron-service-storage");
    let storage = Storage::open(&instance, StorageConfig::default()).expect("open storage");
    let cron = Arc::new(Mutex::new(
        CronStore::open_storage(
            &storage,
            fixture.temp.path().join("cron-logs"),
            MockClock::starting_at(1_700_000_000_000),
            CronStorageOptions::default(),
        )
        .expect("open storage-backed cron store"),
    ));

    let job = fixture
        .service
        .cron_create(
            &cron,
            &wake,
            CronCreateParams {
                workspace_id: fixture
                    .service
                    .mux
                    .read()
                    .find_workspace_for_tab(&owner_tab)
                    .expect("owner workspace"),
                ..create_params("service-persisted", None)
            },
        )
        .expect("create cron through ServiceContext");
    let mut patch = empty_patch();
    patch.name = Some("service-persisted-updated".into());
    let updated = fixture
        .service
        .cron_update(&cron, &wake, &job.id, patch)
        .expect("update cron through ServiceContext");
    assert_eq!(updated.name, "service-persisted-updated");

    let run_id = "service-persisted-run";
    assert_eq!(
        record_fire_start(&cron, &job.id, run_id),
        FireStart::Recorded,
        "run start must commit before it is considered fired"
    );
    commit(&cron, |store| {
        Ok(store.prepare_set_run_tab(&job.id, run_id, &owner_tab))
    })
    .expect("persist run owner");
    cron.lock().register_tab(&owner_tab, &job.id, run_id);
    let completion = fixture
        .service
        .cron_complete(
            &cron,
            &wake,
            run_id,
            Some("service-completion-summary"),
            Some(&owner_tab),
            Some("storage-owner-nonce"),
            false,
        )
        .expect("finish run through service completion")
        .expect("owned run schedules its after-response completion");
    fixture.service.cron_settle_run_end(
        &completion.cron,
        Some(&completion.wake),
        CronRunEnd {
            job_id: &completion.job_id,
            run_id: &completion.run_id,
            status: CronRunStatus::Success,
            summary: &completion.summary,
        },
    );

    storage
        .shutdown()
        .wait(Duration::from_secs(30))
        .expect("close first storage instance");
    drop(cron);
    drop(storage);

    let reopened = Storage::open(&instance, StorageConfig::default()).expect("reopen storage");
    let reloaded = Arc::new(Mutex::new(
        CronStore::open_storage(
            &reopened,
            fixture.temp.path().join("cron-logs"),
            MockClock::starting_at(1_700_000_000_000),
            CronStorageOptions::default(),
        )
        .expect("hydrate reopened cron store"),
    ));
    let restored = fixture
        .service
        .cron_get(&reloaded, &job.id)
        .expect("service reads restored definition");
    assert_eq!(restored.name, "service-persisted-updated");
    let history = fixture
        .service
        .cron_history(&reloaded, &job.id)
        .expect("service reads restored run history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].run_id, run_id);
    assert_eq!(history[0].status, CronRunStatus::Success);
    assert_eq!(
        history[0].summary.as_deref(),
        Some("service-completion-summary")
    );

    fixture
        .service
        .cron_delete(&reloaded, &wake, &job.id)
        .expect("delete cron through ServiceContext");
    let stored = reopened
        .crons()
        .snapshot()
        .wait(Duration::from_secs(30))
        .expect("read committed delete")
        .value;
    assert!(stored
        .states
        .iter()
        .all(|state| state.job_id.as_str() != job.id.as_str()));
    reopened
        .shutdown()
        .wait(Duration::from_secs(30))
        .expect("close reopened storage instance");
}

#[test]
fn cron_history_accepts_a_unique_job_name_and_its_id() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");
    fs::write(
        &path,
        r#"{"version":"1","jobs":[{"id":"cron-id","name":"Nightly QA","workspace_id":"workspace","schedule":"* * * * *","enabled":false,"agent":"claude","task":"run checks","created_at":1}],"history":{"cron-id":[{"run_id":"run-id","started_at":10,"status":"Success"}]}}"#,
    )
    .unwrap();
    let cron = Arc::new(Mutex::new(CronStore::new_with_path(path)));

    let by_id = fixture.service.cron_history(&cron, "cron-id").unwrap();
    let by_name = fixture.service.cron_history(&cron, "Nightly QA").unwrap();

    assert_eq!(by_id, by_name);
    assert_eq!(by_name[0].run_id, "run-id");
}

#[test]
fn cron_history_refuses_an_ambiguous_job_name() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crons.json");
    fs::write(
        &path,
        r#"{"version":"1","jobs":[{"id":"cron-a","name":"Shared","workspace_id":"one","schedule":"* * * * *","enabled":false,"agent":"claude","task":"run","created_at":1},{"id":"cron-b","name":"Shared","workspace_id":"two","schedule":"* * * * *","enabled":false,"agent":"claude","task":"run","created_at":2}],"history":{}}"#,
    )
    .unwrap();
    let cron = Arc::new(Mutex::new(CronStore::new_with_path(path)));

    assert_eq!(
        fixture.service.cron_history(&cron, "Shared").unwrap_err(),
        "Cron job name is ambiguous: Shared"
    );
}

#[test]
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
        launchable(MockProvider::new("cron-catalogue"))
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
        |tab_id, envelope, text| {
            delivered.push((
                tab_id.to_string(),
                crate::service::messenger::Messenger::from(envelope).wrap(text),
            ));
            delivery
        },
    );
    delivered
}

#[test]
fn cron_end_context_names_job_and_lowercase_status() {
    assert_eq!(
        super::cron_end_context("nightly-build", &CronRunStatus::Success),
        "cron:nightly-build:success"
    );
    assert_eq!(
        super::cron_end_context("nightly-build", &CronRunStatus::Failed),
        "cron:nightly-build:failed"
    );
    assert_eq!(
        super::cron_end_context("job-1", &CronRunStatus::Cancelled),
        "cron:job-1:cancelled"
    );
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
fn completed_oneshot_reports_and_keeps_history() {
    let (fixture, cron, wake, _) = fixture();
    let job_id = started_job(&cron, true, Some("report-tab"));

    let delivered = settle(&fixture, &cron, &wake, &job_id, Ok(()));

    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].0, "report-tab");
    assert_eq!(
        delivered[0].1,
        "<domios from=\"system\" context=\"cron:settle:success\">Cron run settle-run completed with status Success. Summary: settle-summary-token</domios>"
    );
    let store = cron.lock();
    assert!(store.get(&job_id).is_none());
    let history = store.get_history(&job_id);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, CronRunStatus::Success);
    assert_eq!(history[0].summary.as_deref(), Some("settle-summary-token"));
    assert_eq!(history[0].report_delivery.as_deref(), Some("delivered"));
}

#[test]
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

#[test]
fn cron_settle_refused_trigger_record_never_dispatches() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::open(temp.path().join("instance"), StorageConfig::default())
        .expect("open storage");
    let cron = Arc::new(Mutex::new(
        CronStore::open_storage(
            &storage,
            temp.path().join("crons"),
            MockClock::starting_at(1),
            CronStorageOptions::default(),
        )
        .expect("open storage-backed cron store"),
    ));
    let job = commit(&cron, |store| {
        store.prepare_create(create_params("refused-trigger", None))
    })
    .expect("create existing job")
    .value;
    storage
        .inject_fault(Fault::FailNextCommit)
        .wait(Duration::from_secs(30))
        .expect("arm commit fault");
    let run = CronRunInfo {
        run_id: "refused-trigger-run".into(),
        started_at: 1,
        finished_at: None,
        status: CronRunStatus::Running,
        tab_id: None,
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    };
    let mut dispatched = false;

    let result = super::dispatch_after_trigger_record(&cron, &job.id, &run, || {
        dispatched = true;
    });

    assert!(result.is_err());
    assert!(!dispatched, "refused run record never dispatches an agent");
    assert!(cron.lock().get_history(&job.id).is_empty());
    let committed = storage
        .crons()
        .snapshot()
        .wait(Duration::from_secs(30))
        .expect("read committed cron snapshot")
        .value;
    let stored_job = committed
        .states
        .iter()
        .find(|state| state.job_id.as_str() == job.id.as_str())
        .expect("existing job remains stored");
    assert!(stored_job.history.is_empty());
    storage
        .shutdown()
        .wait(Duration::from_secs(30))
        .expect("shutdown storage");
}

#[test]
fn runtime_cron_owner_binding_is_exact_and_idempotent_before_launch() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = CronStore::new_with_path(temp.path().join("runtime-cron.json"));
    let job = store.create(create_params("runtime-owner", None)).unwrap();
    store
        .record_run(
            &job.id,
            CronRunInfo {
                run_id: "run-exact".into(),
                started_at: 1,
                finished_at: None,
                status: CronRunStatus::Running,
                tab_id: None,
                session_id: None,
                cwd: None,
                summary: None,
                report_delivery: None,
            },
        )
        .unwrap();
    assert!(super::runtime_cron_owner_needs_registration(
        &store,
        "tab-exact",
        &job.id,
        "run-exact"
    )
    .unwrap());
    assert!(super::runtime_cron_owner_needs_registration(
        &store,
        "tab-exact",
        "foreign-job",
        "run-exact"
    )
    .is_err());
    store.register_tab("tab-exact", &job.id, "run-exact");
    assert!(!super::runtime_cron_owner_needs_registration(
        &store,
        "tab-exact",
        &job.id,
        "run-exact"
    )
    .unwrap());
    store.register_tab("tab-exact", &job.id, "foreign-run");
    assert!(super::runtime_cron_owner_needs_registration(
        &store,
        "tab-exact",
        &job.id,
        "run-exact"
    )
    .is_err());
    store.unregister_tab("tab-exact");
    store
        .update_run_status(&job.id, "run-exact", CronRunStatus::Cancelled)
        .unwrap();
    assert!(super::runtime_cron_owner_needs_registration(
        &store,
        "tab-exact",
        &job.id,
        "run-exact"
    )
    .is_err());
}

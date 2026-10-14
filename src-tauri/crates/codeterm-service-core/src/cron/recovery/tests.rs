use std::sync::Arc;
use std::time::Duration;

use super::*;
use parking_lot::Mutex;

use crate::cron::clock::MockClock;
use crate::cron::scheduler::CronState;
use crate::cron::store::{
    commit, CronCreateParams, CronRunInfo, CronRunStatus, CronStorageOptions, CronStore,
};
use crate::storage::{Storage, StorageConfig};

#[test]
fn cron_recovery_preserves_live_prompt_and_recovers_vanished_workspace_run() {
    let temp = tempfile::tempdir().expect("temporary storage root");
    let instance = temp.path().join("instance");
    let logs = temp.path().join("logs");
    let (mut initial_mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = initial_mux
        .create_workspace("restored cron workspace".into(), None)
        .id;
    initial_mux
        .set_active_workspace(&workspace_id)
        .expect("activate restored workspace");
    let live_tab_id = initial_mux
        .create_tab(&workspace_id)
        .expect("create live owner tab");
    let snapshot = initial_mux.to_session_snapshot();
    let storage = Storage::open(&instance, StorageConfig::default()).expect("open storage");
    let cron: CronState = Arc::new(Mutex::new(
        CronStore::open_storage(
            &storage,
            logs.clone(),
            MockClock::starting_at(1_700_000_000_000),
            CronStorageOptions::default(),
            crate::config::schema::AppConfig::default()
                .general_agent
                .agent_provider,
        )
        .expect("open cron storage"),
    ));
    let live_job = create_job(&cron, "live-owner", &workspace_id);
    let orphan_job = create_job(&cron, "vanished-owner", "missing-workspace");
    record_active_run(&cron, &live_job.id, "live-run", &live_tab_id);
    record_active_run(&cron, &orphan_job.id, "orphan-run", "vanished-tab");

    storage
        .shutdown()
        .wait(Duration::from_secs(30))
        .expect("close first storage instance");
    drop(cron);
    drop(storage);

    let reopened = Storage::open(&instance, StorageConfig::default()).expect("reopen storage");
    let cron: CronState = Arc::new(Mutex::new(
        CronStore::open_storage(
            &reopened,
            logs,
            MockClock::starting_at(1_700_000_000_000),
            CronStorageOptions::default(),
            crate::config::schema::AppConfig::default()
                .general_agent
                .agent_provider,
        )
        .expect("hydrate cron storage"),
    ));
    let restored_mux =
        crate::multiplexer::state::MultiplexerState::from_session_snapshot(&snapshot);

    let restored = super::RestoredCronSession::from_mux(&restored_mux);
    let (retained, terminalized) = reconcile_restored_runs(&cron, &restored)
        .expect("startup reconciliation commits orphan terminal state");
    assert_eq!((retained, terminalized), (1, 1));
    let owner = cron.lock().get_tab_owner(&live_tab_id).cloned();
    assert_eq!(owner, Some((live_job.id.clone(), "live-run".to_string())));
    assert!(cron.lock().is_job_running(&live_job.id));
    let live = cron.lock().get_history(&live_job.id)[0].clone();
    assert_eq!(live.status, CronRunStatus::AwaitingOwnerPrompt);
    assert_eq!(live.finished_at, None);
    let orphan = cron.lock().get_history(&orphan_job.id)[0].clone();
    assert_eq!(orphan.status, CronRunStatus::Cancelled);
    assert!(!cron.lock().is_job_running(&orphan_job.id));
    assert!(orphan.finished_at.is_some());
    let reason = orphan.summary.expect("terminal reason persisted");
    assert!(reason.contains("vanished-tab"));
    assert!(reason.contains("missing-workspace"));
    assert_eq!(
        restored_spawn_workspace(&restored_mux, "missing-workspace"),
        Some(workspace_id.clone())
    );
    let spawn = crate::cron::spawn_spec::build_cron_spawn_spec(
        &crate::chatbackend::ChatBackendRegistry::new(),
        crate::cron::spawn_spec::CronSpawnInputs {
            job_id: orphan_job.id.clone(),
            run_id: "fixture-run".into(),
            job_name: orphan_job.name.clone(),
            agent: orphan_job.agent.clone(),
            model: orphan_job.model.clone(),
            reasoning_effort: orphan_job.reasoning_effort.clone(),
            task_text: orphan_job.task.clone(),
            role: orphan_job.role.clone(),
            cwd: orphan_job.cwd.clone(),
            workspace_id: restored_spawn_workspace(&restored_mux, &orphan_job.workspace_id),
            team_id: orphan_job.team_id.clone(),
            creator_tab_id: orphan_job.creator_tab_id.clone(),
            report_target_tab_id: orphan_job.report_target_tab_id.clone(),
        },
    );
    assert_eq!(spawn.workspace_id.as_deref(), Some(workspace_id.as_str()));

    reopened
        .shutdown()
        .wait(Duration::from_secs(30))
        .expect("close reconciled storage");
    drop(cron);
    drop(reopened);

    let verified = Storage::open(&instance, StorageConfig::default()).expect("verify fresh open");
    let verified_cron = CronStore::open_storage(
        &verified,
        temp.path().join("verify-logs"),
        MockClock::starting_at(1_700_000_000_000),
        CronStorageOptions::default(),
        crate::config::schema::AppConfig::default()
            .general_agent
            .agent_provider,
    )
    .expect("read committed reconciliation");
    let committed_orphan = verified_cron.get_history(&orphan_job.id)[0];
    assert_eq!(committed_orphan.status, CronRunStatus::Cancelled);
    assert!(committed_orphan
        .summary
        .as_deref()
        .is_some_and(|summary| summary.contains("missing-workspace")));
    let committed_live = verified_cron.get_history(&live_job.id)[0];
    assert_eq!(committed_live.status, CronRunStatus::AwaitingOwnerPrompt);
    assert_eq!(committed_live.finished_at, None);
    verified
        .shutdown()
        .wait(Duration::from_secs(30))
        .expect("close verified storage");
}

fn create_job(cron: &CronState, name: &str, workspace_id: &str) -> crate::cron::store::CronJob {
    commit(cron, |store| {
        store.prepare_create(CronCreateParams {
            name: name.into(),
            workspace_id: workspace_id.into(),
            team_id: None,
            detach_from_team: false,
            schedule: "0 * * * *".into(),
            task: "run the job".into(),
            description: None,
            agent: Some("test-agent".into()),
            model: None,
            reasoning_effort: None,
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
        })
    })
    .expect("create cron job")
    .value
}

fn record_active_run(cron: &CronState, job_id: &str, run_id: &str, tab_id: &str) {
    let run = CronRunInfo {
        run_id: run_id.into(),
        started_at: 1_700_000_000_000,
        finished_at: None,
        status: CronRunStatus::Running,
        tab_id: None,
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    };
    commit(cron, |store| store.prepare_record_run(job_id, run.clone())).expect("record active run");
    commit(cron, |store| {
        Ok(store.prepare_set_run_tab(job_id, run_id, tab_id))
    })
    .expect("record run owner tab");
    cron.lock().register_tab(tab_id, job_id, run_id);
    commit(cron, |store| Ok(store.prepare_owner_prompt(tab_id, true)))
        .expect("record owner prompt state");
}

//! Background cron scheduler — fires cron jobs and spawns agents.
//!
//! Time comes exclusively from the store's injected Clock, never from
//! SystemTime directly — that's what makes cron time-travel testable.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use super::store::{
    commit, finish_run, is_stale_fire, CronJob, CronRunInfo, CronRunStatus, CronStore,
    STALE_FIRE_MS,
};
use crate::service::ServiceState;
use parking_lot::Mutex;

pub type CronState = Arc<Mutex<CronStore>>;

/// Notification channel to wake the scheduler when cron jobs are modified.
pub type CronWake = Arc<std::sync::Condvar>;
const FAILURE_DEADLINE_MS: u64 = 60_000;

/// Start the cron scheduler in a background thread.
pub fn start(cron_state: CronState, cron_wake: CronWake, service: ServiceState) {
    let wake_mutex = Arc::new(std::sync::Mutex::new(()));

    codeterm_util::named_thread::spawn("cron-scheduler", move || {
        log::info!("Cron scheduler started");
        loop {
            let wait = tick(&cron_state, &service);
            let guard = wake_mutex.lock().unwrap();
            match wait {
                TickWait::Until(wait_ms) => {
                    let _ = cron_wake.wait_timeout(guard, Duration::from_millis(wait_ms));
                }
                TickWait::Settlement => {
                    if cron_state.lock().scheduler_settlement_ready() {
                        continue;
                    }
                    let timed_out = cron_wake
                        .wait_timeout(guard, Duration::from_millis(FAILURE_DEADLINE_MS))
                        .map(|(_, result)| result.timed_out())
                        .unwrap_or(true);
                    if timed_out {
                        let mut store = cron_state.lock();
                        if store.scheduler_settlement_ready() {
                            continue;
                        }
                        store.mark_scheduler_unavailable(
                            "cron storage settlement deadline expired".to_string(),
                        );
                    }
                }
                TickWait::Unavailable => {
                    let _ =
                        cron_wake.wait_timeout(guard, Duration::from_millis(FAILURE_DEADLINE_MS));
                }
            }
        }
    });
}

struct FireSpec {
    job_id: String,
    agent: String,
    model: Option<String>,
    reasoning_effort: Option<String>,
    task: String,
    cwd: Option<String>,
    role: Option<String>,
    workspace_id: String,
    team_id: Option<String>,
    creator_tab_id: Option<String>,
    report_target_tab_id: Option<String>,
    timeout_seconds: Option<u64>,
    skill: Option<String>,
}

fn fire_spec(job: &CronJob) -> FireSpec {
    FireSpec {
        job_id: job.id.clone(),
        agent: job.agent.clone(),
        model: job.model.clone(),
        reasoning_effort: job.reasoning_effort.clone(),
        task: job.task.clone(),
        cwd: job.cwd.clone(),
        role: job.role.clone(),
        workspace_id: job.workspace_id.clone(),
        team_id: job.team_id.clone(),
        creator_tab_id: job.creator_tab_id.clone(),
        report_target_tab_id: job.report_target_tab_id.clone(),
        timeout_seconds: job.timeout_seconds,
        skill: job.skill.clone(),
    }
}

/// Fires overdue jobs and selects the next wake condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TickWait {
    Until(u64),
    Settlement,
    Unavailable,
}

fn tick(cron_state: &CronState, service: &ServiceState) -> TickWait {
    tick_with(cron_state, |spec| fire_job(cron_state, service, spec))
}

fn tick_with(cron_state: &CronState, mut fire: impl FnMut(&FireSpec) -> FireStart) -> TickWait {
    if scheduler_unavailable(cron_state) {
        return TickWait::Unavailable;
    }
    let now_ms = cron_state.lock().now_ms();
    let mut next_fire: Option<u64> = None;
    let mut to_fire: Vec<FireSpec> = Vec::new();
    let mut stale: Vec<String> = Vec::new();

    {
        let store = cron_state.lock();
        for job in store.list_enabled() {
            if let Some(nr) = job.next_run {
                if nr <= now_ms {
                    if is_stale_fire(nr, now_ms, job.oneshot) {
                        stale.push(job.id.clone());
                        continue;
                    }
                    if store.is_job_running(&job.id) {
                        log::info!("[cron] skip {}: previous run still active", job.id);
                        let recheck = now_ms + 30_000;
                        next_fire = Some(match next_fire {
                            Some(current) => current.min(recheck),
                            None => recheck,
                        });
                        continue;
                    }
                    to_fire.push(fire_spec(job));
                } else {
                    next_fire = Some(match next_fire {
                        Some(current) => current.min(nr),
                        None => nr,
                    });
                }
            }
        }
    }

    for job_id in &stale {
        match commit(cron_state, |store| store.prepare_skip_stale_fire(job_id)) {
            Ok(committed) if committed.value => log::info!(
                "[cron] skip {job_id}: slot missed by more than {} min, moved to the next slot",
                STALE_FIRE_MS / 60_000
            ),
            Ok(_) => {}
            Err(error) => log::error!("[cron] could not skip stale slot for {job_id}: {error}"),
        }
    }

    let had_due_jobs = !to_fire.is_empty() || !stale.is_empty();
    let mut recorded_any = false;
    let mut refused = HashSet::new();
    for spec in to_fire {
        match fire(&spec) {
            FireStart::Recorded => recorded_any = true,
            FireStart::Refused => {
                if scheduler_unavailable(cron_state) {
                    return TickWait::Unavailable;
                }
                refused.insert(spec.job_id);
            }
            FireStart::Fenced => return TickWait::Settlement,
        }
    }

    if had_due_jobs {
        let store = cron_state.lock();
        for job in store.list_enabled() {
            if !refused.contains(&job.id) {
                if let Some(nr) = job.next_run {
                    next_fire = Some(match next_fire {
                        Some(current) => current.min(nr),
                        None => nr,
                    });
                }
            }
        }
    }

    // Sleep duration: min of (next_fire - now, 60s)
    match next_fire {
        Some(nr) => {
            let now_ms = cron_state.lock().now_ms();
            if nr > now_ms {
                TickWait::Until((nr - now_ms).min(FAILURE_DEADLINE_MS))
            } else {
                if recorded_any && refused.is_empty() {
                    TickWait::Until(100)
                } else {
                    TickWait::Until(FAILURE_DEADLINE_MS)
                }
            }
        }
        None => TickWait::Until(FAILURE_DEADLINE_MS),
    }
}

fn scheduler_unavailable(cron_state: &CronState) -> bool {
    let mut store = cron_state.lock();
    let unavailable = store.scheduler_unavailable();
    let incident = store.take_scheduler_unavailable_incident();
    drop(store);
    if let Some(reason) = incident {
        log::error!("[cron] scheduler unavailable: {reason}");
    }
    unavailable
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FireStart {
    Recorded,
    Refused,
    Fenced,
}

impl std::ops::Not for FireStart {
    type Output = bool;

    fn not(self) -> Self::Output {
        !matches!(self, FireStart::Recorded)
    }
}

fn fire_job(cron_state: &CronState, service: &ServiceState, spec: &FireSpec) -> FireStart {
    let job_id = spec.job_id.as_str();
    let agent = spec.agent.as_str();
    let model = spec.model.as_deref();
    let reasoning_effort = spec.reasoning_effort.as_deref();
    let task = spec.task.as_str();
    let cwd = spec.cwd.as_deref();
    let role = spec.role.as_deref();
    let workspace_id = spec.workspace_id.as_str();
    let report_target_tab_id = spec.report_target_tab_id.as_deref();
    let job_timeout = spec.timeout_seconds;

    let run_id = uuid::Uuid::new_v4().to_string();
    match record_fire_start(cron_state, job_id, &run_id) {
        FireStart::Recorded => {}
        outcome => return outcome,
    }
    log::info!("Cron fired: job={}, run={}", job_id, run_id);
    crate::flow_trace::record(&run_id, "cron.started", "", "", "fired", None);

    // Spawn agent via the unified spawn_tab surface.
    let skill_block = spec.skill.as_deref().map(crate::commands::cron_skill_block);
    let task_text = crate::cron::task_text::build_cron_task_text(
        task,
        skill_block.as_deref(),
        report_target_tab_id,
        &run_id,
    );
    let resume_system_prompt = crate::service::provider_launch_task(
        &service.agent_registry,
        agent,
        Some(&task_text),
        role,
        None,
    );

    // (shared default). Resolved against `agent`'s own catalog so a cron job
    // fired without a stored model still gets a model that provider accepts.
    let model = model.map(|s| s.to_string()).or_else(|| {
        let mem_cfg = crate::config::memory_config();
        let (resolved, _fell_back) = codeterm_mem::worker_agent::resolve_worker_model(
            &mem_cfg.dreaming_model,
            &mem_cfg.worker_model,
            agent,
            &crate::service::spawn::CatalogBackgroundModelPort,
        );
        (!resolved.is_empty()).then_some(resolved)
    });

    // Include cron job name in the tab title so reports show `cron:<job>` instead of plain `Terminal`.
    let job_name = cron_state
        .lock()
        .get(job_id)
        .map(|j| j.name.clone())
        .unwrap_or_else(|| job_id.to_string());

    // Routes through SpawnTabSpec::from_agent_params_with_chatbackends so a scheduled
    // agent and a CLI-spawned one agree on which runtime serves the provider.
    let spawn_spec = crate::cron::spawn_spec::build_cron_spawn_spec(
        service.chatbackend_registry.as_ref(),
        crate::cron::spawn_spec::CronSpawnInputs {
            job_id: job_id.to_string(),
            run_id: run_id.clone(),
            job_name,
            agent: agent.to_string(),
            model,
            reasoning_effort: reasoning_effort.map(str::to_string),
            task_text,
            role: role.map(|s| s.to_string()),
            cwd: cwd.map(|s| s.to_string()),
            workspace_id: crate::cron::recovery::restored_spawn_workspace(
                &service.mux.read(),
                workspace_id,
            ),
            team_id: spec.team_id.clone(),
            creator_tab_id: spec.creator_tab_id.clone(),
            report_target_tab_id: report_target_tab_id.map(str::to_string),
        },
    );

    match service.spawn_tab(spawn_spec) {
        Ok(handle) => {
            crate::flow_trace::record(&run_id, "cron.spawned", &handle.tab_id, "", "spawned", None);
            log::info!(
                "Cron agent spawned: job={}, tab={}, tab={}",
                job_id,
                handle.tab_id,
                handle.tab_id
            );

            cron_state
                .lock()
                .register_tab(&handle.tab_id, job_id, &run_id);
            if let Err(error) = commit(cron_state, |store| {
                Ok(store.prepare_set_run_tab(job_id, &run_id, &handle.tab_id))
            }) {
                log::error!("Failed to record cron run tab: job={job_id} run={run_id}: {error}");
            }
            service.sync_cron_owner_prompt_state(&handle.tab_id);

            // Register with the auto-complete watcher (cron flavor).
            if let Some(pty_id) = service
                .agent_run_pty_map
                .lock()
                .get(&handle.tab_id)
                .cloned()
            {
                let global_timeout = crate::config::memory_config().worker_timeout_seconds;
                let timeout = job_timeout.unwrap_or(global_timeout);
                if job_timeout.is_some() {
                    log::info!(
                        "[cron] fire {} run={} timeout={}s (per-job override)",
                        job_id,
                        run_id,
                        timeout
                    );
                }
                service.run_watcher.register(
                    (**service).clone(),
                    pty_id,
                    handle.tab_id.clone(),
                    crate::agent::run_watcher::AgentRunKind::Cron {
                        job_id: job_id.to_string(),
                        run_id: run_id.clone(),
                    },
                    timeout,
                    resume_system_prompt.clone(),
                );
            }

            crate::service::cron::emit_cron_fired(
                &service.app_handle,
                job_id,
                &run_id,
                &handle.tab_id,
            );
        }
        Err(e) => {
            log::error!("Cron agent spawn failed: job={}, error={}", job_id, e);

            let fresh_transition =
                finish_run(cron_state, job_id, &run_id, CronRunStatus::Failed, None)
                    .unwrap_or(false);

            service.log_system_notice(format!("Cron failed: {}", job_id), Some(e), Some("error"));
            if fresh_transition {
                crate::cron::notify::notify_on_consecutive_failure(service, cron_state, job_id);
            }
        }
    }
    FireStart::Recorded
}

/// Records the run start before any dispatch. Only a recorded start dispatches: a
/// refused or unconfirmed record is never promoted to a launched run.
pub(crate) fn record_fire_start(cron_state: &CronState, job_id: &str, run_id: &str) -> FireStart {
    let run = CronRunInfo {
        run_id: run_id.to_string(),
        started_at: cron_state.lock().now_ms(),
        finished_at: None,
        status: CronRunStatus::Running,
        tab_id: None,
        session_id: None,
        cwd: None,
        summary: None,
        report_delivery: None,
    };
    match commit(cron_state, |store| {
        store.prepare_record_run(job_id, run.clone())
    }) {
        Ok(_) => FireStart::Recorded,
        Err(error) => {
            if error.applied() {
                log::error!("Failed to record cron run: {}", error);
                return FireStart::Recorded;
            }
            if scheduler_unavailable(cron_state) {
                return FireStart::Refused;
            }
            log::error!("Failed to record cron run: {}", error);
            if matches!(
                &error,
                super::store::CronCommitError::Fenced(_)
                    | super::store::CronCommitError::Unconfirmed(_)
            ) {
                FireStart::Fenced
            } else {
                FireStart::Refused
            }
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {

    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    };
    use std::time::Duration;

    use parking_lot::Mutex;

    use crate::cron::clock::{Clock, MockClock, SystemClock};
    use crate::cron::store::{
        commit, compute_next_run, CronCommitError, CronCreateParams, CronStorageOptions, CronStore,
        CronUpdateParams,
    };
    use crate::storage::{Storage, StorageConfig};

    const TEST_DEADLINE: Duration = Duration::from_secs(3);
    const TEST_START: u64 = 1_700_000_000_000;

    struct StorageFixture {
        cron: super::CronState,
        storage: Storage,
        clock: Arc<MockClock>,
        _temp: tempfile::TempDir,
    }

    fn storage_fixture() -> StorageFixture {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().join("instance"), StorageConfig::default())
            .expect("open typed storage");
        let clock = MockClock::starting_at(TEST_START);
        let cron = CronStore::open_storage(
            &storage,
            temp.path().join("crons"),
            clock.clone(),
            CronStorageOptions::default(),
        )
        .expect("hydrate cron store");
        StorageFixture {
            cron: Arc::new(Mutex::new(cron)),
            storage,
            clock,
            _temp: temp,
        }
    }

    fn create_due_job(fixture: &StorageFixture) -> String {
        let params: CronCreateParams = serde_json::from_value(serde_json::json!({
            "name": "Due",
            "workspace_id": "workspace",
            "schedule": "* * * * *",
            "task": "run once",
            "agent": "claude",
            "jitter_max_ms": 0
        }))
        .unwrap();
        let job = commit(&fixture.cron, |store| store.prepare_create(params.clone()))
            .expect("create cron job")
            .value;
        fixture
            .clock
            .set_ms(job.next_run.expect("enabled job schedule"));
        job.id
    }

    #[test]
    fn recurring_slot_missed_past_stale_window_moves_to_next_slot_without_firing() {
        let fixture = storage_fixture();
        let job_id = create_due_job(&fixture);
        let slot = fixture
            .cron
            .lock()
            .get(&job_id)
            .and_then(|job| job.next_run);
        let late = slot.expect("due slot") + super::STALE_FIRE_MS + 60_000;
        fixture.clock.set_ms(late);
        let fired = AtomicUsize::new(0);
        super::tick_with(&fixture.cron, |_| {
            fired.fetch_add(1, Ordering::SeqCst);
            super::FireStart::Recorded
        });
        assert_eq!(fired.load(Ordering::SeqCst), 0);
        let next = fixture
            .cron
            .lock()
            .get(&job_id)
            .and_then(|job| job.next_run);
        assert!(next.is_some_and(|next| next > late));
        fixture.storage.shutdown().wait(TEST_DEADLINE).unwrap();
    }

    #[test]
    fn recurring_slot_late_inside_stale_window_still_fires() {
        let fixture = storage_fixture();
        let job_id = create_due_job(&fixture);
        let slot = fixture
            .cron
            .lock()
            .get(&job_id)
            .and_then(|job| job.next_run);
        fixture.clock.set_ms(slot.expect("due slot") + 60_000);
        let fired = AtomicUsize::new(0);
        super::tick_with(&fixture.cron, |spec| {
            fired.fetch_add(1, Ordering::SeqCst);
            super::record_fire_start(&fixture.cron, &spec.job_id, "late-run")
        });
        assert_eq!(fired.load(Ordering::SeqCst), 1);
        fixture.storage.shutdown().wait(TEST_DEADLINE).unwrap();
    }

    #[test]
    fn test_compute_next_fire_soonest() {
        let next = compute_next_run("* * * * *").unwrap().unwrap();
        let now_ms = SystemClock.now_ms();
        assert!(next > now_ms);
        assert!(next - now_ms <= 60_000 + 1000);
    }

    #[test]
    fn test_disabled_not_in_enabled_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        let mut store = CronStore::new_with_path(path);

        store
            .create(CronCreateParams {
                name: "Enabled".into(),
                workspace_id: "ws".into(),
                team_id: None,
                detach_from_team: false,
                schedule: "0 * * * *".into(),
                task: "test".into(),
                description: None,
                agent: None,
                model: None,
                reasoning_effort: None,
                cwd: None,
                role: None,
                skill: None,
                creator_tab_id: None,
                report_target_tab_id: None,
                enabled: Some(true),
                max_history: None,
                oneshot: None,
                jitter_max_ms: Some(0),
                tz: None,
                timeout_seconds: None,
            })
            .unwrap();

        store
            .create(CronCreateParams {
                name: "Disabled".into(),
                workspace_id: "ws".into(),
                team_id: None,
                detach_from_team: false,
                schedule: "0 * * * *".into(),
                task: "test".into(),
                description: None,
                agent: None,
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
            .unwrap();

        let enabled = store.list_enabled();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "Enabled");
    }

    #[test]
    fn cron_old_job_without_reasoning_effort_fires_at_provider_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        let mut store = CronStore::new_with_path(path.clone());
        let job = store
            .create(CronCreateParams {
                name: "legacy".into(),
                workspace_id: "ws".into(),
                team_id: None,
                detach_from_team: false,
                schedule: "0 * * * *".into(),
                task: "run once".into(),
                description: None,
                agent: Some("tiered".into()),
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
            .unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("reasoning_effort"));

        let loaded = CronStore::new_with_path(path);
        let fire = super::fire_spec(loaded.get(&job.id).unwrap());
        assert_eq!(fire.reasoning_effort, None);

        let spec = crate::cron::spawn_spec::build_cron_spawn_spec(
            &crate::chatbackend::ChatBackendRegistry::new(),
            crate::cron::spawn_spec::CronSpawnInputs {
                job_id: fire.job_id,
                run_id: "fixture-run".into(),
                job_name: "legacy".into(),
                agent: fire.agent,
                model: fire.model,
                reasoning_effort: fire.reasoning_effort,
                task_text: fire.task,
                role: fire.role,
                cwd: fire.cwd,
                workspace_id: Some(fire.workspace_id),
                team_id: fire.team_id,
                creator_tab_id: fire.creator_tab_id,
                report_target_tab_id: fire.report_target_tab_id,
            },
        );
        match spec.command {
            crate::service::spawn::TabCommand::Agent(agent) => {
                assert_eq!(agent.reasoning_effort, None)
            }
            other => panic!("expected PTY agent spawn, got {other:?}"),
        }
    }

    #[test]
    fn cron_fence_scheduler_waits_for_settlement_event_and_records_once() {
        let fixture = storage_fixture();
        let job_id = create_due_job(&fixture);
        let (settled_tx, settled_rx) = mpsc::channel();
        fixture.cron.lock().set_fence_notifier(move || {
            let _ = settled_tx.send(());
        });
        let hold = fixture.storage.hold_writer().expect("hold writer");
        hold.wait_held(TEST_DEADLINE).expect("writer hold reached");
        fixture
            .cron
            .lock()
            .set_storage_deadline(Duration::from_millis(1));
        let pending = commit(&fixture.cron, |store| {
            store.prepare_update(
                &job_id,
                serde_json::from_value::<CronUpdateParams>(serde_json::json!({
                    "name": "Pending rename"
                }))
                .unwrap(),
            )
        });
        assert!(matches!(pending, Err(CronCommitError::Unconfirmed(_))));

        let attempts = Arc::new(AtomicUsize::new(0));
        let first_attempts = attempts.clone();
        assert_eq!(
            super::tick_with(&fixture.cron, |spec| {
                first_attempts.fetch_add(1, Ordering::SeqCst);
                super::record_fire_start(&fixture.cron, &spec.job_id, "settled-run")
            }),
            super::TickWait::Settlement
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 1);

        fixture
            .cron
            .lock()
            .set_storage_deadline(CronStorageOptions::default().deadline);
        hold.release();
        settled_rx
            .recv_timeout(TEST_DEADLINE)
            .expect("settlement wakes the scheduler signal");
        let second_attempts = attempts.clone();
        super::tick_with(&fixture.cron, |spec| {
            second_attempts.fetch_add(1, Ordering::SeqCst);
            super::record_fire_start(&fixture.cron, &spec.job_id, "settled-run")
        });

        let store = fixture.cron.lock();
        let run = store
            .get(&job_id)
            .and_then(|job| job.last_run.as_ref())
            .expect("run start published");
        assert_eq!(run.run_id, "settled-run");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        drop(store);
        fixture.storage.shutdown().wait(TEST_DEADLINE).unwrap();
    }

    #[test]
    fn cron_fence_closed_storage_marks_scheduler_unavailable_without_retry() {
        let fixture = storage_fixture();
        let job_id = create_due_job(&fixture);
        fixture.storage.shutdown().wait(TEST_DEADLINE).unwrap();
        let attempts = Arc::new(AtomicUsize::new(0));
        let first_attempts = attempts.clone();

        assert_eq!(
            super::tick_with(&fixture.cron, |spec| {
                first_attempts.fetch_add(1, Ordering::SeqCst);
                super::record_fire_start(&fixture.cron, &spec.job_id, "closed-run")
            }),
            super::TickWait::Unavailable
        );
        let second_attempts = attempts.clone();
        assert_eq!(
            super::tick_with(&fixture.cron, |spec| {
                second_attempts.fetch_add(1, Ordering::SeqCst);
                super::record_fire_start(&fixture.cron, &spec.job_id, "closed-run")
            }),
            super::TickWait::Unavailable
        );

        let store = fixture.cron.lock();
        assert!(store.scheduler_unavailable());
        assert!(store.get(&job_id).unwrap().last_run.is_none());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}

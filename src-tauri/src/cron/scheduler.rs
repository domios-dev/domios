//! Background cron scheduler — fires cron jobs and spawns agents.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use tauri::Emitter;

use super::store::{CronRunInfo, CronRunStatus, CronStore};
use crate::service::{SpawnAgentParams, ServiceState};

pub type CronState = Arc<Mutex<CronStore>>;

/// Notification channel to wake the scheduler when cron jobs are modified.
pub type CronWake = Arc<std::sync::Condvar>;

/// Start the cron scheduler in a background thread.
pub fn start(
    cron_state: CronState,
    cron_wake: CronWake,
    service: ServiceState,
) {
    let wake_mutex = Arc::new(std::sync::Mutex::new(()));

    std::thread::spawn(move || {
        log::info!("Cron scheduler started");
        loop {
            let sleep_ms = tick(&cron_state, &service);
            let duration = Duration::from_millis(sleep_ms);

            // Sleep until next fire time or until woken by CRUD operation
            let guard = wake_mutex.lock().unwrap();
            let _ = cron_wake.wait_timeout(guard, duration);
        }
    });
}

/// One scheduler tick: fire any overdue jobs, return ms until next fire.
fn tick(cron_state: &CronState, service: &ServiceState) -> u64 {
    let now_ms = now();
    let mut next_fire: Option<u64> = None;
    let mut to_fire: Vec<(String, String, String, Option<String>, Option<String>, String, Option<String>)> = Vec::new();

    // Collect jobs that need firing
    {
        let store = cron_state.lock();
        for job in store.list_enabled() {
            if let Some(nr) = job.next_run {
                if nr <= now_ms {
                    to_fire.push((
                        job.id.clone(),
                        job.agent.clone(),
                        job.task.clone(),
                        job.cwd.clone(),
                        job.role.clone(),
                        job.workspace_id.clone(),
                        job.creator_pane_id.clone(),
                    ));
                } else {
                    next_fire = Some(match next_fire {
                        Some(current) => current.min(nr),
                        None => nr,
                    });
                }
            }
        }
    }

    // Fire jobs
    let fired_any = !to_fire.is_empty();
    for (job_id, agent, task, cwd, role, workspace_id, creator_pane_id) in to_fire {
        fire_job(cron_state, service, &job_id, &agent, &task, cwd.as_deref(), role.as_deref(), &workspace_id, creator_pane_id.as_deref());
    }

    // Recalculate next fire after firing
    if fired_any {
        let store = cron_state.lock();
        for job in store.list_enabled() {
            if let Some(nr) = job.next_run {
                next_fire = Some(match next_fire {
                    Some(current) => current.min(nr),
                    None => nr,
                });
            }
        }
    }

    // Sleep duration: min of (next_fire - now, 60s)
    match next_fire {
        Some(nr) => {
            let now_ms = now();
            if nr > now_ms {
                (nr - now_ms).min(60_000)
            } else {
                100 // Fire immediately on next tick
            }
        }
        None => 60_000, // No jobs, check every 60s
    }
}

fn fire_job(
    cron_state: &CronState,
    service: &ServiceState,
    job_id: &str,
    agent: &str,
    task: &str,
    cwd: Option<&str>,
    role: Option<&str>,
    workspace_id: &str,
    creator_pane_id: Option<&str>,
) {
    let run_id = uuid::Uuid::new_v4().to_string();
    log::info!("Cron fired: job={}, run={}", job_id, run_id);

    // Record run as Running
    let run_info = CronRunInfo {
        run_id: run_id.clone(),
        started_at: now(),
        finished_at: None,
        status: CronRunStatus::Running,
        pane_id: None,
        tab_id: None,
    };
    {
        let mut store = cron_state.lock();
        if let Err(e) = store.record_run(job_id, run_info) {
            log::error!("Failed to record cron run: {}", e);
        }
    }

    // Spawn agent
    let mut task_text = task.to_string();
    if creator_pane_id.is_some() {
        task_text.push_str(crate::prompts::CRON_REPORT_HINT);
    }
    task_text.push_str(&crate::prompts::CRON_COMPLETE_SUFFIX.replace("{}", &run_id));
    // Pass dreaming model from config
    let model_args = {
        let mem_cfg = crate::config::memory_config();
        if !mem_cfg.dreaming_model.is_empty() {
            let args = service.agent_registry
                .with_provider(agent, |p| p.model_launch_args(&mem_cfg.dreaming_model))
                .unwrap_or_default();
            if args.is_empty() { None } else { Some(args) }
        } else {
            None
        }
    };

    let params = SpawnAgentParams {
        agent: Some(agent.to_string()),
        agent_name: None,
        args: model_args,
        cwd: cwd.map(|s| s.to_string()),
        title: None,
        role: role.map(|s| s.to_string()),
        parent_pane_id: None,
        can_spawn: None,
        task: Some(task_text),
        workspace_id: if workspace_id.is_empty() { None } else { Some(workspace_id.to_string()) },
        force_tab: Some(true),
        report_target_pane_id: creator_pane_id.map(|s| s.to_string()),
        skip_permissions: None,
    };

    match service.spawn_agent_sync(params) {
        Ok(result) => {
            log::info!(
                "Cron agent spawned: job={}, pane={}, tab={}",
                job_id, result.pane_id, result.tab_id
            );

            // Update run with pane/tab info + register ownership
            {
                let mut store = cron_state.lock();
                store.register_pane(&result.pane_id, job_id, &run_id);
                if let Some(history) = store.get_history_mut(job_id) {
                    if let Some(run) = history.iter_mut().find(|r| r.run_id == run_id) {
                        run.pane_id = Some(result.pane_id.clone());
                        run.tab_id = Some(result.tab_id.clone());
                    }
                }
                // Also update last_run
                if let Some(job) = store.get_mut(job_id) {
                    if let Some(ref mut lr) = job.last_run {
                        if lr.run_id == run_id {
                            lr.pane_id = Some(result.pane_id.clone());
                            lr.tab_id = Some(result.tab_id.clone());
                        }
                    }
                }
                let _ = store.save();
            }

            // Emit event
            let _ = service.app_handle.emit(
                "cron-fired",
                serde_json::json!({
                    "cron_id": job_id,
                    "run_id": run_id,
                    "pane_id": result.pane_id,
                    "tab_id": result.tab_id,
                }),
            );

        }
        Err(e) => {
            log::error!("Cron agent spawn failed: job={}, error={}", job_id, e);

            // Mark run as failed
            {
                let mut store = cron_state.lock();
                let _ = store.update_run_status(job_id, &run_id, CronRunStatus::Failed);
            }

            // Notification
            service.create_notification(
                format!("Cron failed: {}", job_id),
                Some(e),
                Some("error"),
            );
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cron::store::{CronCreateParams, CronStore, compute_next_run};

    #[test]
    fn test_compute_next_fire_soonest() {
        // Test that compute_next_run returns a future time
        let next = compute_next_run("* * * * *").unwrap().unwrap();
        let now_ms = now();
        assert!(next > now_ms);
        assert!(next - now_ms <= 60_000 + 1000);
    }

    #[test]
    fn test_disabled_not_in_enabled_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        let mut store = CronStore::new_with_path(path);

        store.create(CronCreateParams {
            name: "Enabled".into(),
            workspace_id: "ws".into(),
            schedule: "0 * * * *".into(),
            task: "test".into(),
            description: None,
            agent: None,
            cwd: None,
            role: None,
            creator_pane_id: None,
            enabled: Some(true),
            max_history: None,
            oneshot: None,
            jitter_max_ms: Some(0),
        }).unwrap();

        store.create(CronCreateParams {
            name: "Disabled".into(),
            workspace_id: "ws".into(),
            schedule: "0 * * * *".into(),
            task: "test".into(),
            description: None,
            agent: None,
            cwd: None,
            role: None,
            creator_pane_id: None,
            enabled: Some(false),
            max_history: None,
            oneshot: None,
            jitter_max_ms: Some(0),
        }).unwrap();

        let enabled = store.list_enabled();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "Enabled");
    }
}

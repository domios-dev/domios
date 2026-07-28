//! Background cron scheduler — fires cron jobs and spawns agents.
//!
//! Time comes exclusively from the store's injected Clock, never from
//! SystemTime directly — that's what makes cron time-travel testable.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tauri::Emitter;

use super::store::{CronRunInfo, CronRunStatus, CronStore};
use crate::service::ServiceState;

pub type CronState = Arc<Mutex<CronStore>>;

/// Notification channel to wake the scheduler when cron jobs are modified.
pub type CronWake = Arc<std::sync::Condvar>;

/// Start the cron scheduler in a background thread.
pub fn start(cron_state: CronState, cron_wake: CronWake, service: ServiceState) {
    let wake_mutex = Arc::new(std::sync::Mutex::new(()));

    std::thread::spawn(move || {
        log::info!("Cron scheduler started");
        loop {
            let sleep_ms = tick(&cron_state, &service);
            let duration = Duration::from_millis(sleep_ms);

            let guard = wake_mutex.lock().unwrap();
            let _ = cron_wake.wait_timeout(guard, duration);
        }
    });
}

struct FireSpec {
    job_id: String,
    agent: String,
    model: Option<String>,
    task: String,
    cwd: Option<String>,
    role: Option<String>,
    workspace_id: String,
    creator_tab_id: Option<String>,
    timeout_seconds: Option<u64>,
    skill: Option<String>,
}

/// One scheduler tick: fire any overdue jobs, return ms until next fire.
fn tick(cron_state: &CronState, service: &ServiceState) -> u64 {
    let now_ms = cron_state.lock().now_ms();
    let mut next_fire: Option<u64> = None;
    let mut to_fire: Vec<FireSpec> = Vec::new();

    {
        let store = cron_state.lock();
        for job in store.list_enabled() {
            if let Some(nr) = job.next_run {
                if nr <= now_ms {
                    if store.is_job_running(&job.id) {
                        log::info!("[cron] skip {}: previous run still active", job.id);
                        let recheck = now_ms + 30_000;
                        next_fire = Some(match next_fire {
                            Some(current) => current.min(recheck),
                            None => recheck,
                        });
                        continue;
                    }
                    to_fire.push(FireSpec {
                        job_id: job.id.clone(),
                        agent: job.agent.clone(),
                        model: job.model.clone(),
                        task: job.task.clone(),
                        cwd: job.cwd.clone(),
                        role: job.role.clone(),
                        workspace_id: job.workspace_id.clone(),
                        creator_tab_id: job.creator_tab_id.clone(),
                        timeout_seconds: job.timeout_seconds,
                        skill: job.skill.clone(),
                    });
                } else {
                    next_fire = Some(match next_fire {
                        Some(current) => current.min(nr),
                        None => nr,
                    });
                }
            }
        }
    }

    let fired_any = !to_fire.is_empty();
    for spec in to_fire {
        fire_job(cron_state, service, &spec);
    }

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
            let now_ms = cron_state.lock().now_ms();
            if nr > now_ms {
                (nr - now_ms).min(60_000)
            } else {
                100 // Fire immediately on next tick
            }
        }
        None => 60_000, // No jobs, check every 60s
    }
}

fn fire_job(cron_state: &CronState, service: &ServiceState, spec: &FireSpec) {
    let job_id = spec.job_id.as_str();
    let agent = spec.agent.as_str();
    let model = spec.model.as_deref();
    let task = spec.task.as_str();
    let cwd = spec.cwd.as_deref();
    let role = spec.role.as_deref();
    let workspace_id = spec.workspace_id.as_str();
    let creator_tab_id = spec.creator_tab_id.as_deref();
    let job_timeout = spec.timeout_seconds;

    let run_id = uuid::Uuid::new_v4().to_string();
    log::info!("Cron fired: job={}, run={}", job_id, run_id);

    let run_info = CronRunInfo {
        run_id: run_id.clone(),
        started_at: cron_state.lock().now_ms(),
        finished_at: None,
        status: CronRunStatus::Running,
        tab_id: None,
        session_id: None,
        cwd: None,
    };
    {
        let mut store = cron_state.lock();
        if let Err(e) = store.record_run(job_id, run_info) {
            log::error!("Failed to record cron run: {}", e);
        }
    }

    // Spawn agent via the unified spawn_tab surface.
    use crate::service::spawn::{AgentSpawnSpec, SpawnTabSpec, TabCommand, TabVisibility};

    let skill_block = spec.skill.as_deref().map(crate::commands::cron_skill_block);
    let task_text = crate::cron::task_text::build_cron_task_text(
        task,
        skill_block.as_deref(),
        creator_tab_id,
        &run_id,
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
    let tab_title = format!("cron:{}", job_name);

    let spec = SpawnTabSpec {
        workspace_id: if workspace_id.is_empty() {
            None
        } else {
            Some(workspace_id.to_string())
        },
        parent_tab_id: None,
        cwd: cwd.map(|s| s.to_string()),
        title: Some(tab_title),
        description: None,
        // Hidden — user observes cron runs via the cron history log viewer, not as a live tab. Mirrors the mem worker pattern.
        visibility: TabVisibility::Hidden,
        eager: false,
        command: TabCommand::Agent(AgentSpawnSpec {
            provider: agent.to_string(),
            planned_model: None,
            model,
            task: Some(task_text),
            role: role.map(|s| s.to_string()),
            auto_approve: true,
            report_target_tab_id: creator_tab_id.map(|s| s.to_string()),
            spawned_by: Some(format!("cron:{}", job_id)),
            ..Default::default()
        }),
    };

    match service.spawn_tab(spec) {
        Ok(handle) => {
            log::info!(
                "Cron agent spawned: job={}, tab={}, tab={}",
                job_id,
                handle.tab_id,
                handle.tab_id
            );

            {
                let mut store = cron_state.lock();
                store.register_tab(&handle.tab_id, job_id, &run_id);
                if let Some(history) = store.get_history_mut(job_id) {
                    if let Some(run) = history.iter_mut().find(|r| r.run_id == run_id) {
                        run.tab_id = Some(handle.tab_id.clone());
                        run.tab_id = Some(handle.tab_id.clone());
                    }
                }
                if let Some(job) = store.get_mut(job_id) {
                    if let Some(ref mut lr) = job.last_run {
                        if lr.run_id == run_id {
                            lr.tab_id = Some(handle.tab_id.clone());
                            lr.tab_id = Some(handle.tab_id.clone());
                        }
                    }
                }
                let _ = store.save();
            }

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
                );
            }

            let _ = service.app_handle.emit(
                "cron-fired",
                serde_json::json!({
                    "cron_id": job_id,
                    "run_id": run_id,
                    "tab_id": handle.tab_id,
                    "tab_id": handle.tab_id,
                }),
            );
        }
        Err(e) => {
            log::error!("Cron agent spawn failed: job={}, error={}", job_id, e);

            let fresh_transition = {
                let mut store = cron_state.lock();
                store
                    .update_run_status(job_id, &run_id, CronRunStatus::Failed)
                    .unwrap_or(false)
            };

            service.create_notification(format!("Cron failed: {}", job_id), Some(e), Some("error"));
            if fresh_transition {
                crate::cron::notify::notify_on_consecutive_failure(service, cron_state, job_id);
            }
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {

    use crate::cron::clock::{Clock, SystemClock};
    use crate::cron::store::{compute_next_run, CronCreateParams, CronStore};

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
                schedule: "0 * * * *".into(),
                task: "test".into(),
                description: None,
                agent: None,
                model: None,
                cwd: None,
                role: None,
                skill: None,
                creator_tab_id: None,
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
                schedule: "0 * * * *".into(),
                task: "test".into(),
                description: None,
                agent: None,
                model: None,
                cwd: None,
                role: None,
                skill: None,
                creator_tab_id: None,
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
}

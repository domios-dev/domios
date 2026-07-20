use super::ServiceContext;
use crate::cron::store::{CronCreateParams, CronJob, CronRunInfo, CronRunStatus, CronUpdateParams};
use crate::CronState;
use crate::CronWake;

impl ServiceContext {
    pub fn cron_create(
        &self,
        cron: &CronState,
        wake: &CronWake,
        params: CronCreateParams,
    ) -> Result<CronJob, String> {
        let job = cron.lock().create(params)?;
        wake.notify_all();
        self.emit_cron_changed();
        Ok(job)
    }

    pub fn cron_list(&self, cron: &CronState, workspace_id: Option<&str>) -> Vec<CronJob> {
        let store = cron.lock();
        match workspace_id {
            Some(ws) => store.list_for_workspace(ws).into_iter().cloned().collect(),
            None => store.list().to_vec(),
        }
    }

    pub fn cron_get(&self, cron: &CronState, id: &str) -> Result<CronJob, String> {
        cron.lock()
            .get(id)
            .cloned()
            .ok_or_else(|| format!("Cron job not found: {}", id))
    }

    pub fn cron_update(
        &self,
        cron: &CronState,
        wake: &CronWake,
        id: &str,
        patch: CronUpdateParams,
    ) -> Result<CronJob, String> {
        let job = cron.lock().update(id, patch)?;
        wake.notify_all();
        self.emit_cron_changed();
        Ok(job)
    }

    pub fn cron_delete(&self, cron: &CronState, wake: &CronWake, id: &str) -> Result<(), String> {
        let mut store = cron.lock();
        let owned_panes: Vec<String> = store.pane_owners_for_job(id);
        let history_panes: Vec<String> = store
            .get_history(id)
            .iter()
            .filter_map(|r| r.pane_id.clone())
            .collect();
        let all_panes: std::collections::HashSet<String> =
            owned_panes.into_iter().chain(history_panes).collect();
        for pane_id in &all_panes {
            store.unregister_pane(pane_id);
            let _ = self.close_pane(pane_id);
        }

        let logs_dir = store.logs_dir().join(id);
        drop(store);
        let _ = std::fs::remove_dir_all(&logs_dir);

        cron.lock().delete(id)?;
        wake.notify_all();
        self.emit_cron_changed();
        Ok(())
    }

    pub fn cron_cancel_running(&self, cron: &CronState, id: &str) -> Result<usize, String> {
        let to_close: Vec<String> =
            {
                let mut store = cron.lock();
                if store.get(id).is_none() {
                    return Err(format!("Cron job not found: {}", id));
                }
                let panes = store.pane_owners_for_job(id);
                for pane_id in &panes {
                    store.unregister_pane(pane_id);
                }
                if let Some(history) = store.get_history_mut(id) {
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    for run in history.iter_mut() {
                        if matches!(
                            run.status,
                            super::super::cron::store::CronRunStatus::Running
                        ) {
                            run.status = super::super::cron::store::CronRunStatus::Cancelled;
                            run.finished_at = Some(now_ms);
                            // Observability parity with update_run_status — see crud::update_run_status.
                            log::info!(
                            "Cron run completed: job={}, run={}, status=Cancelled, elapsed_ms={}",
                            id, run.run_id, now_ms.saturating_sub(run.started_at)
                        );
                        }
                    }
                }
                let _ = store.save();
                panes
            };

        let count = to_close.len();
        for pane_id in &to_close {
            let _ = self.close_pane(pane_id);
        }
        self.emit_cron_changed();
        Ok(count)
    }

    pub fn cron_enable(
        &self,
        cron: &CronState,
        wake: &CronWake,
        id: &str,
        enabled: bool,
    ) -> Result<CronJob, String> {
        let job = cron.lock().set_enabled(id, enabled)?;
        wake.notify_all();
        self.emit_cron_changed();
        Ok(job)
    }

    pub fn cron_trigger(&self, cron: &CronState, id: &str) -> Result<CronRunInfo, String> {
        use crate::service::spawn::{AgentSpawnSpec, PaneCommand, PaneVisibility, SpawnPaneSpec};

        let (
            agent,
            model,
            task,
            cwd,
            role,
            skill,
            workspace_id,
            creator_pane_id,
            job_name,
            job_timeout,
        ) = {
            let store = cron.lock();
            let job = store
                .get(id)
                .ok_or_else(|| format!("Cron job not found: {}", id))?;
            if store.is_job_running(id) {
                return Err(format!("Cron job '{}' is already running", job.name));
            }
            (
                job.agent.clone(),
                job.model.clone(),
                job.task.clone(),
                job.cwd.clone(),
                job.role.clone(),
                job.skill.clone(),
                job.workspace_id.clone(),
                job.creator_pane_id.clone(),
                job.name.clone(),
                job.timeout_seconds,
            )
        };

        let run_id = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut run_info = CronRunInfo {
            run_id: run_id.clone(),
            started_at: now,
            finished_at: None,
            status: CronRunStatus::Running,
            pane_id: None,
            tab_id: None,
            session_id: None,
            cwd: None,
        };

        let skill_block = skill.as_deref().map(crate::commands::cron_skill_block);
        let task_text = crate::cron::task_text::build_cron_task_text(
            &task,
            skill_block.as_deref(),
            creator_pane_id.as_deref(),
            &run_id,
        );

        let model = model
            .or_else(|| {
                let mem_cfg = crate::config::memory_config();
                if !mem_cfg.dreaming_model.is_empty() {
                    Some(mem_cfg.dreaming_model.clone())
                } else if !mem_cfg.worker_model.is_empty() {
                    Some(mem_cfg.worker_model.clone())
                } else {
                    None
                }
            })
            .map(|s| crate::agent::models::canonicalize_short(&s).to_string());

        let spec = SpawnPaneSpec {
            workspace_id: if workspace_id.is_empty() {
                None
            } else {
                Some(workspace_id)
            },
            parent_pane_id: None,
            cwd,
            title: Some(format!("cron:{}", job_name)),
            description: None,
            visibility: PaneVisibility::Hidden,
            eager: false,
            command: PaneCommand::Agent(AgentSpawnSpec {
                provider: agent,
                planned_model: None,
                model,
                task: Some(task_text),
                role,
                auto_approve: true,
                report_target_pane_id: creator_pane_id,
                spawned_by: Some(format!("cron:{}", id)),
                ..Default::default()
            }),
        };

        match self.spawn_pane(spec) {
            Ok(handle) => {
                run_info.pane_id = Some(handle.pane_id.clone());
                run_info.tab_id = Some(handle.tab_id.clone());
                cron.lock().register_pane(&handle.pane_id, id, &run_id);
                // Register with the auto-complete watcher (cron flavor).
                if let Some(pty_id) = self.agent_run_pty_map.lock().get(&handle.pane_id).cloned() {
                    let global_timeout = crate::config::memory_config().worker_timeout_seconds;
                    let timeout = job_timeout.unwrap_or(global_timeout);
                    if job_timeout.is_some() {
                        log::info!(
                            "[cron] manual trigger {} run={} timeout={}s (per-job override)",
                            id,
                            run_id,
                            timeout
                        );
                    }
                    self.run_watcher.register(
                        self.clone(),
                        pty_id,
                        handle.pane_id.clone(),
                        crate::agent::run_watcher::AgentRunKind::Cron {
                            job_id: id.to_string(),
                            run_id: run_id.clone(),
                        },
                        timeout,
                    );
                }
            }
            Err(e) => {
                run_info.status = CronRunStatus::Failed;
                run_info.finished_at = Some(now);
                let mut store = cron.lock();
                let _ = store.record_run(id, run_info.clone());
                return Err(e);
            }
        }

        let mut store = cron.lock();
        let _ = store.record_run(id, run_info.clone());
        self.emit_cron_changed();
        Ok(run_info)
    }

    pub fn cron_complete(
        &self,
        cron: &CronState,
        wake: &CronWake,
        run_id: &str,
        summary: Option<&str>,
    ) -> Result<(), String> {
        let mut store = cron.lock();
        let owner = store
            .get_pane_owner_by_run(run_id)
            .map(|(j, p)| (j.to_string(), p.to_string()));
        if let Some((job_id, pane_id)) = owner {
            let (pty_id, session_id, run_cwd) = {
                let mux = self.mux.write();
                let pane = mux.get_pane(&pane_id);
                (
                    pane.and_then(|p| p.pty_id.clone()),
                    pane.and_then(|p| p.agent_session_id.clone()),
                    pane.and_then(|p| p.cwd.clone()),
                )
            };
            if let Some(ref sid) = session_id {
                store.set_run_session(&job_id, run_id, sid, run_cwd.as_deref());
            }
            let raw = if let Some(ref pid) = pty_id {
                self.output_buf.lock().get_text(pid).unwrap_or_default()
            } else {
                String::new()
            };
            let raw_clean = crate::pty::ansi::clean_terminal_output(&raw);

            let summary_text = summary.unwrap_or("(no summary provided)");
            let log_text = format!(
                "--- SUMMARY ---\n{}\n\n--- RAW OUTPUT ---\n{}",
                summary_text, raw_clean
            );

            if let Err(e) = store.save_log(&job_id, run_id, &log_text) {
                log::error!("Failed to save cron log: {}", e);
            }

            let _ = store.update_run_status(&job_id, run_id, CronRunStatus::Success);
            let is_oneshot = store.get(&job_id).map(|j| j.oneshot).unwrap_or(false);
            store.unregister_pane(&pane_id);
            log::info!(
                "Cron run completed: job={}, run={}, oneshot={}, summary_len={}",
                job_id,
                run_id,
                is_oneshot,
                log_text.len()
            );
            drop(store);

            // Tell the run-watcher this pane is already done so the idle / hard timeout watchdog does NOT fire later and overwrite our Success status with Failed.
            if let Some(ref pid) = pty_id {
                self.run_watcher.mark_completed_externally(pid);
            }

            // The `codeterm cron complete` CLI client that drove this request is a
            // child of the pane we're about to kill. Killing it before the caller
            // writes the JSON-RPC response would break the pipe mid-write, which
            // interprocess escalates to std::process::abort(). Defer the teardown
            // so the response reaches the live client and it disconnects first.
            let svc = self.clone();
            let cron = cron.clone();
            let wake = wake.clone();
            let pane_id = pane_id.clone();
            let job_id = job_id.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let _ = svc.close_pane(&pane_id);
                if is_oneshot {
                    log::info!("Oneshot cron auto-deleting: job={}", job_id);
                    if let Err(e) = svc.cron_delete(&cron, &wake, &job_id) {
                        log::error!("Failed to auto-delete oneshot cron: {}", e);
                    }
                }
                svc.emit_cron_changed();
            });
            Ok(())
        } else {
            Err(format!("No cron run found for run_id: {}", run_id))
        }
    }

    pub fn cron_history(&self, cron: &CronState, id: &str) -> Vec<CronRunInfo> {
        cron.lock().get_history(id).into_iter().cloned().collect()
    }

    fn emit_cron_changed(&self) {
        let handle = self.app_handle.clone();
        std::thread::spawn(move || {
            crate::api::events::emit_everywhere(&handle, "cron-run-finished", ());
        });
    }
}

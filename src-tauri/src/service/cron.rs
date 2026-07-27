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
        let owned_tabs: Vec<String> = store.tab_owners_for_job(id);
        let history_tabs: Vec<String> = store
            .get_history(id)
            .iter()
            .filter_map(|r| r.tab_id.clone())
            .collect();
        let all_tabs: std::collections::HashSet<String> =
            owned_tabs.into_iter().chain(history_tabs).collect();
        for tab_id in &all_tabs {
            store.unregister_tab(tab_id);
            let _ = self.close_tab(tab_id);
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
                let tabs = store.tab_owners_for_job(id);
                for tab_id in &tabs {
                    store.unregister_tab(tab_id);
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
                tabs
            };

        let count = to_close.len();
        for tab_id in &to_close {
            let _ = self.close_tab(tab_id);
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
        use crate::service::spawn::{AgentSpawnSpec, TabCommand, TabVisibility, SpawnTabSpec};

        let (
            agent,
            model,
            task,
            cwd,
            role,
            skill,
            workspace_id,
            creator_tab_id,
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
                job.creator_tab_id.clone(),
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
            tab_id: None,
            session_id: None,
            cwd: None,
        };

        let skill_block = skill.as_deref().map(crate::commands::cron_skill_block);
        let task_text = crate::cron::task_text::build_cron_task_text(
            &task,
            skill_block.as_deref(),
            creator_tab_id.as_deref(),
            &run_id,
        );

        let model = model.or_else(|| {
            let mem_cfg = crate::config::memory_config();
            let (resolved, _fell_back) = codeterm_mem::worker_agent::resolve_worker_model(
                &mem_cfg.dreaming_model,
                &mem_cfg.worker_model,
                &agent,
                &crate::service::spawn::CatalogBackgroundModelPort,
            );
            (!resolved.is_empty()).then_some(resolved)
        });

        let spec = SpawnTabSpec {
            workspace_id: if workspace_id.is_empty() {
                None
            } else {
                Some(workspace_id)
            },
            parent_tab_id: None,
            cwd,
            title: Some(format!("cron:{}", job_name)),
            description: None,
            visibility: TabVisibility::Hidden,
            eager: false,
            command: TabCommand::Agent(AgentSpawnSpec {
                provider: agent,
                planned_model: None,
                model,
                task: Some(task_text),
                role,
                auto_approve: true,
                report_target_tab_id: creator_tab_id,
                spawned_by: Some(format!("cron:{}", id)),
                ..Default::default()
            }),
        };

        match self.spawn_tab(spec) {
            Ok(handle) => {
                run_info.tab_id = Some(handle.tab_id.clone());
                run_info.tab_id = Some(handle.tab_id.clone());
                cron.lock().register_tab(&handle.tab_id, id, &run_id);
                // Register with the auto-complete watcher (cron flavor).
                if let Some(pty_id) = self.agent_run_pty_map.lock().get(&handle.tab_id).cloned() {
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
                        handle.tab_id.clone(),
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
            .get_tab_owner_by_run(run_id)
            .map(|(j, p)| (j.to_string(), p.to_string()));
        if let Some((job_id, tab_id)) = owner {
            let (pty_id, session_id, run_cwd) = {
                let mux = self.mux.write();
                let tab = mux.get_tab(&tab_id);
                (
                    tab.and_then(|p| p.pty_id.clone()),
                    tab.and_then(|p| p.agent_session_id.clone()),
                    tab.and_then(|p| p.cwd.clone()),
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
            store.unregister_tab(&tab_id);
            log::info!(
                "Cron run completed: job={}, run={}, oneshot={}, summary_len={}",
                job_id,
                run_id,
                is_oneshot,
                log_text.len()
            );
            drop(store);

            // Tell the run-watcher this tab is already done so the idle / hard timeout watchdog does NOT fire later and overwrite our Success status with Failed.
            if let Some(ref pid) = pty_id {
                self.run_watcher.mark_completed_externally(pid);
            }

            // The `codeterm cron complete` CLI client that drove this request is a
            // child of the tab we're about to kill. Killing it before the caller
            // writes the JSON-RPC response would break the pipe mid-write, which
            // interprocess escalates to std::process::abort(). Defer the teardown
            // so the response reaches the live client and it disconnects first.
            let svc = self.clone();
            let cron = cron.clone();
            let wake = wake.clone();
            let tab_id = tab_id.clone();
            let job_id = job_id.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let _ = svc.close_tab(&tab_id);
                if is_oneshot {
                    log::info!("Oneshot cron auto-deleting: job={}", job_id);
                    if let Err(e) = svc.cron_delete(&cron, &wake, &job_id) {
                        log::error!("Failed to auto-delete oneshot cron: {}", e);
                    }
                }
                svc.emit_cron_changed();
            });
            Ok(())
        } else if let Some((job_id, run)) = store.find_run_by_id(run_id) {
            // The watchdog (idle / hard timeout) already finalized this run and
            // unregistered its tab before this explicit completion arrived —
            // a durable race, not a caller error. The terminal status already
            // recorded stands (first terminal wins); log it so the race stays
            // observable instead of surfacing as an opaque "not found".
            log::warn!(
                "[cron] cron.complete for run={} (job={}) arrived after the run was already finalized as {:?} — treating as idempotent no-op",
                run_id, job_id, run.status
            );
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

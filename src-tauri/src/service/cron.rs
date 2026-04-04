//! Cron job management — ServiceContext methods for CRUD + trigger.

use crate::cron::store::{CronCreateParams, CronJob, CronRunInfo, CronRunStatus, CronUpdateParams};
use crate::CronState;
use crate::CronWake;
use super::ServiceContext;

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

    pub fn cron_delete(
        &self,
        cron: &CronState,
        wake: &CronWake,
        id: &str,
    ) -> Result<(), String> {
        // Cascade: kill all running panes, close their tabs, delete logs
        let mut store = cron.lock();

        // Collect all pane_ids owned by this cron
        let owned_panes: Vec<String> = store.pane_owners_for_job(id);
        // Also collect from history
        let history_panes: Vec<String> = store.get_history(id)
            .iter()
            .filter_map(|r| r.pane_id.clone())
            .collect();

        // Kill PTYs and close panes
        let all_panes: std::collections::HashSet<String> = owned_panes.into_iter()
            .chain(history_panes)
            .collect();
        for pane_id in &all_panes {
            store.unregister_pane(pane_id);
            // Close pane (kills PTY + removes from mux)
            let _ = self.close_pane(pane_id);
        }

        // Delete logs directory
        let logs_dir = store.logs_dir().join(id);
        drop(store); // release lock before fs ops
        let _ = std::fs::remove_dir_all(&logs_dir);

        // Delete cron from store
        cron.lock().delete(id)?;
        wake.notify_all();
        self.emit_cron_changed();
        Ok(())
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

    pub fn cron_trigger(
        &self,
        cron: &CronState,
        id: &str,
    ) -> Result<CronRunInfo, String> {
        let (agent, task, cwd, role, workspace_id, creator_pane_id) = {
            let store = cron.lock();
            let job = store.get(id).ok_or_else(|| format!("Cron job not found: {}", id))?;
            (
                job.agent.clone(),
                job.task.clone(),
                job.cwd.clone(),
                job.role.clone(),
                job.workspace_id.clone(),
                job.creator_pane_id.clone(),
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
        };

        // Spawn agent
        let mut task_text = task;
        if creator_pane_id.is_some() {
            task_text.push_str(crate::prompts::CRON_REPORT_HINT);
        }
        task_text.push_str(&crate::prompts::CRON_COMPLETE_SUFFIX.replace("{}", &run_id));
        let params = super::SpawnAgentParams {
            agent: Some(agent),
            agent_name: None,
            args: None,
            cwd,
            title: None,
            role,
            parent_pane_id: None,
            can_spawn: None,
            task: Some(task_text),
            workspace_id: Some(workspace_id),
            force_tab: Some(true),
            report_target_pane_id: creator_pane_id,
            skip_permissions: None,
        };

        match self.spawn_agent_sync(params) {
            Ok(result) => {
                run_info.pane_id = Some(result.pane_id.clone());
                run_info.tab_id = Some(result.tab_id.clone());
                // Register pane ownership for log capture on exit
                cron.lock().register_pane(&result.pane_id, id, &run_id);
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

    /// Called by the agent when cron task is complete. Saves summary as log and updates status.
    /// Oneshot jobs are automatically deleted after completion.
    pub fn cron_complete(
        &self,
        cron: &CronState,
        wake: &CronWake,
        run_id: &str,
        summary: Option<&str>,
    ) -> Result<(), String> {
        let mut store = cron.lock();
        let owner = store.get_pane_owner_by_run(run_id).map(|(j, p)| (j.to_string(), p.to_string()));
        if let Some((job_id, pane_id)) = owner {
            // Always grab raw terminal output
            let pty_id = {
                let mux = self.mux.lock();
                mux.get_pane(&pane_id).and_then(|p| p.pty_id.clone())
            };
            let raw = if let Some(ref pid) = pty_id {
                self.output_buf.lock().get_text(pid).unwrap_or_default()
            } else {
                String::new()
            };
            let raw_clean = crate::api::routes::clean_terminal_output(&raw);

            // Build log: summary section + raw section separated by marker
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
            log::info!("Cron run completed: job={}, run={}, oneshot={}, summary_len={}", job_id, run_id, is_oneshot, log_text.len());
            drop(store);

            // Close the pane — it's now a log entry, not a live terminal
            let _ = self.close_pane(&pane_id);

            // Oneshot: auto-delete the job after completion
            if is_oneshot {
                log::info!("Oneshot cron auto-deleting: job={}", job_id);
                if let Err(e) = self.cron_delete(cron, wake, &job_id) {
                    log::error!("Failed to auto-delete oneshot cron: {}", e);
                }
            }

            self.emit_cron_changed();
            Ok(())
        } else {
            Err(format!("No cron run found for run_id: {}", run_id))
        }
    }

    pub fn cron_history(&self, cron: &CronState, id: &str) -> Vec<CronRunInfo> {
        cron.lock().get_history(id).into_iter().cloned().collect()
    }

    fn emit_cron_changed(&self) {
        use tauri::Emitter;
        let handle = self.app_handle.clone();
        std::thread::spawn(move || {
            let _ = handle.emit("cron-state-changed", ());
        });
    }
}

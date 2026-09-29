use super::ServiceContext;
use crate::cron::store::{CronCreateParams, CronJob, CronRunInfo, CronRunStatus, CronUpdateParams};
use crate::multiplexer::types::TabCloseReason;
use crate::CronState;
use crate::CronWake;

/// A cron run that has ended, by any path, and still owes its report and oneshot retirement.
pub(crate) struct CronRunEnd<'a> {
    pub(crate) job_id: &'a str,
    pub(crate) run_id: &'a str,
    pub(crate) status: CronRunStatus,
    pub(crate) summary: &'a str,
}

impl ServiceContext {
    pub fn cron_create(
        &self,
        cron: &CronState,
        wake: &CronWake,
        mut params: CronCreateParams,
    ) -> Result<CronJob, String> {
        let inherited_attachment = crate::service::team::resolve_inherited_team_attachment(
            &self.mux.read(),
            params.team_id.as_deref(),
            params.creator_tab_id.as_deref(),
            &params.workspace_id,
            params.detach_from_team,
        )?;
        params.team_id = inherited_attachment.map(|attachment| attachment.team_id);
        self.validate_cron_ownership(&params)?;
        let provider = params
            .agent
            .as_deref()
            .filter(|agent| !agent.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(crate::config::default_agent_provider);
        self.validate_cron_reasoning_effort(
            &provider,
            params.model.as_deref(),
            params.reasoning_effort.as_deref(),
        )?;
        let job = cron.lock().create(params)?;
        wake.notify_all();
        self.emit_cron_changed();
        Ok(job)
    }

    fn validate_cron_ownership(&self, params: &CronCreateParams) -> Result<(), String> {
        let mux = self.mux.read();
        if let Some(target_tab_id) = params.report_target_tab_id.as_deref() {
            mux.get_tab(target_tab_id)
                .ok_or_else(|| format!("Cron report target Tab {target_tab_id} not found"))?;
        }
        match (params.team_id.as_deref(), params.creator_tab_id.as_deref()) {
            (None, _) => Ok(()),
            (Some(team_id), Some(creator_tab_id)) => {
                let creator = mux
                    .get_tab(creator_tab_id)
                    .ok_or_else(|| format!("Cron creator Tab {creator_tab_id} not found"))?;
                if creator.team_id.as_deref() != Some(team_id) {
                    return Err(format!(
                        "Cron creator Tab {creator_tab_id} does not belong to Team {team_id}"
                    ));
                }
                let team_workspace = mux
                    .team_workspace(team_id)
                    .ok_or_else(|| format!("Team {team_id} not found"))?;
                if team_workspace != params.workspace_id {
                    return Err(format!(
                        "Team {team_id} belongs to workspace {}, not {}",
                        team_workspace, params.workspace_id
                    ));
                }
                Ok(())
            }
            (Some(team_id), None) => Err(format!(
                "Team cron {team_id} requires creator_tab_id from a live Team member"
            )),
        }
    }

    fn validate_cron_reasoning_effort(
        &self,
        provider: &str,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<(), String> {
        if effort
            .map(str::trim)
            .filter(|effort| !effort.is_empty())
            .is_none()
        {
            return Ok(());
        }
        self.require_agent_provider_ready(provider)?;
        let model =
            model.map(|model| crate::agent::model_catalog::resolve_model(provider, Some(model)).id);
        crate::service::spawn::validate_agent_reasoning_effort(
            &self.agent_registry,
            provider,
            model.as_deref(),
            effort,
        )
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
        let (provider, model, effort) = {
            let store = cron.lock();
            let job = store
                .get(id)
                .ok_or_else(|| format!("Cron job not found: {}", id))?;
            let provider = patch
                .agent
                .as_deref()
                .filter(|agent| !agent.trim().is_empty())
                .unwrap_or(job.agent.as_str())
                .to_string();
            let effort = patch
                .reasoning_effort
                .clone()
                .or_else(|| job.reasoning_effort.clone());
            let model = patch.model.clone().or_else(|| job.model.clone());
            (provider, model, effort)
        };
        self.validate_cron_reasoning_effort(&provider, model.as_deref(), effort.as_deref())?;
        let job = cron.lock().update(id, patch)?;
        wake.notify_all();
        self.emit_cron_changed();
        Ok(job)
    }

    pub fn cron_delete(&self, cron: &CronState, wake: &CronWake, id: &str) -> Result<(), String> {
        self.cron_delete_inner(cron, wake, id, false)
    }

    fn cron_delete_inner(
        &self,
        cron: &CronState,
        wake: &CronWake,
        id: &str,
        preserve_history: bool,
    ) -> Result<(), String> {
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
            let _ = self.close_tab_with_reason(tab_id, TabCloseReason::Cancelled);
        }

        let logs_dir = store.logs_dir().join(id);
        drop(store);
        let _ = std::fs::remove_dir_all(&logs_dir);

        if preserve_history {
            cron.lock().delete_preserving_history(id)?;
        } else {
            cron.lock().delete(id)?;
        }
        wake.notify_all();
        self.emit_cron_changed();
        Ok(())
    }

    pub(crate) fn cron_settle_run_end(
        &self,
        cron: &CronState,
        wake: Option<&CronWake>,
        end: CronRunEnd<'_>,
    ) {
        self.cron_settle_run_end_with(cron, wake, end, |tab_id, text| self.send_text(tab_id, text));
    }

    /// Delivers the ended run's status to its report tab, records the outcome in history,
    /// and retires a oneshot job while keeping that history.
    pub(crate) fn cron_settle_run_end_with(
        &self,
        cron: &CronState,
        wake: Option<&CronWake>,
        end: CronRunEnd<'_>,
        deliver: impl FnOnce(&str, &str) -> Result<(), String>,
    ) {
        let CronRunEnd {
            job_id,
            run_id,
            status,
            summary,
        } = end;
        let (report_target, is_oneshot) = {
            let store = cron.lock();
            let job = store.get(job_id);
            (
                job.and_then(|job| job.report_target_tab_id.clone()),
                job.is_some_and(|job| job.oneshot),
            )
        };
        let delivery = match report_target {
            Some(report_target) => {
                let report = format!(
                    "Cron run {run_id} completed with status {status:?}. Summary: {summary}"
                );
                match deliver(&report_target, &report) {
                    Ok(()) => "delivered".to_string(),
                    Err(error) => format!("failed: {error}"),
                }
            }
            None => "no report target".to_string(),
        };
        if let Err(error) = cron
            .lock()
            .set_run_report(job_id, run_id, summary, delivery)
        {
            log::error!("Failed to record cron run report: job={job_id} run={run_id}: {error}");
        }
        if !is_oneshot {
            return;
        }
        let Some(wake) = wake else {
            log::error!("Failed to auto-delete oneshot cron: CronWake unavailable");
            return;
        };
        log::info!("Oneshot cron auto-deleting: job={}", job_id);
        if let Err(error) = self.cron_delete_inner(cron, wake, job_id, true) {
            log::error!("Failed to auto-delete oneshot cron: {}", error);
        }
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
            let _ = self.close_tab_with_reason(tab_id, TabCloseReason::Cancelled);
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
        let (
            agent,
            model,
            reasoning_effort,
            task,
            cwd,
            role,
            skill,
            workspace_id,
            team_id,
            creator_tab_id,
            report_target_tab_id,
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
                job.reasoning_effort.clone(),
                job.task.clone(),
                job.cwd.clone(),
                job.role.clone(),
                job.skill.clone(),
                job.workspace_id.clone(),
                job.team_id.clone(),
                job.creator_tab_id.clone(),
                job.report_target_tab_id.clone(),
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
            summary: None,
            report_delivery: None,
        };

        let skill_block = skill.as_deref().map(crate::commands::cron_skill_block);
        let task_text = crate::cron::task_text::build_cron_task_text(
            &task,
            skill_block.as_deref(),
            report_target_tab_id.as_deref(),
            &run_id,
        );
        let resume_system_prompt = crate::service::orchestrator::provider_launch_task(
            &self.agent_registry,
            &agent,
            Some(&task_text),
            role.as_deref(),
            None,
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

        // Same builder as the scheduler's automatic fire: one routing decision, so a
        // triggered run and a scheduled run cannot disagree about the runtime.
        let spec = crate::cron::spawn_spec::build_cron_spawn_spec(
            self.chatbackend_registry.as_ref(),
            crate::cron::spawn_spec::CronSpawnInputs {
                job_id: id.to_string(),
                job_name,
                agent,
                model,
                reasoning_effort,
                task_text,
                role,
                cwd,
                workspace_id: (!workspace_id.is_empty()).then_some(workspace_id),
                team_id,
                creator_tab_id,
                report_target_tab_id,
            },
        );

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
                        resume_system_prompt.clone(),
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
        caller_tab_id: Option<&str>,
        binding_nonce: Option<&str>,
    ) -> Result<(), String> {
        let mut store = cron.lock();
        let owner = store
            .get_tab_owner_by_run(run_id)
            .map(|(j, p)| (j.to_string(), p.to_string()));
        if let Some((job_id, tab_id)) = owner {
            if caller_tab_id.is_some_and(|caller| {
                caller != tab_id
                    || !binding_nonce.is_some_and(|nonce| {
                        self.mux
                            .read()
                            .get_tab(tab_id)
                            .and_then(|tab| tab.session_binding_nonce.as_deref())
                            == Some(nonce)
                    })
            }) {
                return Err(format!(
                    "Only the owning Tab or a human caller may complete cron run {run_id}"
                ));
            }
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

            let summary_text = summary.unwrap_or("(no summary provided)").to_string();
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

            // Killing it before the caller writes the JSON-RPC response would break the pipe mid-write, which interprocess escalates to std::process::abort().
            let svc = self.clone();
            let cron = cron.clone();
            let wake = wake.clone();
            let tab_id = tab_id.clone();
            let job_id = job_id.clone();
            let run_id = run_id.to_string();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let _ = svc.close_tab_with_reason(&tab_id, TabCloseReason::AgentRunEnded);
                svc.cron_settle_run_end(
                    &cron,
                    Some(&wake),
                    CronRunEnd {
                        job_id: &job_id,
                        run_id: &run_id,
                        status: CronRunStatus::Success,
                        summary: &summary_text,
                    },
                );
                svc.emit_cron_changed();
            });
            Ok(())
        } else if let Some((job_id, run)) = store.find_run_by_id(run_id) {
            // The terminal status already recorded stands (first terminal wins); log it so the race stays observable instead of surfacing as an opaque "not found".
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::CronRunEnd;
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
        mux.get_tab_mut(&owner_tab)
            .unwrap()
            .session_binding_nonce = Some("owner-nonce".into());
        mux.get_tab_mut(&foreign_tab)
            .unwrap()
            .session_binding_nonce = Some("foreign-nonce".into());
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
                )
                .is_err());
        }

        {
            let store = cron.lock();
            let run = store
                .get_history(&job_id)
                .iter()
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
            )
            .is_ok());
        let store = cron.lock();
        let run = store
            .get_history(&job_id)
            .iter()
            .find(|run| run.run_id == run_id)
            .expect("owner-completed run remains recorded");
        assert_eq!(run.status, CronRunStatus::Success);
    }
}

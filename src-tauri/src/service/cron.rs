use super::ServiceContext;
use crate::cron::store::{
    commit, finish_run, CronCommitError, CronCommitted, CronCreateParams, CronJob, CronRunInfo,
    CronRunStatus, CronScope, CronStore, CronUpdateParams, RunSession,
};
use crate::multiplexer::types::TabCloseReason;
use crate::service::message_gateway::DeliveryClass;
use crate::service::messenger::MessageEnvelope;
use crate::CronState;
use crate::CronWake;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use tauri::Manager;

pub(crate) fn cron_fired_payload(cron_id: &str, run_id: &str, tab_id: &str) -> serde_json::Value {
    serde_json::json!({"cron_id": cron_id, "run_id": run_id, "tab_id": tab_id})
}

pub(crate) fn emit_cron_fired(app: &tauri::AppHandle, cron_id: &str, run_id: &str, tab_id: &str) {
    use tauri::Emitter;
    let _ = app.emit("cron-fired", cron_fired_payload(cron_id, run_id, tab_id));
}

fn cron_report_delivery_label(result: Result<(), String>) -> String {
    match result {
        Ok(()) => "delivered".to_string(),
        Err(error) if error.starts_with("submit_unconfirmed:") => {
            "delivered_unconfirmed".to_string()
        }
        Err(error) => format!("failed: {error}"),
    }
}

fn cron_status_slug(status: &CronRunStatus) -> &'static str {
    match status {
        CronRunStatus::Running => "running",
        CronRunStatus::AwaitingOwnerPrompt => "awaiting_owner_prompt",
        CronRunStatus::Success => "success",
        CronRunStatus::Failed => "failed",
        CronRunStatus::Cancelled => "cancelled",
    }
}

fn cron_end_context(job_name: &str, status: &CronRunStatus) -> String {
    format!("cron:{job_name}:{}", cron_status_slug(status))
}

/// A cron run that has ended, by any path, and still owes its report and oneshot retirement.
pub(crate) struct CronRunEnd<'a> {
    pub(crate) job_id: &'a str,
    pub(crate) run_id: &'a str,
    pub(crate) status: CronRunStatus,
    pub(crate) summary: &'a str,
}

pub(crate) struct CronCompleteAfterResponse {
    pub(crate) tab_id: String,
    pub(crate) job_id: String,
    pub(crate) run_id: String,
    pub(crate) summary: String,
    pub(crate) cron: CronState,
    pub(crate) wake: CronWake,
}

/// Newest owner-prompt value per tab while a Storage commit of that tab is settling.
#[derive(Clone, Default)]
struct OwnerPromptRequests(Arc<Mutex<HashMap<String, OwnerPromptRequest>>>);

struct OwnerPromptRequest {
    awaiting: bool,
    requested: u64,
    admitted: u64,
}

impl OwnerPromptRequests {
    /// Never waits for COMMIT. One change per tab is admitted at a time and each settlement
    /// admits the tab's newest value, so the latest request always commits last.
    fn request<R>(&self, cron: &CronState, tab_id: &str, awaiting: bool, report: R)
    where
        R: Fn(&str, Result<CronCommitted<bool>, CronCommitError>) + Send + 'static,
    {
        {
            let mut tabs = self.0.lock();
            if let Some(request) = tabs.get_mut(tab_id) {
                request.awaiting = awaiting;
                request.requested += 1;
                return;
            }
            tabs.insert(
                tab_id.to_string(),
                OwnerPromptRequest {
                    awaiting,
                    requested: 1,
                    admitted: 0,
                },
            );
        }
        let Some(first) = self.admit_next(cron, tab_id, &report) else {
            return;
        };
        let (requests, cron, tab_id) = (self.clone(), cron.clone(), tab_id.to_string());
        tauri::async_runtime::spawn(async move {
            let mut settling = Some(first);
            while let Some(commit) = settling.take() {
                let outcome = commit.await;
                report(&tab_id, outcome);
                settling = requests.admit_next(&cron, &tab_id, &report);
            }
        });
    }

    /// Admits the tab's newest unadmitted value; `None` retires the tab once none is left.
    fn admit_next(
        &self,
        cron: &CronState,
        tab_id: &str,
        report: &impl Fn(&str, Result<CronCommitted<bool>, CronCommitError>),
    ) -> Option<impl Future<Output = Result<CronCommitted<bool>, CronCommitError>> + Send + 'static>
    {
        loop {
            let awaiting = {
                let mut tabs = self.0.lock();
                let request = tabs.get_mut(tab_id)?;
                if request.admitted == request.requested {
                    tabs.remove(tab_id);
                    return None;
                }
                request.admitted = request.requested;
                request.awaiting
            };
            let tab = tab_id.to_string();
            match CronStore::stage(cron, move |store| {
                Ok(store.prepare_owner_prompt(&tab, awaiting))
            }) {
                Ok(Some(commit)) => return Some(commit),
                Ok(None) => {}
                Err(error) => report(tab_id, Err(error)),
            }
        }
    }
}

impl ServiceContext {
    /// Under Storage the change is staged and settles on its completion's wake; the legacy
    /// JSON backend still commits before returning.
    pub(crate) fn set_cron_owner_prompt_waiting(&self, tab_id: &str, awaiting: bool) {
        let Some(cron) = self.app_handle.try_state::<CronState>() else {
            return;
        };
        let staged = cron.lock().commits_to_storage();
        if staged {
            let service = self.clone();
            self.owner_prompt_requests().request(
                cron.inner(),
                tab_id,
                awaiting,
                move |tab_id, updated| service.report_owner_prompt(tab_id, updated),
            );
            return;
        }
        let updated = commit(cron.inner(), |store| {
            Ok(store.prepare_owner_prompt(tab_id, awaiting))
        });
        self.report_owner_prompt(tab_id, updated);
    }

    fn owner_prompt_requests(&self) -> OwnerPromptRequests {
        if let Some(requests) = self.app_handle.try_state::<OwnerPromptRequests>() {
            return requests.inner().clone();
        }
        self.app_handle.manage(OwnerPromptRequests::default());
        self.app_handle
            .state::<OwnerPromptRequests>()
            .inner()
            .clone()
    }

    fn report_owner_prompt(
        &self,
        tab_id: &str,
        updated: Result<CronCommitted<bool>, CronCommitError>,
    ) {
        match updated {
            Ok(done) if done.value => self.emit_cron_changed(),
            Ok(_) => {}
            Err(error) => {
                log::error!("Failed to persist cron owner-prompt state for tab {tab_id}: {error}");
                if error.applied() {
                    self.emit_cron_changed();
                }
            }
        }
    }

    pub(crate) fn sync_cron_owner_prompt_state(&self, tab_id: &str) {
        let awaiting = self
            .mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| tab.pending_prompt.is_some());
        self.set_cron_owner_prompt_waiting(tab_id, awaiting);
    }

    pub fn cron_create(
        &self,
        cron: &CronState,
        wake: &CronWake,
        mut params: CronCreateParams,
    ) -> Result<CronJob, String> {
        params.creator_tab_id = params
            .creator_tab_id
            .take()
            .filter(|tab_id| !tab_id.trim().is_empty());
        if let Some(tab_id) = params.creator_tab_id.as_deref() {
            if self.mux.read().get_tab(tab_id).is_none() {
                return Err(format!("creator_tab_id: Cron Tab {tab_id} not found"));
            }
        }
        let inherited_attachment = crate::service::team::resolve_inherited_team_attachment(
            &self.mux.read(),
            params.team_id.as_deref(),
            params.creator_tab_id.as_deref(),
            &params.workspace_id,
            params.detach_from_team,
        )
        .map_err(|error| format!("team_id/creator_tab_id: {error}"))?;
        params.team_id = inherited_attachment.map(|attachment| attachment.team_id);
        self.validate_cron_ownership(&params)?;
        let provider = params
            .agent
            .as_deref()
            .filter(|agent| !agent.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(crate::config::default_agent_provider);
        params.agent = Some(provider.clone());
        params.model = params.model.take().filter(|model| !model.trim().is_empty());
        self.validate_cron_spawn_fields(&provider, params.model.as_deref(), params.cwd.as_deref())?;
        self.validate_cron_reasoning_effort(
            &provider,
            params.model.as_deref(),
            params.reasoning_effort.as_deref(),
        )?;
        let job = commit(cron, |store| store.prepare_create(params.clone()))?.value;
        wake.notify_all();
        self.emit_cron_changed();
        self.record_product_feature(codeterm_analytics::Feature::CronCreated);
        Ok(job)
    }

    fn validate_cron_ownership(&self, params: &CronCreateParams) -> Result<(), String> {
        let mux = self.mux.read();
        if let Some(target_tab_id) = params.report_target_tab_id.as_deref() {
            mux.get_tab(target_tab_id).ok_or_else(|| {
                format!("report_target_tab_id: Cron Tab {target_tab_id} not found")
            })?;
        }
        match (params.team_id.as_deref(), params.creator_tab_id.as_deref()) {
            (None, _) => Ok(()),
            (Some(team_id), Some(creator_tab_id)) => {
                let creator = mux.get_tab(creator_tab_id).ok_or_else(|| {
                    format!("creator_tab_id: Cron Tab {creator_tab_id} not found")
                })?;
                if creator.team_id.as_deref() != Some(team_id) {
                    return Err(format!(
                        "creator_tab_id: Cron Tab {creator_tab_id} does not belong to Team {team_id}"
                    ));
                }
                let team_workspace = mux
                    .team_workspace(team_id)
                    .ok_or_else(|| format!("team_id: Team {team_id} not found"))?;
                if team_workspace != params.workspace_id {
                    return Err(format!(
                        "team_id: Team {team_id} belongs to workspace {}, not {}",
                        team_workspace, params.workspace_id
                    ));
                }
                Ok(())
            }
            (Some(team_id), None) => Err(format!(
                "creator_tab_id: Team cron {team_id} requires a live Team member Tab"
            )),
        }
    }

    fn validate_cron_spawn_fields(
        &self,
        provider: &str,
        model: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<(), String> {
        self.require_agent_provider_ready(provider)
            .map_err(|error| format!("agent: {error}"))?;
        crate::service::spawn::validate_agent_model(&self.agent_registry, provider, model)
            .map_err(|error| format!("model: {error}"))?;
        crate::service::spawn_cwd::validate_spawn_cwd(cwd).map_err(|error| format!("cwd: {error}"))
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
        self.require_agent_provider_ready(provider)
            .map_err(|error| format!("agent: {error}"))?;
        let model =
            model.map(|model| crate::agent::model_catalog::resolve_model(provider, Some(model)).id);
        crate::service::spawn::validate_agent_reasoning_effort(
            &self.agent_registry,
            provider,
            model.as_deref(),
            effort,
        )
        .map_err(|error| format!("reasoning_effort: {error}"))
    }

    pub fn cron_list(
        &self,
        cron: &CronState,
        workspace_id: Option<&str>,
    ) -> Result<Vec<CronJob>, String> {
        let store = cron.lock();
        if let Some(reason) = store.unavailable_reason() {
            return Err(format!("cron storage unavailable: {reason}"));
        }
        Ok(match workspace_id {
            Some(ws) => store.list_for_workspace(ws).into_iter().cloned().collect(),
            None => store.list().to_vec(),
        })
    }

    pub fn cron_get(&self, cron: &CronState, id: &str) -> Result<CronJob, String> {
        let store = cron.lock();
        if let Some(reason) = store.unavailable_reason() {
            return Err(format!("cron storage unavailable: {reason}"));
        }
        store
            .get(id)
            .cloned()
            .ok_or_else(|| format!("Cron job not found: {}", id))
    }

    pub fn cron_update(
        &self,
        cron: &CronState,
        wake: &CronWake,
        id: &str,
        mut patch: CronUpdateParams,
    ) -> Result<CronJob, String> {
        if patch
            .agent
            .as_deref()
            .is_some_and(|agent| agent.trim().is_empty())
        {
            patch.agent = Some(crate::config::default_agent_provider());
        }
        let (provider, model, cwd, effort) = {
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
            let cwd = patch.cwd.clone().or_else(|| job.cwd.clone());
            (provider, model, cwd, effort)
        };
        self.validate_cron_spawn_fields(
            &provider,
            model.as_deref().filter(|model| !model.trim().is_empty()),
            cwd.as_deref(),
        )?;
        self.validate_cron_reasoning_effort(
            &provider,
            model.as_deref().filter(|model| !model.trim().is_empty()),
            effort.as_deref(),
        )?;
        let job = commit(cron, |store| store.prepare_update(id, patch.clone()))?.value;
        wake.notify_all();
        self.emit_cron_changed();
        Ok(job)
    }

    pub fn cron_delete(&self, cron: &CronState, wake: &CronWake, id: &str) -> Result<(), String> {
        self.cron_delete_inner(cron, wake, id, false)
    }

    /// Tabs close and logs go only after the deletion committed; a refused or failed
    /// deletion leaves runs, tabs and logs untouched.
    fn cron_delete_inner(
        &self,
        cron: &CronState,
        wake: &CronWake,
        id: &str,
        preserve_history: bool,
    ) -> Result<(), String> {
        let (all_tabs, logs_dir) = {
            let store = cron.lock();
            let tabs: std::collections::HashSet<String> = store
                .tab_owners_for_job(id)
                .into_iter()
                .chain(
                    store
                        .get_history(id)
                        .iter()
                        .filter_map(|r| r.tab_id.clone()),
                )
                .collect();
            (tabs, store.job_logs_dir(id))
        };
        let committed = commit(cron, |store| store.prepare_delete(id, preserve_history));
        if let Err(error) = &committed {
            if !error.applied() {
                return Err(error.to_string());
            }
        }
        {
            let mut store = cron.lock();
            for tab_id in &all_tabs {
                store.unregister_tab(tab_id);
            }
        }
        for tab_id in &all_tabs {
            let _ = self.close_tab_with_reason(tab_id, TabCloseReason::Cancelled);
        }
        if !preserve_history {
            if let Some(logs_dir) = logs_dir {
                let _ = std::fs::remove_dir_all(&logs_dir);
            }
        }
        committed?;
        wake.notify_all();
        self.emit_cron_changed();
        Ok(())
    }

    pub(crate) fn cron_delete_workspace(
        &self,
        cron: &CronState,
        wake: Option<&CronWake>,
        workspace_id: &str,
    ) -> Result<(), String> {
        let removed = commit(cron, |store| {
            store.prepare_delete_scope(CronScope::Workspace(workspace_id.to_string()))
        })?
        .removed;
        if removed.is_empty() {
            return Ok(());
        }
        let logs: Vec<_> = {
            let store = cron.lock();
            removed
                .iter()
                .filter_map(|job_id| store.job_logs_dir(job_id))
                .collect()
        };
        for logs_dir in logs {
            let _ = std::fs::remove_dir_all(logs_dir);
        }
        if let Some(wake) = wake {
            wake.notify_all();
        }
        self.emit_cron_changed();
        Ok(())
    }

    pub(crate) fn cron_settle_run_end(
        &self,
        cron: &CronState,
        wake: Option<&CronWake>,
        end: CronRunEnd<'_>,
    ) {
        self.cron_settle_run_end_with(cron, wake, end, |tab_id, envelope, text| {
            self.deliver_cron_report(tab_id, envelope, text)
        });
    }

    /// Agents get a framed system notice; plain shells keep the bare text.
    pub(crate) fn deliver_cron_report(
        &self,
        target: &str,
        envelope: MessageEnvelope,
        body: &str,
    ) -> Result<(), String> {
        if !self.tab_has_agent_identity(target) {
            return self.send_text(target, body);
        }
        self.gateway_deliver(target, envelope, body, DeliveryClass::SystemNotice)
            .map(|_| ())
    }

    /// Delivers the ended run's status to its report tab, records the outcome in history,
    /// and retires a oneshot job while keeping that history.
    pub(crate) fn cron_settle_run_end_with(
        &self,
        cron: &CronState,
        wake: Option<&CronWake>,
        end: CronRunEnd<'_>,
        deliver: impl FnOnce(&str, MessageEnvelope, &str) -> Result<(), String>,
    ) {
        let CronRunEnd {
            job_id,
            run_id,
            status,
            summary,
        } = end;
        let (report_target, is_oneshot, job_name) = {
            let store = cron.lock();
            let job = store.get(job_id);
            (
                job.and_then(|job| job.report_target_tab_id.clone()),
                job.is_some_and(|job| job.oneshot),
                job.map(|job| job.name.trim().to_string())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| job_id.to_string()),
            )
        };
        let delivery = match report_target {
            Some(report_target) => {
                let report = format!(
                    "Cron run {run_id} completed with status {status:?}. Summary: {summary}"
                );
                let envelope =
                    MessageEnvelope::codeterm_reminder(cron_end_context(&job_name, &status));
                cron_report_delivery_label(deliver(&report_target, envelope, &report))
            }
            None => "no report target".to_string(),
        };
        if let Err(error) = commit(cron, |store| {
            Ok(store.prepare_set_run_report(job_id, run_id, summary, &delivery))
        }) {
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

    /// Tabs close only once the cancellation is recorded; legacy JSON keeps its
    /// in-memory cancellation even when the file save fails.
    pub fn cron_cancel_running(&self, cron: &CronState, id: &str) -> Result<usize, String> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        match commit(cron, |store| store.prepare_cancel_running(id, now_ms)) {
            Ok(done) => {
                for (run_id, elapsed_ms) in &done.value {
                    // Observability parity with update_run_status — see crud::log_finished.
                    log::info!(
                        "Cron run completed: job={}, run={}, status=Cancelled, elapsed_ms={}",
                        id,
                        run_id,
                        elapsed_ms
                    );
                }
            }
            Err(error) if error.applied() => {
                log::error!("Failed to persist cron cancellation: job={id}: {error}");
            }
            Err(error) => return Err(error.to_string()),
        }
        let to_close = {
            let mut store = cron.lock();
            let tabs = store.tab_owners_for_job(id);
            for tab_id in &tabs {
                store.unregister_tab(tab_id);
            }
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
        let job = commit(cron, |store| store.prepare_set_enabled(id, enabled))?.value;
        wake.notify_all();
        self.emit_cron_changed();
        Ok(job)
    }

    pub(crate) fn prepare_runtime_cron_owner(
        &self,
        tab_id: &str,
        job_id: &str,
        run_id: &str,
    ) -> Result<(), String> {
        let cron = self
            .app_handle
            .try_state::<CronState>()
            .ok_or("Runtime cron state unavailable")?;
        {
            let store = cron.lock();
            let (actual_job, run) = store
                .find_run_by_id(run_id)
                .ok_or("Runtime cron run unavailable")?;
            if actual_job != job_id {
                return Err("Runtime cron run belongs to another job".into());
            }
            if run
                .tab_id
                .as_deref()
                .is_some_and(|existing| existing != tab_id)
            {
                return Err("Runtime cron run belongs to another tab".into());
            }
            if run.status != CronRunStatus::Running {
                return Err("Runtime cron run already ended".into());
            }
            if let Some((owner_job, owner_run)) = store.get_tab_owner(tab_id) {
                if owner_job != job_id || owner_run != run_id {
                    return Err("Runtime cron owner changed".into());
                }
                return Ok(());
            }
        }
        commit(&cron, |store| {
            Ok(store.prepare_set_run_tab(job_id, run_id, tab_id))
        })
        .map_err(|error| error.to_string())?;
        cron.lock().register_tab(tab_id, job_id, run_id);
        Ok(())
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
                run_id: run_id.clone(),
                job_name,
                agent,
                model,
                reasoning_effort,
                task_text,
                role,
                cwd,
                workspace_id: crate::cron::recovery::restored_spawn_workspace(
                    &self.mux.read(),
                    &workspace_id,
                ),
                team_id,
                creator_tab_id,
                report_target_tab_id,
            },
        );

        let spawn = dispatch_after_trigger_record(cron, id, &run_info, || self.spawn_tab(spec))?;
        match spawn {
            Ok(handle) => {
                run_info.tab_id = Some(handle.tab_id.clone());
                cron.lock().register_tab(&handle.tab_id, id, &run_id);
                if let Err(error) = commit(cron, |store| {
                    Ok(store.prepare_set_run_tab(id, &run_id, &handle.tab_id))
                }) {
                    log::error!(
                        "Failed to record triggered cron Tab: job={id} run={run_id}: {error}"
                    );
                }
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
                let _ = finish_run(cron, id, &run_id, CronRunStatus::Failed, None);
                return Err(e);
            }
        }

        if let Some(tab_id) = run_info.tab_id.as_deref() {
            self.sync_cron_owner_prompt_state(tab_id);
            emit_cron_fired(&self.app_handle, id, &run_id, tab_id);
        }
        self.emit_cron_changed();
        Ok(run_info)
    }

    pub(crate) fn cron_complete(
        &self,
        cron: &CronState,
        wake: &CronWake,
        run_id: &str,
        summary: Option<&str>,
        caller_tab_id: Option<&str>,
        binding_nonce: Option<&str>,
        human_confirmed: bool,
    ) -> Result<Option<CronCompleteAfterResponse>, String> {
        let caller_tab_id = caller_tab_id.filter(|caller| !caller.trim().is_empty());
        if caller_tab_id.is_none() && !human_confirmed {
            return Err("completing a cron run without tab identity requires --yes".into());
        }
        let owner = cron
            .lock()
            .get_tab_owner_by_run(run_id)
            .map(|(j, p)| (j.to_string(), p.to_string()));
        if let Some((job_id, tab_id)) = owner {
            if caller_tab_id.is_some_and(|caller| {
                caller != tab_id
                    || !binding_nonce.is_some_and(|nonce| {
                        self.mux
                            .read()
                            .get_tab(&tab_id)
                            .and_then(|tab| tab.session_binding_nonce.as_deref())
                            == Some(nonce)
                    })
            }) {
                return Err(format!(
                    "Only the owning Tab or a human caller may complete cron run {run_id}"
                ));
            }
            if self
                .mux
                .read()
                .get_tab(&tab_id)
                .is_some_and(|tab| tab.agent_type.is_some())
            {
                self.run_watcher.request_cron_completion(
                    self,
                    &tab_id,
                    run_id,
                    summary.unwrap_or("(no summary provided)"),
                )?;
                return Ok(None);
            }
            let (pty_id, session_id, run_cwd) = {
                let mux = self.mux.read();
                let tab = mux.get_tab(&tab_id);
                (
                    tab.and_then(|p| p.pty_id.clone()),
                    tab.and_then(|p| p.agent_session_id.clone()),
                    tab.and_then(|p| p.cwd.clone()),
                )
            };
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

            let log_file = cron.lock().log_file(&job_id, run_id);
            if let Err(e) =
                log_file.and_then(|path| CronStore::create_log(&path, &log_text).map(|_| ()))
            {
                log::error!("Failed to save cron log: {}", e);
            }

            let session = session_id.as_deref().map(|session_id| RunSession {
                session_id,
                cwd: run_cwd.as_deref(),
            });
            let _ = finish_run(cron, &job_id, run_id, CronRunStatus::Success, session);
            let is_oneshot = {
                let mut store = cron.lock();
                store.unregister_tab(&tab_id);
                store.get(&job_id).map(|j| j.oneshot).unwrap_or(false)
            };
            log::info!(
                "Cron run completed: job={}, run={}, oneshot={}, summary_len={}",
                job_id,
                run_id,
                is_oneshot,
                log_text.len()
            );

            // Tell the run-watcher this tab is already done so the idle / hard timeout watchdog does NOT fire later and overwrite our Success status with Failed.
            if let Some(ref pid) = pty_id {
                if let Some(attempt) = crate::service::launch_readiness::LaunchAttempt::current(
                    &self.mux, &tab_id, pid,
                ) {
                    crate::service::launch_readiness::retire_attempt(&attempt);
                }
                self.run_watcher.mark_completed_externally(pid);
            }

            Ok(Some(CronCompleteAfterResponse {
                tab_id,
                job_id,
                run_id: run_id.to_string(),
                summary: summary_text,
                cron: cron.clone(),
                wake: wake.clone(),
            }))
        } else if let Some((job_id, run)) = cron.lock().find_run_by_id(run_id) {
            // The terminal status already recorded stands (first terminal wins); log it so the race stays observable instead of surfacing as an opaque "not found".
            log::warn!(
                "[cron] cron.complete for run={} (job={}) arrived after the run was already finalized as {:?} — treating as idempotent no-op",
                run_id, job_id, run.status
            );
            Ok(None)
        } else {
            Err(format!("No cron run found for run_id: {}", run_id))
        }
    }

    pub fn cron_history(&self, cron: &CronState, id: &str) -> Result<Vec<CronRunInfo>, String> {
        let store = cron.lock();
        if let Some(reason) = store.unavailable_reason() {
            return Err(format!("cron storage unavailable: {reason}"));
        }
        let job_id = resolve_cron_id_or_name(&store, id)?;
        let history = store.get_history(job_id);
        if history.is_empty() && store.get(job_id).is_none() {
            return Err(format!("Cron job not found: {id}"));
        }
        Ok(history.into_iter().cloned().collect())
    }

    pub(crate) fn emit_cron_changed(&self) {
        let handle = self.app_handle.clone();
        std::thread::spawn(move || {
            crate::api::events::emit_everywhere(&handle, "cron-run-finished", ());
        });
    }
}

fn resolve_cron_id_or_name<'a>(
    store: &'a CronStore,
    id_or_name: &'a str,
) -> Result<&'a str, String> {
    if let Some(job) = store.get(id_or_name) {
        return Ok(&job.id);
    }
    if !store.get_history(id_or_name).is_empty() {
        return Ok(id_or_name);
    }
    let mut matches = store.list().iter().filter(|job| job.name == id_or_name);
    let Some(job) = matches.next() else {
        return Err(format!("Cron job not found: {id_or_name}"));
    };
    if matches.next().is_some() {
        return Err(format!("Cron job name is ambiguous: {id_or_name}"));
    }
    Ok(&job.id)
}

fn dispatch_after_trigger_record<T>(
    cron: &CronState,
    id: &str,
    run: &CronRunInfo,
    dispatch: impl FnOnce() -> T,
) -> Result<T, String> {
    record_trigger_run(cron, id, run)?;
    Ok(dispatch())
}

fn record_trigger_run(cron: &CronState, id: &str, run: &CronRunInfo) -> Result<(), String> {
    match commit(cron, |store| store.prepare_record_run(id, run.clone())) {
        Ok(_) => Ok(()),
        Err(error) => {
            log::error!("Failed to record triggered cron run: job={id}: {error}");
            if error.applied() {
                Ok(())
            } else {
                Err(error.to_string())
            }
        }
    }
}

#[cfg(test)]
#[path = "cron/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cron/owner_prompt_tests.rs"]
mod owner_prompt_tests;

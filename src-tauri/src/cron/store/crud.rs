//! CronStore operations. Each `prepare_*` derives one authorized change from the cache
//! without touching it; the backend commits that change before the cache sees it.

use std::collections::HashSet;

use super::{
    cap_jitter_max_ms_at, compute_backoff, compute_next_run_jittered_at, default_max_history,
    is_stale_fire, parse_oneshot_schedule_at, validate_tz, CronCreateParams, CronJob, CronRunInfo,
    CronRunStatus, CronStore, CronUpdateParams, MAX_CONSECUTIVE_FAILURES,
};

/// Complete post-state values of one authorized cron change.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CronChange {
    Create(CronJob),
    /// Definition values; a patch never moves the job's last run.
    Patch(CronJob),
    /// The job's post-state, whose last run is `run`, and `run` appended to its history.
    StartRun {
        job: CronJob,
        run: CronRunInfo,
    },
    /// Changed history entries and, when it changed, the job's post-state.
    UpdateRuns {
        job_id: String,
        history: Vec<CronRunInfo>,
        job: Option<CronJob>,
    },
    Remove {
        id: String,
        keep_history: bool,
    },
    RemoveScope(CronScope),
}

impl CronChange {
    /// The single job a change is fenced on; scope removals name no job up front.
    pub(crate) fn job_id(&self) -> Option<&str> {
        match self {
            CronChange::Create(job) | CronChange::Patch(job) | CronChange::StartRun { job, .. } => {
                Some(job.id.as_str())
            }
            CronChange::UpdateRuns { job_id, .. } => Some(job_id.as_str()),
            CronChange::Remove { id, .. } => Some(id.as_str()),
            CronChange::RemoveScope(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CronScope {
    Workspace(String),
    Team(String),
}

impl CronScope {
    pub(crate) fn contains(&self, job: &CronJob) -> bool {
        match self {
            CronScope::Workspace(workspace) => job.workspace_id == *workspace,
            CronScope::Team(team) => job.team_id.as_deref() == Some(team.as_str()),
        }
    }
}

/// An authorized change and the value its caller receives once the change is committed.
pub(crate) struct Prepared<T> {
    pub(crate) changes: Vec<CronChange>,
    pub(crate) value: T,
}

impl<T> Prepared<T> {
    pub(crate) fn new(change: CronChange, value: T) -> Self {
        Prepared {
            changes: vec![change],
            value,
        }
    }

    pub(crate) fn unchanged(value: T) -> Self {
        Prepared {
            changes: Vec::new(),
            value,
        }
    }
}

/// Outcome of a run completion; `fresh` is true only for the first terminal transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunFinished {
    pub(crate) fresh: bool,
    pub(crate) elapsed_ms: Option<u64>,
}

/// Session a run's agent bound before its tab closed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunSession<'a> {
    pub(crate) session_id: &'a str,
    pub(crate) cwd: Option<&'a str>,
}

/// Clones of one run's history entry and of its job, edited like the legacy in-place updates.
pub(super) struct RunEdit {
    job_id: String,
    run_id: String,
    entry: Option<CronRunInfo>,
    job: Option<CronJob>,
}

impl RunEdit {
    fn mirror(&mut self) -> Option<&mut CronRunInfo> {
        let run_id = &self.run_id;
        self.job
            .as_mut()?
            .last_run
            .as_mut()
            .filter(|run| run.run_id == *run_id)
    }

    /// Applies one edit to the history entry and to the job's last run of the same run.
    pub(super) fn each(&mut self, mut edit: impl FnMut(&mut CronRunInfo)) {
        if let Some(entry) = self.entry.as_mut() {
            edit(entry);
        }
        if let Some(mirror) = self.mirror() {
            edit(mirror);
        }
    }

    /// Keeps only what differs from the cache.
    pub(super) fn into_prepared<T>(self, store: &CronStore, value: T) -> Prepared<T> {
        let history: Vec<CronRunInfo> = self
            .entry
            .filter(|entry| store.history_entry(&self.job_id, &self.run_id) != Some(entry))
            .into_iter()
            .collect();
        let job = self.job.filter(|job| store.get(&self.job_id) != Some(job));
        if history.is_empty() && job.is_none() {
            return Prepared::unchanged(value);
        }
        Prepared::new(
            CronChange::UpdateRuns {
                job_id: self.job_id,
                history,
                job,
            },
            value,
        )
    }
}

impl CronStore {
    pub fn get(&self, id: &str) -> Option<&CronJob> {
        self.file.jobs.iter().find(|j| j.id == id)
    }

    pub fn list(&self) -> &[CronJob] {
        &self.file.jobs
    }

    pub fn list_for_workspace(&self, workspace_id: &str) -> Vec<&CronJob> {
        self.file
            .jobs
            .iter()
            .filter(|j| j.workspace_id == workspace_id)
            .collect()
    }

    pub fn list_for_team(&self, team_id: &str) -> Vec<&CronJob> {
        self.file
            .jobs
            .iter()
            .filter(|job| job.team_id.as_deref() == Some(team_id))
            .collect()
    }

    pub fn list_enabled(&self) -> Vec<&CronJob> {
        self.file.jobs.iter().filter(|j| j.enabled).collect()
    }

    pub fn get_history(&self, job_id: &str) -> Vec<&CronRunInfo> {
        self.file
            .history
            .get(job_id)
            .map(|h| h.iter().collect())
            .unwrap_or_default()
    }

    pub(crate) fn active_runs(&self) -> Vec<(String, CronRunInfo)> {
        let mut runs = Vec::new();
        let mut seen = HashSet::new();
        for job in &self.file.jobs {
            for run in self.file.history.get(&job.id).into_iter().flatten() {
                if is_active_run(run) && seen.insert(run.run_id.clone()) {
                    runs.push((job.id.clone(), run.clone()));
                }
            }
            if let Some(run) = job.last_run.as_ref().filter(|run| is_active_run(run)) {
                if seen.insert(run.run_id.clone()) {
                    runs.push((job.id.clone(), run.clone()));
                }
            }
        }
        runs
    }

    /// Locate a run by id across all jobs' history, without knowing its job_id
    /// up front — used to distinguish "no such run" from "run already
    /// finalized" when its tab ownership has already been unregistered.
    pub fn find_run_by_id(&self, run_id: &str) -> Option<(&str, &CronRunInfo)> {
        self.file.history.iter().find_map(|(job_id, runs)| {
            runs.iter()
                .find(|r| r.run_id == run_id)
                .map(|r| (job_id.as_str(), r))
        })
    }

    pub(super) fn history_entry(&self, job_id: &str, run_id: &str) -> Option<&CronRunInfo> {
        self.file
            .history
            .get(job_id)?
            .iter()
            .find(|run| run.run_id == run_id)
    }

    pub(super) fn edit_run(&self, job_id: &str, run_id: &str) -> RunEdit {
        RunEdit {
            job_id: job_id.to_string(),
            run_id: run_id.to_string(),
            entry: self.history_entry(job_id, run_id).cloned(),
            job: self.get(job_id).cloned(),
        }
    }

    /// Writes committed post-state values into the cache.
    pub(crate) fn apply_change(&mut self, change: &CronChange) {
        match change {
            CronChange::Create(job) => self.file.jobs.push(job.clone()),
            CronChange::Patch(job) => self.replace_job(job),
            CronChange::StartRun { job, run } => {
                self.replace_job(job);
                let history = self.file.history.entry(job.id.clone()).or_default();
                history.push(run.clone());
                while history.len() > job.max_history {
                    history.remove(0);
                }
            }
            CronChange::UpdateRuns {
                job_id,
                history,
                job,
            } => {
                if let Some(entries) = self.file.history.get_mut(job_id) {
                    for run in history {
                        if let Some(entry) = entries.iter_mut().find(|e| e.run_id == run.run_id) {
                            *entry = run.clone();
                        }
                    }
                }
                if let Some(job) = job {
                    self.replace_job(job);
                }
            }
            CronChange::Remove { id, keep_history } => self.remove_cached(id, *keep_history),
            CronChange::RemoveScope(scope) => {
                let ids: Vec<String> = self
                    .file
                    .jobs
                    .iter()
                    .filter(|job| scope.contains(job))
                    .map(|job| job.id.clone())
                    .collect();
                for id in ids {
                    self.remove_scoped(&id);
                }
            }
        }
    }

    fn replace_job(&mut self, job: &CronJob) {
        if let Some(slot) = self.file.jobs.iter_mut().find(|slot| slot.id == job.id) {
            *slot = job.clone();
        }
    }

    pub(super) fn remove_cached(&mut self, id: &str, keep_history: bool) {
        self.file.jobs.retain(|job| job.id != id);
        if !keep_history {
            self.file.history.remove(id);
        }
    }

    /// Scope removal also forgets the removed jobs' live tab ownership.
    pub(super) fn remove_scoped(&mut self, id: &str) {
        self.remove_cached(id, false);
        self.tab_owners.retain(|_, (job_id, _)| job_id != id);
    }

    /// Legacy JSON path for callers holding the cron lock: apply, then save the file.
    #[cfg(test)]
    pub(super) fn legacy<T>(&mut self, prepared: Result<Prepared<T>, String>) -> Result<T, String> {
        self.backend.require_legacy()?;
        let prepared = prepared?;
        for change in &prepared.changes {
            self.apply_change(change);
        }
        if !prepared.changes.is_empty() {
            self.save()?;
        }
        Ok(prepared.value)
    }

    #[cfg(not(test))]
    pub(super) fn legacy<T>(
        &mut self,
        _prepared: Result<Prepared<T>, String>,
    ) -> Result<T, String> {
        Err("legacy cron backend is test-only".to_string())
    }

    pub(super) fn build_job(&self, params: CronCreateParams) -> Result<CronJob, String> {
        codeterm_types::automation_name::validate_cron_name(&params.name)?;
        let oneshot = params.oneshot.unwrap_or(false);
        let jitter = params.jitter_max_ms;
        let tz = params
            .tz
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if let Some(ref t) = tz {
            validate_tz(t)?;
        }
        let timeout_seconds = params.timeout_seconds.filter(|n| *n > 0);
        let reasoning_effort = normalize_reasoning_effort(params.reasoning_effort);

        let now_ms = self.now_ms();
        let enabled = params.enabled.unwrap_or(true);
        let (next_run, jitter) = if oneshot {
            let next = parse_oneshot_schedule_at(&params.schedule, now_ms)
                .map_err(|error| format!("schedule: {error}"))?;
            (enabled.then_some(next), jitter)
        } else {
            let jitter = cap_jitter_max_ms_at(&params.schedule, jitter, tz.as_deref(), now_ms)?;
            let next =
                compute_next_run_jittered_at(&params.schedule, jitter, tz.as_deref(), now_ms)?;
            (if enabled { next } else { None }, jitter)
        };

        Ok(CronJob {
            id: uuid::Uuid::new_v4().to_string(),
            name: params.name,
            description: params.description,
            workspace_id: params.workspace_id,
            team_id: params.team_id,
            schedule: params.schedule,
            enabled,
            agent: params
                .agent
                .unwrap_or_else(|| self.default_agent_provider.clone()),
            model: params.model,
            reasoning_effort,
            task: params.task,
            cwd: params.cwd,
            role: params.role,
            skill: params.skill,
            creator_tab_id: params.creator_tab_id,
            report_target_tab_id: params.report_target_tab_id,
            oneshot,
            max_history: params.max_history.unwrap_or(default_max_history()),
            created_at: now_ms,
            last_run: None,
            next_run,
            jitter_max_ms: jitter,
            tz,
            timeout_seconds,
            consecutive_failures: 0,
            protected: false,
        })
    }

    pub(crate) fn prepare_create(
        &self,
        params: CronCreateParams,
    ) -> Result<Prepared<CronJob>, String> {
        let job = self.build_job(params)?;
        Ok(Prepared::new(CronChange::Create(job.clone()), job))
    }

    pub(crate) fn prepare_update(
        &self,
        id: &str,
        patch: CronUpdateParams,
    ) -> Result<Prepared<CronJob>, String> {
        let now_ms = self.now_ms();
        let mut job = self
            .get(id)
            .cloned()
            .ok_or_else(|| format!("Cron job not found: {}", id))?;
        let schedule_changed = patch.schedule.is_some();
        let enabled_requested = patch.enabled == Some(true);

        if let Some(name) = patch.name {
            codeterm_types::automation_name::validate_cron_name(&name)?;
            job.name = name;
        }
        if let Some(desc) = patch.description {
            job.description = Some(desc);
        }
        if let Some(schedule) = patch.schedule {
            job.schedule = schedule;
        }
        if let Some(task) = patch.task {
            job.task = task;
        }
        if let Some(cwd) = patch.cwd {
            job.cwd = Some(cwd);
        }
        if let Some(role) = patch.role {
            job.role = Some(role);
        }
        if let Some(skill) = patch.skill {
            job.skill = if skill.is_empty() { None } else { Some(skill) };
        }
        if let Some(max_history) = patch.max_history {
            job.max_history = max_history;
        }
        if let Some(enabled) = patch.enabled {
            job.enabled = enabled;
        }
        if let Some(agent) = patch.agent {
            job.agent = agent;
        }
        if let Some(model) = patch.model {
            job.model = if model.is_empty() { None } else { Some(model) };
        }
        if let Some(reasoning_effort) = patch.reasoning_effort {
            job.reasoning_effort = normalize_reasoning_effort(Some(reasoning_effort));
        }
        if let Some(jitter) = patch.jitter_max_ms {
            job.jitter_max_ms = Some(jitter);
        }
        if let Some(tz) = patch.tz {
            let trimmed = tz.trim();
            if trimmed.is_empty() {
                job.tz = None;
            } else {
                validate_tz(trimmed)?;
                job.tz = Some(trimmed.to_string());
            }
        }
        if let Some(ts) = patch.timeout_seconds {
            job.timeout_seconds = if ts == 0 { None } else { Some(ts) };
        }

        job.next_run = if job.oneshot {
            if schedule_changed || enabled_requested || job.enabled {
                let next = parse_oneshot_schedule_at(&job.schedule, now_ms)
                    .map_err(|error| format!("schedule: {error}"))?;
                if job.enabled {
                    Some(next)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            job.jitter_max_ms =
                cap_jitter_max_ms_at(&job.schedule, job.jitter_max_ms, job.tz.as_deref(), now_ms)?;
            let next = compute_next_run_jittered_at(
                &job.schedule,
                job.jitter_max_ms,
                job.tz.as_deref(),
                now_ms,
            )?;
            if job.enabled {
                next
            } else {
                None
            }
        };

        Ok(Prepared::new(CronChange::Patch(job.clone()), job))
    }

    pub(crate) fn prepare_delete(
        &self,
        id: &str,
        keep_history: bool,
    ) -> Result<Prepared<()>, String> {
        let job = self
            .get(id)
            .ok_or_else(|| format!("Cron job not found: {}", id))?;
        if job.protected {
            return Err(format!(
                "Cron job '{}' is protected and cannot be deleted. Use disable instead.",
                job.name
            ));
        }
        Ok(Prepared::new(
            CronChange::Remove {
                id: id.to_string(),
                keep_history,
            },
            (),
        ))
    }

    /// Removes every job of the scope with its history; protection does not apply.
    pub(crate) fn prepare_delete_scope(
        &self,
        scope: CronScope,
    ) -> Result<Prepared<Vec<CronJob>>, String> {
        let removed: Vec<CronJob> = self
            .file
            .jobs
            .iter()
            .filter(|job| scope.contains(job))
            .cloned()
            .collect();
        if removed.is_empty() {
            return Ok(Prepared::unchanged(removed));
        }
        Ok(Prepared::new(CronChange::RemoveScope(scope), removed))
    }

    pub(crate) fn prepare_set_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<Prepared<CronJob>, String> {
        let now_ms = self.now_ms();
        let mut job = self
            .get(id)
            .cloned()
            .ok_or_else(|| format!("Cron job not found: {}", id))?;
        job.enabled = enabled;
        job.next_run = if !enabled {
            None
        } else if job.oneshot {
            Some(parse_oneshot_schedule_at(&job.schedule, now_ms)?)
        } else {
            compute_next_run_jittered_at(
                &job.schedule,
                job.jitter_max_ms,
                job.tz.as_deref(),
                now_ms,
            )?
        };
        if enabled {
            job.consecutive_failures = 0;
        }
        Ok(Prepared::new(CronChange::Patch(job.clone()), job))
    }

    pub(crate) fn prepare_set_protected(
        &self,
        id: &str,
        protected: bool,
    ) -> Result<Prepared<CronJob>, String> {
        let mut job = self
            .get(id)
            .cloned()
            .ok_or_else(|| format!("Cron job not found: {}", id))?;
        job.protected = protected;
        Ok(Prepared::new(CronChange::Patch(job.clone()), job))
    }

    /// Moves a recurring job whose slot went stale to its next future slot without firing.
    pub(crate) fn prepare_skip_stale_fire(&self, job_id: &str) -> Result<Prepared<bool>, String> {
        let now_ms = self.now_ms();
        let Some(job) = self.get(job_id) else {
            return Ok(Prepared::unchanged(false));
        };
        let stale = job.enabled
            && job
                .next_run
                .is_some_and(|next| is_stale_fire(next, now_ms, job.oneshot));
        if !stale {
            return Ok(Prepared::unchanged(false));
        }
        let mut job = job.clone();
        job.next_run = compute_next_run_jittered_at(
            &job.schedule,
            job.jitter_max_ms,
            job.tz.as_deref(),
            now_ms,
        )?;
        Ok(Prepared::new(CronChange::Patch(job), true))
    }

    /// Schedule advancement is fire-time-only; completion never re-derives next_run.
    pub(crate) fn prepare_record_run(
        &self,
        job_id: &str,
        run: CronRunInfo,
    ) -> Result<Prepared<()>, String> {
        let store_now_ms = self.now_ms();
        let Some(job) = self.get(job_id) else {
            return Ok(Prepared::unchanged(()));
        };
        let mut job = job.clone();
        job.last_run = Some(run.clone());
        apply_failure_accounting(&mut job, job_id, &run.status, store_now_ms)?;
        if !matches!(run.status, CronRunStatus::Failed) {
            if job.oneshot {
                job.next_run = None;
                job.enabled = false;
            } else if job.enabled {
                job.next_run = compute_next_run_jittered_at(
                    &job.schedule,
                    job.jitter_max_ms,
                    job.tz.as_deref(),
                    store_now_ms,
                )?;
            }
        }
        Ok(Prepared::new(CronChange::StartRun { job, run }, ()))
    }

    /// The real completion path. Idempotent: once a run has a terminal status, later calls
    /// update timestamps but skip accounting, so only the first is `fresh`.
    pub(crate) fn prepare_finish_run(
        &self,
        job_id: &str,
        run_id: &str,
        status: CronRunStatus,
        session: Option<RunSession<'_>>,
    ) -> Result<Prepared<RunFinished>, String> {
        self.prepare_finish_run_with_summary(job_id, run_id, status, session, None)
    }

    pub(crate) fn prepare_finish_run_with_summary(
        &self,
        job_id: &str,
        run_id: &str,
        status: CronRunStatus,
        session: Option<RunSession<'_>>,
        summary: Option<&str>,
    ) -> Result<Prepared<RunFinished>, String> {
        let now = self.now_ms();
        let mut edit = self.edit_run(job_id, run_id);
        if let Some(session) = session {
            edit.each(|run| set_session(run, session));
        }
        if let Some(summary) = summary {
            edit.each(|run| run.summary = Some(summary.to_string()));
        }
        let mut prior: Option<CronRunStatus> = None;
        let mut started_at: Option<u64> = None;
        edit.each(|run| {
            if prior.is_none() {
                prior = Some(run.status.clone());
            }
            if started_at.is_none() {
                started_at = Some(run.started_at);
            }
            run.status = status.clone();
            run.finished_at = Some(now);
        });
        // First terminal status wins — a racing duplicate completion call for the
        // same run must not double-count failures or re-fire a notification.
        let already_terminal = matches!(
            prior,
            Some(CronRunStatus::Failed)
                | Some(CronRunStatus::Success)
                | Some(CronRunStatus::Cancelled)
        );
        let mut fresh = false;
        if !already_terminal {
            if let Some(job) = edit.job.as_mut() {
                apply_failure_accounting(job, job_id, &status, now)?;
                fresh = true;
            }
        }
        let finished = RunFinished {
            fresh,
            elapsed_ms: started_at.map(|started| now.saturating_sub(started)),
        };
        Ok(edit.into_prepared(self, finished))
    }

    pub(crate) fn prepare_set_run_session(
        &self,
        job_id: &str,
        run_id: &str,
        session: RunSession<'_>,
    ) -> Prepared<()> {
        let mut edit = self.edit_run(job_id, run_id);
        edit.each(|run| set_session(run, session));
        edit.into_prepared(self, ())
    }

    pub(crate) fn prepare_set_run_report(
        &self,
        job_id: &str,
        run_id: &str,
        summary: &str,
        delivery: &str,
    ) -> Prepared<()> {
        let mut edit = self.edit_run(job_id, run_id);
        edit.each(|run| {
            run.summary = Some(summary.to_string());
            run.report_delivery = Some(delivery.to_string());
        });
        edit.into_prepared(self, ())
    }

    /// Records the tab a dispatched run was spawned into.
    pub(crate) fn prepare_set_run_tab(
        &self,
        job_id: &str,
        run_id: &str,
        tab_id: &str,
    ) -> Prepared<()> {
        let mut edit = self.edit_run(job_id, run_id);
        edit.each(|run| run.tab_id = Some(tab_id.to_string()));
        edit.into_prepared(self, ())
    }

    /// Cancels every unfinished run of the job, in its history and as its last run;
    /// returns each cancelled run with its elapsed time.
    pub(crate) fn prepare_cancel_running(
        &self,
        job_id: &str,
        now_ms: u64,
    ) -> Result<Prepared<Vec<(String, u64)>>, String> {
        let original = self
            .get(job_id)
            .ok_or_else(|| format!("Cron job not found: {}", job_id))?;
        let unfinished = |run: &CronRunInfo| {
            matches!(
                run.status,
                CronRunStatus::Running | CronRunStatus::AwaitingOwnerPrompt
            )
        };
        let cancel = |run: &mut CronRunInfo| {
            run.status = CronRunStatus::Cancelled;
            run.finished_at = Some(now_ms);
        };
        let mut cancelled = Vec::new();
        let mut history = Vec::new();
        for run in self.file.history.get(job_id).into_iter().flatten() {
            if unfinished(run) {
                cancelled.push((run.run_id.clone(), now_ms.saturating_sub(run.started_at)));
                let mut next = run.clone();
                cancel(&mut next);
                history.push(next);
            }
        }
        let mut job = original.clone();
        if let Some(last) = job.last_run.as_mut().filter(|last| unfinished(&**last)) {
            if !cancelled.iter().any(|(run_id, _)| *run_id == last.run_id) {
                cancelled.push((last.run_id.clone(), now_ms.saturating_sub(last.started_at)));
            }
            cancel(last);
        }
        let job = (job != *original).then_some(job);
        if history.is_empty() && job.is_none() {
            return Ok(Prepared::unchanged(cancelled));
        }
        Ok(Prepared::new(
            CronChange::UpdateRuns {
                job_id: job_id.to_string(),
                history,
                job,
            },
            cancelled,
        ))
    }

    pub fn create(&mut self, params: CronCreateParams) -> Result<CronJob, String> {
        let prepared = self.prepare_create(params);
        self.legacy(prepared)
    }

    pub fn update(&mut self, id: &str, patch: CronUpdateParams) -> Result<CronJob, String> {
        let prepared = self.prepare_update(id, patch);
        self.legacy(prepared)
    }

    pub fn delete_for_workspace(&mut self, workspace_id: &str) -> Result<Vec<CronJob>, String> {
        let prepared = self.prepare_delete_scope(CronScope::Workspace(workspace_id.to_string()));
        self.legacy(prepared)
    }

    /// Typed Team teardown for the integration owner: the Team's jobs and their history.
    pub fn delete_for_team(&mut self, team_id: &str) -> Result<Vec<CronJob>, String> {
        let prepared = self.prepare_delete_scope(CronScope::Team(team_id.to_string()));
        self.legacy(prepared)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let prepared = self.prepare_delete(id, false);
        self.legacy(prepared)
    }

    pub fn delete_preserving_history(&mut self, id: &str) -> Result<(), String> {
        let prepared = self.prepare_delete(id, true);
        self.legacy(prepared)
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<CronJob, String> {
        let prepared = self.prepare_set_enabled(id, enabled);
        self.legacy(prepared)
    }

    pub fn set_protected(&mut self, id: &str, protected: bool) -> Result<CronJob, String> {
        let prepared = self.prepare_set_protected(id, protected);
        self.legacy(prepared)
    }

    pub fn record_run(&mut self, job_id: &str, run: CronRunInfo) -> Result<(), String> {
        let prepared = self.prepare_record_run(job_id, run);
        self.legacy(prepared)
    }

    /// Returns whether this call applied a fresh terminal transition (i.e. whether the
    /// caller should treat this as a real completion event — e.g. fire a notification).
    pub fn update_run_status(
        &mut self,
        job_id: &str,
        run_id: &str,
        status: CronRunStatus,
    ) -> Result<bool, String> {
        let prepared = self.prepare_finish_run(job_id, run_id, status.clone(), None);
        let finished = self.legacy(prepared)?;
        log_finished(job_id, run_id, &status, finished);
        Ok(finished.fresh)
    }

    pub fn set_run_session(
        &mut self,
        job_id: &str,
        run_id: &str,
        session_id: &str,
        cwd: Option<&str>,
    ) {
        let prepared = self.prepare_set_run_session(job_id, run_id, RunSession { session_id, cwd });
        let _ = self.legacy(Ok(prepared));
    }

    pub fn set_run_report(
        &mut self,
        job_id: &str,
        run_id: &str,
        summary: &str,
        delivery: String,
    ) -> Result<(), String> {
        let prepared = self.prepare_set_run_report(job_id, run_id, summary, &delivery);
        self.legacy(Ok(prepared))
    }
}

fn is_active_run(run: &CronRunInfo) -> bool {
    matches!(
        run.status,
        CronRunStatus::Running | CronRunStatus::AwaitingOwnerPrompt
    )
}

fn set_session(run: &mut CronRunInfo, session: RunSession<'_>) {
    run.session_id = Some(session.session_id.to_string());
    if let Some(cwd) = session.cwd {
        run.cwd = Some(cwd.to_string());
    }
}

/// service::cron_complete parity: dashboards and grep-based observability need every
/// Failed/Cancelled run.
pub(crate) fn log_finished(job_id: &str, run_id: &str, status: &CronRunStatus, run: RunFinished) {
    if matches!(status, CronRunStatus::Failed | CronRunStatus::Cancelled) {
        log::info!(
            "Cron run completed: job={}, run={}, status={:?}, elapsed_ms={}",
            job_id,
            run_id,
            status,
            run.elapsed_ms.unwrap_or(0)
        );
    }
}

fn normalize_reasoning_effort(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Increments the failure streak (+backoff, +auto-disable) on `Failed`, resets
/// to 0 on `Success`. Shared by run starts and completions.
fn apply_failure_accounting(
    job: &mut CronJob,
    job_id: &str,
    status: &CronRunStatus,
    now_ms: u64,
) -> Result<(), String> {
    match status {
        CronRunStatus::Failed => {
            job.consecutive_failures = job.consecutive_failures.saturating_add(1);
            if job.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                job.enabled = false;
                job.next_run = None;
                log::warn!(
                    "[cron] auto-disabled job {} after {} consecutive failures",
                    job_id,
                    job.consecutive_failures
                );
            } else if !job.oneshot && job.enabled {
                let base_ms = compute_next_run_jittered_at(
                    &job.schedule,
                    Some(0),
                    job.tz.as_deref(),
                    now_ms,
                )?
                .map(|t| t.saturating_sub(now_ms))
                .unwrap_or(60_000);
                let backoff = compute_backoff(job.consecutive_failures, base_ms);
                job.next_run = Some(now_ms + backoff);
                log::info!(
                    "[cron] {} backoff after failure {}/{}: +{}ms",
                    job_id,
                    job.consecutive_failures,
                    MAX_CONSECUTIVE_FAILURES,
                    backoff
                );
            }
        }
        CronRunStatus::Success => {
            job.consecutive_failures = 0;
        }
        _ => {}
    }
    Ok(())
}

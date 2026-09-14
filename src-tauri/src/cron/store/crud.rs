//! CronStore CRUD: create / read / update / delete + run history recording.

use super::{
    compute_backoff, compute_next_run_jittered_at, default_max_history, parse_oneshot_schedule_at,
    validate_schedule, validate_tz, CronCreateParams, CronJob, CronRunInfo, CronRunStatus,
    CronStore, CronUpdateParams, MAX_CONSECUTIVE_FAILURES,
};

impl CronStore {
    pub fn create(&mut self, params: CronCreateParams) -> Result<CronJob, String> {
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
        let next_run = if !params.enabled.unwrap_or(true) {
            if oneshot {
                parse_oneshot_schedule_at(&params.schedule, now_ms)?;
            } else {
                validate_schedule(&params.schedule)?;
            }
            None
        } else if oneshot {
            Some(parse_oneshot_schedule_at(&params.schedule, now_ms)?)
        } else {
            validate_schedule(&params.schedule)?;
            compute_next_run_jittered_at(&params.schedule, jitter, tz.as_deref(), now_ms)?
        };

        let job = CronJob {
            id: uuid::Uuid::new_v4().to_string(),
            name: params.name,
            description: params.description,
            workspace_id: params.workspace_id,
            team_id: params.team_id,
            schedule: params.schedule,
            enabled: params.enabled.unwrap_or(true),
            agent: params
                .agent
                .unwrap_or_else(crate::config::default_agent_provider),
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
        };

        self.file.jobs.push(job.clone());
        self.save()?;
        Ok(job)
    }

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

    pub fn get_mut(&mut self, id: &str) -> Option<&mut CronJob> {
        self.file.jobs.iter_mut().find(|j| j.id == id)
    }

    pub fn get_history_mut(&mut self, job_id: &str) -> Option<&mut Vec<CronRunInfo>> {
        self.file.history.get_mut(job_id)
    }

    pub fn update(&mut self, id: &str, patch: CronUpdateParams) -> Result<CronJob, String> {
        let now_ms = self.now_ms();
        let job = self
            .file
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or_else(|| format!("Cron job not found: {}", id))?;

        if let Some(name) = patch.name {
            job.name = name;
        }
        if let Some(desc) = patch.description {
            job.description = Some(desc);
        }
        if let Some(schedule) = patch.schedule {
            if job.oneshot {
                parse_oneshot_schedule_at(&schedule, now_ms)?;
            } else {
                validate_schedule(&schedule)?;
            }
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

        job.next_run = if !job.enabled {
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

        let updated = job.clone();
        self.save()?;
        Ok(updated)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        if let Some(job) = self.file.jobs.iter().find(|j| j.id == id) {
            if job.protected {
                return Err(format!(
                    "Cron job '{}' is protected and cannot be deleted. Use disable instead.",
                    job.name
                ));
            }
        }
        let before = self.file.jobs.len();
        self.file.jobs.retain(|j| j.id != id);
        if self.file.jobs.len() == before {
            return Err(format!("Cron job not found: {}", id));
        }
        self.file.history.remove(id);
        self.save()
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<CronJob, String> {
        let now_ms = self.now_ms();
        let job = self
            .file
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
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

        let updated = job.clone();
        self.save()?;
        Ok(updated)
    }

    pub fn record_run(&mut self, job_id: &str, run: CronRunInfo) -> Result<(), String> {
        let store_now_ms = self.now_ms();
        if let Some(job) = self.file.jobs.iter_mut().find(|j| j.id == job_id) {
            job.last_run = Some(run.clone());
            apply_failure_accounting(job, job_id, &run.status, store_now_ms)?;

            // Schedule advancement is fire-time-only; completion never re-derives next_run.
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

            let max = job.max_history;
            let history = self.file.history.entry(job_id.to_string()).or_default();
            history.push(run);
            while history.len() > max {
                history.remove(0);
            }
        }
        self.save()
    }

    /// The real completion path (run_watcher, cron_complete, pty-dead fallbacks). Idempotent:
    /// once a run has a terminal status, later calls update timestamps but skip accounting.
    /// Returns whether this call applied a fresh terminal transition (i.e. whether the caller
    /// should treat this as a real completion event — e.g. fire a notification).
    pub fn update_run_status(
        &mut self,
        job_id: &str,
        run_id: &str,
        status: CronRunStatus,
    ) -> Result<bool, String> {
        let now = self.now_ms();
        let mut started_at: Option<u64> = None;
        let mut prior_status: Option<CronRunStatus> = None;
        if let Some(history) = self.file.history.get_mut(job_id) {
            if let Some(run) = history.iter_mut().find(|r| r.run_id == run_id) {
                prior_status = Some(run.status.clone());
                run.status = status.clone();
                run.finished_at = Some(now);
                started_at = Some(run.started_at);
            }
        }
        if let Some(job) = self.file.jobs.iter_mut().find(|j| j.id == job_id) {
            if let Some(ref mut lr) = job.last_run {
                if lr.run_id == run_id {
                    if prior_status.is_none() {
                        prior_status = Some(lr.status.clone());
                    }
                    lr.status = status.clone();
                    lr.finished_at = Some(now);
                    if started_at.is_none() {
                        started_at = Some(lr.started_at);
                    }
                }
            }
        }

        // First terminal status wins — a racing duplicate completion call for the
        // same run must not double-count failures or re-fire a notification.
        let already_terminal = matches!(
            prior_status,
            Some(CronRunStatus::Failed)
                | Some(CronRunStatus::Success)
                | Some(CronRunStatus::Cancelled)
        );
        let mut fresh_transition = false;
        if !already_terminal {
            if let Some(job) = self.file.jobs.iter_mut().find(|j| j.id == job_id) {
                apply_failure_accounting(job, job_id, &status, now)?;
                fresh_transition = true;
            }
        }

        // service::cron_complete). Without this, dashboards and grep-based observability miss every Failed/Cancelled run.
        let elapsed_ms = started_at.map(|s| now.saturating_sub(s));
        match status {
            CronRunStatus::Failed | CronRunStatus::Cancelled => {
                log::info!(
                    "Cron run completed: job={}, run={}, status={:?}, elapsed_ms={}",
                    job_id,
                    run_id,
                    status,
                    elapsed_ms.unwrap_or(0)
                );
            }
            _ => {}
        }

        self.save()?;
        Ok(fresh_transition)
    }

    pub fn set_run_session(
        &mut self,
        job_id: &str,
        run_id: &str,
        session_id: &str,
        cwd: Option<&str>,
    ) {
        if let Some(history) = self.file.history.get_mut(job_id) {
            if let Some(run) = history.iter_mut().find(|r| r.run_id == run_id) {
                run.session_id = Some(session_id.to_string());
                if let Some(c) = cwd {
                    run.cwd = Some(c.to_string());
                }
            }
        }
        if let Some(job) = self.file.jobs.iter_mut().find(|j| j.id == job_id) {
            if let Some(ref mut lr) = job.last_run {
                if lr.run_id == run_id {
                    lr.session_id = Some(session_id.to_string());
                    if let Some(c) = cwd {
                        lr.cwd = Some(c.to_string());
                    }
                }
            }
        }
        let _ = self.save();
    }

    pub fn get_history(&self, job_id: &str) -> Vec<&CronRunInfo> {
        self.file
            .history
            .get(job_id)
            .map(|h| h.iter().collect())
            .unwrap_or_default()
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
}

fn normalize_reasoning_effort(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Increments the failure streak (+backoff, +auto-disable) on `Failed`, resets
/// to 0 on `Success`. Shared by `record_run` and `update_run_status`.
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

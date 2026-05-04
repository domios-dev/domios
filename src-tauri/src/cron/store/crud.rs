//! CronStore CRUD: create / read / update / delete + run history recording.

use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    compute_backoff, compute_next_run_jittered, default_max_history, parse_oneshot_schedule,
    validate_schedule, CronCreateParams, CronJob, CronRunInfo, CronRunStatus, CronStore,
    CronUpdateParams, MAX_CONSECUTIVE_FAILURES,
};

impl CronStore {
    pub(super) fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub fn create(&mut self, params: CronCreateParams) -> Result<CronJob, String> {
        let oneshot = params.oneshot.unwrap_or(false);
        let jitter = params.jitter_max_ms;

        let next_run = if !params.enabled.unwrap_or(true) {
            if oneshot {
                parse_oneshot_schedule(&params.schedule)?;
            } else {
                validate_schedule(&params.schedule)?;
            }
            None
        } else if oneshot {
            Some(parse_oneshot_schedule(&params.schedule)?)
        } else {
            validate_schedule(&params.schedule)?;
            compute_next_run_jittered(&params.schedule, jitter)?
        };

        let job = CronJob {
            id: uuid::Uuid::new_v4().to_string(),
            name: params.name,
            description: params.description,
            workspace_id: params.workspace_id,
            schedule: params.schedule,
            enabled: params.enabled.unwrap_or(true),
            agent: params.agent.unwrap_or_else(crate::config::default_agent_provider),
            model: params.model,
            task: params.task,
            cwd: params.cwd,
            role: params.role,
            creator_pane_id: params.creator_pane_id,
            oneshot,
            max_history: params.max_history.unwrap_or(default_max_history()),
            created_at: Self::now_ms(),
            last_run: None,
            next_run,
            jitter_max_ms: jitter,
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
                parse_oneshot_schedule(&schedule)?;
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
        if let Some(jitter) = patch.jitter_max_ms {
            job.jitter_max_ms = Some(jitter);
        }

        job.next_run = if !job.enabled {
            None
        } else if job.oneshot {
            Some(parse_oneshot_schedule(&job.schedule)?)
        } else {
            compute_next_run_jittered(&job.schedule, job.jitter_max_ms)?
        };

        let updated = job.clone();
        self.save()?;
        Ok(updated)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        if let Some(job) = self.file.jobs.iter().find(|j| j.id == id) {
            if job.protected {
                return Err(format!("Cron job '{}' is protected and cannot be deleted. Use disable instead.", job.name));
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
            Some(parse_oneshot_schedule(&job.schedule)?)
        } else {
            compute_next_run_jittered(&job.schedule, job.jitter_max_ms)?
        };
        if enabled {
            job.consecutive_failures = 0;
        }

        let updated = job.clone();
        self.save()?;
        Ok(updated)
    }

    pub fn record_run(&mut self, job_id: &str, run: CronRunInfo) -> Result<(), String> {
        if let Some(job) = self.file.jobs.iter_mut().find(|j| j.id == job_id) {
            let was_failure = matches!(run.status, CronRunStatus::Failed);
            let was_success = matches!(run.status, CronRunStatus::Success);
            job.last_run = Some(run.clone());

            if was_failure {
                job.consecutive_failures = job.consecutive_failures.saturating_add(1);
                if job.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    job.enabled = false;
                    job.next_run = None;
                    log::warn!(
                        "[cron] auto-disabled job {} after {} consecutive failures",
                        job_id, job.consecutive_failures
                    );
                } else if !job.oneshot && job.enabled {
                    let now_ms = Self::now_ms();
                    let base_ms = compute_next_run_jittered(&job.schedule, Some(0))?
                        .map(|t| t.saturating_sub(now_ms))
                        .unwrap_or(60_000);
                    let backoff = compute_backoff(job.consecutive_failures, base_ms);
                    job.next_run = Some(now_ms + backoff);
                    log::info!(
                        "[cron] {} backoff after failure {}/{}: +{}ms",
                        job_id, job.consecutive_failures, MAX_CONSECUTIVE_FAILURES, backoff
                    );
                }
            } else {
                if was_success {
                    job.consecutive_failures = 0;
                }
                // Recompute next_run normally (oneshot: no next run after firing)
                if job.oneshot {
                    job.next_run = None;
                    job.enabled = false;
                } else if job.enabled {
                    job.next_run = compute_next_run_jittered(&job.schedule, job.jitter_max_ms)?;
                }
            }

            let max = job.max_history;
            let history = self.file.history.entry(job_id.to_string()).or_default();
            history.push(run);
            // Evict old entries
            while history.len() > max {
                history.remove(0);
            }
        }
        self.save()
    }

    pub fn update_run_status(
        &mut self,
        job_id: &str,
        run_id: &str,
        status: CronRunStatus,
    ) -> Result<(), String> {
        let now = Self::now_ms();
        if let Some(history) = self.file.history.get_mut(job_id) {
            if let Some(run) = history.iter_mut().find(|r| r.run_id == run_id) {
                run.status = status.clone();
                run.finished_at = Some(now);
            }
        }
        if let Some(job) = self.file.jobs.iter_mut().find(|j| j.id == job_id) {
            if let Some(ref mut lr) = job.last_run {
                if lr.run_id == run_id {
                    lr.status = status;
                    lr.finished_at = Some(now);
                }
            }
        }
        self.save()
    }

    pub fn get_history(&self, job_id: &str) -> Vec<&CronRunInfo> {
        self.file
            .history
            .get(job_id)
            .map(|h| h.iter().collect())
            .unwrap_or_default()
    }
}

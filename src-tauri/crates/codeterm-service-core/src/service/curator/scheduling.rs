use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mirror {
    pub provider: String,
    pub launch_profile: Option<String>,
    pub model: String,
    pub effort: Option<String>,
}

const MAX_MIRROR_CAUSE_CHARS: usize = 480;

/// Why a mirrored launch cannot start; `cause` is what the fallback round shows the owner.
#[derive(Debug)]
pub(super) struct MirrorRefusal {
    pub(super) reason: CuratorFailure,
    pub(super) cause: Option<String>,
}
impl MirrorRefusal {
    fn profile(cause: impl std::fmt::Display) -> Self {
        Self {
            reason: CuratorFailure::ProfileUnavailable,
            cause: Some(
                cause
                    .to_string()
                    .chars()
                    .take(MAX_MIRROR_CAUSE_CHARS)
                    .collect(),
            ),
        }
    }
}
impl From<CuratorFailure> for MirrorRefusal {
    fn from(reason: CuratorFailure) -> Self {
        Self {
            reason,
            cause: None,
        }
    }
}
impl From<MirrorRefusal> for String {
    fn from(refusal: MirrorRefusal) -> Self {
        refusal.cause.unwrap_or_else(|| refusal.reason.into())
    }
}

pub(crate) struct CuratorFireError {
    pub reason: CuratorFailure,
    pub(super) read_failure: bool,
    pub(super) cause: Option<String>,
    detail: String,
}
impl From<CuratorFailure> for CuratorFireError {
    fn from(reason: CuratorFailure) -> Self {
        Self {
            reason,
            read_failure: false,
            cause: None,
            detail: reason.as_str().into(),
        }
    }
}
impl From<MirrorRefusal> for CuratorFireError {
    fn from(refusal: MirrorRefusal) -> Self {
        Self {
            reason: refusal.reason,
            read_failure: false,
            detail: refusal
                .cause
                .clone()
                .unwrap_or_else(|| refusal.reason.as_str().into()),
            cause: refusal.cause,
        }
    }
}
impl From<String> for CuratorFireError {
    fn from(detail: String) -> Self {
        Self {
            reason: CuratorFailure::LaunchFailed,
            read_failure: false,
            cause: None,
            detail,
        }
    }
}
impl From<crate::service::plan::PlanError> for CuratorFireError {
    fn from(error: crate::service::plan::PlanError) -> Self {
        Self {
            reason: CuratorFailure::LaunchFailed,
            cause: None,
            detail: error.to_string(),
            read_failure: true,
        }
    }
}
impl From<&str> for CuratorFireError {
    fn from(detail: &str) -> Self {
        detail.to_string().into()
    }
}
impl std::fmt::Display for CuratorFireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)
    }
}

pub fn resolve_mirror(tab: &crate::multiplexer::types::Tab) -> Result<Mirror, CuratorFailure> {
    if crate::service::hierarchy::heartbeat_gate::binding_digest(tab, None).is_none() {
        return Err(CuratorFailure::OrchestratorUnbound);
    }
    Ok(Mirror {
        provider: tab
            .agent_type
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or(CuratorFailure::MirrorUnavailable)?,
        launch_profile: crate::service::agent_runtime::launch_profile::of_tab(tab),
        model: tab
            .agent_session_model
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or(CuratorFailure::MirrorUnavailable)?,
        effort: tab
            .reasoning_effort_identity
            .actual
            .clone()
            .or_else(|| tab.reasoning_effort_identity.resolved.clone()),
    })
}

fn waits_for_orchestrator(reason: Option<CuratorFailure>) -> bool {
    matches!(
        reason,
        Some(CuratorFailure::OrchestratorAbsent | CuratorFailure::OrchestratorUnbound)
    )
}

fn reconcile_idle_loop(
    state: &mut ControlLoop,
    mirror_failure: Option<CuratorFailure>,
    group_close: Option<u64>,
    now: u64,
) {
    match mirror_failure {
        Some(reason) => state.fallback_reason = Some(reason),
        None if waits_for_orchestrator(state.fallback_reason) => state.fallback_reason = None,
        None => {}
    }
    if waits_for_orchestrator(state.fallback_reason) {
        state.next_at = None;
        return;
    }
    if group_close.is_some_and(|at| {
        state
            .last_synthesis_group_close_at
            .is_none_or(|prior| at > prior)
    }) {
        let wake = now.saturating_add(1000);
        state.next_at = Some(state.next_at.map_or(wake, |next| next.min(wake)));
    }
    let bound = now.saturating_add(codeterm_types::plan::CURATOR_MAX_DELAY_SECS * 1000);
    state.next_at = Some(
        state
            .next_at
            .unwrap_or_else(|| now.saturating_add(curator_delay(None) * 1000))
            .min(bound),
    );
}

#[cfg(test)]
mod tests;

impl ServiceContext {
    fn wake_curator_schedule(&self) {
        if let Some(wake) = self
            .extensions
            .try_get::<crate::cron::scheduler::CronWake>()
        {
            wake.notify_all();
        }
    }
    /// The cron launch skips spawn validation, so the profile and its selections are proven here.
    pub(super) fn available_mirror(&self, mirror: Mirror) -> Result<Mirror, MirrorRefusal> {
        if self
            .agent_registry
            .with_provider(&mirror.provider, |_| ())
            .is_none()
            && !self
                .chatbackend_registry
                .get(&mirror.provider)
                .is_some_and(|backend| backend.spawnable())
        {
            return Err(CuratorFailure::ProviderUnavailable.into());
        }
        self.validate_launch_profile(&mirror.provider, mirror.launch_profile.as_deref())
            .map_err(MirrorRefusal::profile)?;
        let mut selection = crate::service::SpawnAgentParams {
            launch_profile: mirror.launch_profile.clone(),
            model: Some(mirror.model.clone()),
            reasoning_effort: mirror.effort.clone(),
            ..Default::default()
        };
        self.resolve_spawn_profile_options(&mirror.provider, &mut selection)
            .map_err(|error| {
                MirrorRefusal::profile(format!(
                    "{}: {error}",
                    mirror.launch_profile.as_deref().unwrap_or_default()
                ))
            })?;
        Ok(Mirror {
            model: selection.model.unwrap_or(mirror.model),
            effort: selection.reasoning_effort,
            ..mirror
        })
    }
    pub(crate) fn curator_fire_failed(
        &self,
        plan_id: &str,
        reason: CuratorFailure,
        now: u64,
    ) -> Result<(), String> {
        self.curator_fire_refused(plan_id, reason, None, now)
    }
    pub(crate) fn curator_fire_refused(
        &self,
        plan_id: &str,
        reason: CuratorFailure,
        cause: Option<&str>,
        now: u64,
    ) -> Result<(), String> {
        if waits_for_orchestrator(Some(reason)) {
            self.plan_store
                .edit_curator_state(plan_id, now, |plan| {
                    plan.control_loop.fallback_reason = Some(reason);
                    plan.control_loop.next_at = None;
                    Ok(())
                })
                .map_err(|error| error.to_string())?;
            self.events
                .publish_internal("curator-reconcile", json!({"plan_id":plan_id}));
            return Ok(());
        }
        let round = self
            .plan_store
            .edit_curator_round(plan_id, now, |plan| {
                let state = &mut plan.control_loop;
                state.consecutive_failures += 1;
                state.fallback_reason = Some(reason);
                let number = state
                    .running
                    .as_ref()
                    .map(|r| r.number)
                    .or_else(|| state.last_round.as_ref().map(|r| r.number + 1))
                    .unwrap_or(1);
                state.running = None;
                let round = CuratorRound {
                    number,
                    started_at: now,
                    ended_at: now,
                    outcome: codeterm_types::plan::CuratorOutcome::Fallback,
                    summary: cause.unwrap_or(reason.as_str()).into(),
                    reason: reason.as_str().into(),
                    next_delay_secs: curator_delay(None),
                    ..Default::default()
                };
                plan.append_round(round.clone());
                Ok(round)
            })
            .map_err(|e| e.to_string())?;
        self.journal_curator(plan_id, &round);
        Ok(())
    }

    pub(super) fn reconcile_curator_plan(
        &self,
        cron: &crate::CronState,
        read: &crate::service::plan::CuratorScheduleRead,
        now: u64,
    ) -> Result<(), String> {
        let plan = read.metadata();
        let suspended = self.curator_suspended_reason_for_fields(
            &plan.mode,
            &plan.status,
            &plan.team_id,
            read.complete(),
        );
        let eligible = read.eligible(
            self.configuration().orchestration.plan_reminders,
            suspended.is_some(),
        );
        let mut loop_state = plan.control_loop.clone();
        if eligible && loop_state.running.is_some() {
            loop_state.agent_states = super::movement_sources::observed_agents(
                self,
                &plan.team_id,
                &plan.control_loop.agent_states,
                now,
            );
        }
        if loop_state.cron_id.is_none() {
            loop_state.cron_id = cron
                .lock()
                .list()
                .iter()
                .find(|job| {
                    job.protected
                        && job.role.as_deref() == Some("curator")
                        && job.team_id == plan.team_id
                        && job.task == plan.id
                })
                .map(|job| job.id.clone());
        }
        loop_state.suspended_reason = suspended;
        if let Some(run) = &loop_state.running {
            let ended = loop_state
                .cron_id
                .as_ref()
                .and_then(|id| cron.lock().get(id).and_then(|job| job.last_run.clone()))
                .filter(|r| {
                    r.started_at >= run.started_at
                        && !matches!(
                            r.status,
                            CronRunStatus::Running | CronRunStatus::AwaitingOwnerPrompt
                        )
                });
            if ended.is_some() || now.saturating_sub(run.started_at) >= CURATOR_TIMEOUT_MS {
                let deadline = now.saturating_sub(run.started_at) >= CURATOR_TIMEOUT_MS;
                let reason = if deadline {
                    "deadline"
                } else {
                    "run_ended_without_finish"
                };
                let round = CuratorRound {
                    number: run.number,
                    run_id: run.run_id.clone(),
                    tab_id: run.tab_id.clone(),
                    started_at: run.started_at,
                    ended_at: now,
                    outcome: if deadline {
                        codeterm_types::plan::CuratorOutcome::Partial
                    } else {
                        codeterm_types::plan::CuratorOutcome::Failed
                    },
                    reason: reason.into(),
                    summary: reason.into(),
                    next_delay_secs: curator_delay(None),
                    digest_hash: run.digest_hash.clone(),
                    section_sizes: run.section_sizes.clone(),
                    turns: run.turns,
                    output_tokens: run.output_tokens,
                    save_id: run.save_id.clone(),
                    synthesis: run.synthesis,
                    duration_ms: now.saturating_sub(run.started_at),
                    ..Default::default()
                };
                if run.synthesis_group_close_at.is_some() {
                    loop_state.last_synthesis_group_close_at = run.synthesis_group_close_at;
                }
                loop_state.running = None;
                loop_state.consecutive_failures += 1;
                loop_state.fallback_reason = Some(CuratorFailure::ConsecutiveFailures);
                loop_state.last_real_round_at = Some(now);
                loop_state.next_at = Some(now + curator_delay(None) * 1000);
                let applied = self
                    .plan_store
                    .edit_curator_round(&plan.id, now, |p| {
                        if !read.apply_control_loop(&mut p.control_loop, loop_state.clone())? {
                            return Ok(false);
                        }
                        p.append_round(round.clone());
                        Ok(true)
                    })
                    .map_err(|e| e.to_string())?;
                if !applied {
                    self.events
                        .publish_internal("curator-reconcile", json!({"plan_id":plan.id}));
                    return Ok(());
                }
                self.journal_curator(&plan.id, &round);
                return Ok(());
            }
        }
        let mut mirror_cause = None;
        if eligible && loop_state.running.is_none() {
            let tab = plan
                .owner
                .tab_id
                .as_ref()
                .and_then(|id| self.mux.read().get_tab(id).cloned());
            let mirror = tab
                .as_ref()
                .ok_or(CuratorFailure::OrchestratorAbsent)
                .and_then(resolve_mirror)
                .map_err(MirrorRefusal::from)
                .and_then(|mirror| self.available_mirror(mirror));
            let failure = match mirror {
                Ok(_) => None,
                Err(refusal) => {
                    mirror_cause = refusal.cause;
                    Some(refusal.reason)
                }
            };
            reconcile_idle_loop(&mut loop_state, failure, read.group_close(), now);
        }
        if !eligible || waits_for_orchestrator(loop_state.fallback_reason) {
            loop_state.next_at = None;
            if let Some(id) = &loop_state.cron_id {
                let job = { cron.lock().get(id).cloned() };
                if let Some(mut job) = job {
                    if job.enabled {
                        job.enabled = false;
                        job.next_run = None;
                        commit(cron, |_| {
                            Ok(Prepared::new(CronChange::Patch(job.clone()), ()))
                        })
                        .map_err(|e| e.to_string())?;
                        self.wake_curator_schedule();
                    }
                }
            }
        } else if loop_state.running.is_none() {
            let next = loop_state.next_at.ok_or("Curator wake time absent")?;
            let existing = loop_state
                .cron_id
                .as_ref()
                .and_then(|id| cron.lock().get(id).cloned());
            let job_running = existing
                .as_ref()
                .is_some_and(|job| cron.lock().is_job_running(&job.id));
            if !job_running
                && existing
                    .as_ref()
                    .is_none_or(|job| job.next_run != Some(next) || !job.enabled)
            {
                let workspace = plan
                    .team_id
                    .as_deref()
                    .and_then(|team| self.mux.read().team_workspace(team))
                    .ok_or("Team workspace absent")?;
                let params = CronCreateParams {
                    name: format!("curator:{}", plan.id),
                    workspace_id: workspace,
                    team_id: plan.team_id.clone(),
                    detach_from_team: false,
                    schedule: next.to_string(),
                    task: plan.id.clone(),
                    description: None,
                    agent: Some("mirror:orchestrator".into()),
                    model: None,
                    reasoning_effort: None,
                    cwd: Some(plan.project_path.clone()),
                    role: Some(
                        crate::service::role_policy::RoleProfile::Curator
                            .as_str()
                            .into(),
                    ),
                    skill: None,
                    creator_tab_id: plan.owner.tab_id.clone(),
                    report_target_tab_id: None,
                    enabled: Some(true),
                    max_history: None,
                    oneshot: Some(true),
                    jitter_max_ms: Some(0),
                    tz: None,
                    timeout_seconds: Some(CURATOR_TIMEOUT_SECS),
                };
                let job = commit(cron, |store| {
                    if let Some(mut job) = existing.clone() {
                        job.enabled = true;
                        job.next_run = Some(next);
                        job.schedule = next.to_string();
                        Ok(Prepared::new(CronChange::Patch(job.clone()), job))
                    } else {
                        let mut prepared = store.prepare_create(params.clone())?;
                        prepared.value.protected = true;
                        prepared.changes = vec![CronChange::Create(prepared.value.clone())];
                        Ok(prepared)
                    }
                })
                .map_err(|e| e.to_string())?
                .value;
                loop_state.cron_id = Some(job.id);
                self.wake_curator_schedule();
            }
        }
        if loop_state != plan.control_loop {
            // Waiting for the owner is loop state, not a round outcome; keep last_round.
            let fallback = loop_state
                .fallback_reason
                .as_ref()
                .filter(|reason| !waits_for_orchestrator(Some(**reason)))
                .filter(|reason| plan.control_loop.fallback_reason.as_ref() != Some(reason))
                .map(|reason| CuratorRound {
                    number: loop_state
                        .last_round
                        .as_ref()
                        .map_or(1, |round| round.number + 1),
                    started_at: now,
                    ended_at: now,
                    outcome: codeterm_types::plan::CuratorOutcome::Fallback,
                    reason: reason.as_str().into(),
                    summary: mirror_cause.as_deref().unwrap_or(reason.as_str()).into(),
                    next_delay_secs: curator_delay(None),
                    ..Default::default()
                });
            let applied = self
                .plan_store
                .edit_curator_round(&plan.id, now, |p| {
                    let next = loop_state.next_at;
                    if !read.apply_control_loop(&mut p.control_loop, loop_state)? {
                        return Ok(false);
                    }
                    if let Some(round) = &fallback {
                        p.append_round(round.clone());
                        p.control_loop.next_at = next;
                    }
                    Ok(true)
                })
                .map_err(|e| e.to_string())?;
            if !applied {
                self.events
                    .publish_internal("curator-reconcile", json!({"plan_id":plan.id}));
                return Ok(());
            }
            if let Some(round) = fallback {
                self.journal_curator(&plan.id, &round);
            }
            self.emit_committed_plan_changed(&plan.id);
        }
        Ok(())
    }

    pub(crate) fn prepare_curator_fire(
        &self,
        job: &CronJob,
        now: u64,
    ) -> Result<Option<(Mirror, String)>, CuratorFireError> {
        self.extensions
            .try_get::<super::CuratorTabs>()
            .ok_or("Curator runtime is unavailable")?;
        let read = self
            .plan_store
            .curator_digest_projection(&job.task, now)
            .map_err(CuratorFireError::from)?;
        let plan = read.scope().clone();
        if !read.eligible(
            self.configuration().orchestration.plan_reminders,
            self.curator_suspended_reason_for_fields(
                &plan.mode,
                &plan.status,
                &plan.team_id,
                read.complete(),
            )
            .is_some(),
        ) || plan.running
        {
            return Ok(None);
        }
        let occupied = self.mux.read().all_tabs().any(|tab| {
            tab.team_id == plan.team_id
                && tab.role_profile.as_deref() == Some("curator")
                && (tab.agent_type.is_some() || tab.agent_booting)
        });
        if occupied {
            self.plan_store
                .edit_curator_state(&plan.id, now, |plan| {
                    plan.control_loop.next_at =
                        Some(now.saturating_add(curator_delay(None) * 1000));
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
            return Ok(None);
        }
        let owner = plan
            .owner
            .tab_id
            .as_deref()
            .ok_or(CuratorFailure::OrchestratorAbsent)?;
        let tab = self
            .mux
            .read()
            .get_tab(owner)
            .cloned()
            .ok_or(CuratorFailure::OrchestratorAbsent)?;
        let mirror = resolve_mirror(&tab)?;
        let mirror = self.available_mirror(mirror)?;
        let since = read.since();
        let mut rows: std::collections::BTreeMap<String, Vec<digest::Row>> =
            std::collections::BTreeMap::new();
        let notices = super::movement_sources::capture_notices(self, &plan.team_id);
        let candidates = self.curator_save_candidates(&plan.id, &plan.team_id, now)?;
        for (id, episodes) in candidates
            .into_iter()
            .filter(|(_, episodes)| episodes.len() >= 2)
        {
            rows.entry("candidates".into())
                .or_default()
                .push(digest::Row {
                    id,
                    kind: "blueprint".into(),
                    state: "repeated".into(),
                    summary: episodes.into_iter().collect::<Vec<_>>().join(" "),
                });
        }
        super::digest_sources::memory(self, &plan.id, since, &mut rows);
        let mut observation = crate::service::hierarchy::heartbeat_gate::Observation::default();
        observation.workers.insert(
            owner.into(),
            crate::service::hierarchy::heartbeat_gate::Worker {
                session_key: plan.owner_session_key.clone(),
                turn_cursor: plan.owner_turn_cursor.clone(),
                ..Default::default()
            },
        );
        let (messages, cursor) = crate::service::hierarchy::heartbeat_gate::collect_owner_messages(
            self,
            owner,
            &observation,
        );
        if !messages.is_empty() {
            rows.entry("owner messages".into())
                .or_default()
                .push(digest::Row {
                    id: owner.into(),
                    kind: "owner_messages".into(),
                    state: "count".into(),
                    summary: messages.len().to_string(),
                });
        }
        let flagged = super::digest_sources::blueprints(self, &plan.team_id, since, now, &mut rows);
        let number = plan
            .last_round
            .as_ref()
            .map_or(Some(1), |round| round.number.checked_add(1))
            .ok_or("Curator round number is exhausted")?;
        let counts = read.counts();
        let lanes = read.lanes();
        let handoff_age = read.latest_handoff_at().map(|at| now.saturating_sub(at));
        let mut header=format!("plan {} | {:?} | control loop curator | orchestrator {} activity {:?} | open {} in_progress {} review {} blocked {} | lanes {} pressure {} | handoff_age_ms {:?} | round {}",plan.id,plan.mode,owner,self.resolved_tab_status(&tab),counts.pending,counts.in_progress,counts.in_review,counts.blocked,lanes,if lanes>CURATOR_HIGH_LANES {"high"}else if lanes>CURATOR_ELEVATED_LANES {"elevated"}else{"normal"},handoff_age,number);
        header.push_str(&format!("\nask: {}", super::ask::command(owner)));
        let rubric = codeterm_prompts::prompts::CURATOR_RUBRIC;
        let real_rounds = read.real_rounds_started();
        let group_close = read.group_close().filter(|at| {
            plan.last_synthesis_group_close_at
                .is_none_or(|prior| *at > prior)
        });
        let synthesis =
            curator::synthesis::due(real_rounds, group_close, plan.last_synthesis_group_close_at);
        header.push_str(if synthesis {
            "\nsynthesis: yes"
        } else {
            "\nsynthesis: no"
        });
        let header = format!("{header}\n\n{rubric}");
        let header = if synthesis {
            read.synthesis_context_header(&header)
                .map_err(CuratorFireError::from)?
        } else {
            header
        };
        let digest = read
            .build_digest_with_runtime(&header, &rows, digest::MAX_DIGEST_BYTES, Some(&notices))
            .map_err(CuratorFireError::from)?;
        let action = curator::due_action(
            digest.activity || group_close.is_some() || plan.manual_run_requested,
            read.has_actionable().map_err(CuratorFireError::from)?,
            plan.last_real_round_at.or(Some(plan.created_at)),
            now,
        );
        let nudge = if action == DueAction::Nudge {
            Some(read.heartbeat_body().map_err(CuratorFireError::from)?)
        } else {
            None
        };
        drop(read);
        let answers = notices
            .iter()
            .filter(|notice| notice.answered)
            .map(|notice| notice.id.clone())
            .collect();
        self.plan_store
            .edit_curator_state(&plan.id, now, |state| {
                state.control_loop.observed_owner_answers = answers;
                if let Some(cursor) = cursor {
                    state.control_loop.owner_turn_cursor = cursor.turn_cursor;
                    state.control_loop.owner_session_key = cursor.session_key;
                }
                Ok(())
            })
            .map_err(|error| error.to_string())?;
        if action != DueAction::Spawn {
            if let Some(body) = nudge {
                self.deliver_cron_report(owner, super::curator_envelope(), &body)?;
            }
            let round = CuratorRound {
                number,
                started_at: now,
                ended_at: now,
                outcome: if action == DueAction::Quiet {
                    codeterm_types::plan::CuratorOutcome::Quiet
                } else {
                    codeterm_types::plan::CuratorOutcome::Nudged
                },
                reason: "no_activity".into(),
                summary: "No activity in the window".into(),
                next_delay_secs: curator_delay(None),
                section_sizes: digest.section_sizes,
                ..Default::default()
            };
            self.plan_store
                .record_curator_round(&plan.id, round.clone())
                .map_err(|e| e.to_string())?;
            self.journal_curator(&plan.id, &round);
            return Ok(None);
        }
        let hash = crate::service::supporter::criteria_hash(digest.body.as_bytes());
        self.plan_store
            .edit_curator_state(&plan.id, now, |p| {
                p.control_loop.running = Some(CuratorRun {
                    synthesis,
                    synthesis_group_close_at: group_close,
                    number,
                    started_at: now,
                    digest_hash: hash,
                    section_sizes: digest.section_sizes,
                    flagged_blueprints: flagged,
                    ..Default::default()
                });
                p.control_loop.manual_run_requested = false;
                p.control_loop.next_at = None;
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        Ok(Some((mirror, digest.body)))
    }

    pub(crate) fn curator_spawned(
        &self,
        plan_id: &str,
        tab_id: &str,
        run_id: &str,
        now: u64,
    ) -> Result<(), String> {
        self.extensions
            .try_get::<super::events::CuratorTabs>()
            .ok_or("Curator runtime is unavailable")?
            .0
            .write()
            .insert(tab_id.into(), plan_id.into());
        let prior_rounds = self
            .plan_store
            .curator_real_rounds_projection(plan_id)
            .map_err(|error| match error {
                crate::service::plan::PlanError::NotFound => "plan absent".to_string(),
                error => error.to_string(),
            })?;
        let number = self
            .plan_store
            .edit_curator_state(plan_id, now, |p| {
                let run = p.control_loop.running.as_mut().ok_or_else(|| {
                    crate::service::plan::PlanError::Validation("round absent".into())
                })?;
                run.tab_id = Some(tab_id.into());
                run.run_id = Some(run_id.into());
                let number = run.number;
                let close = run.synthesis_group_close_at;
                p.control_loop.real_rounds_started = prior_rounds.saturating_add(1);
                if close.is_some() {
                    p.control_loop.last_synthesis_group_close_at = close;
                }
                Ok(number)
            })
            .map_err(|e| e.to_string())?;
        let plan =
            self.plan_store
                .plan_identity_projection(plan_id)
                .map_err(|error| match error {
                    crate::service::plan::PlanError::NotFound => "plan absent".to_string(),
                    error => error.to_string(),
                })?;
        self.mux
            .write()
            .set_tab_description(tab_id, Some(format!("Round {number}")))?;
        self.emit_state_changed(crate::service::MuxEmitCause::TabLifecycle);
        self.bind_curator_session(tab_id)?;
        if let Some(owner) = plan.owner.tab_id {
            let body=format!("Curator round {number} spawned (tab {tab_id}); it may ask you up to three questions in one message; answer with `domios send --tab {tab_id}` or skip; grants: `domios plan curator grant {plan_id} {tab_id} --blueprint <id> | --plan-skill <slug>`.");
            self.deliver_cron_report(&owner, super::curator_started_envelope(number), &body)?;
        }
        Ok(())
    }
}

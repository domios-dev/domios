use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mirror {
    pub provider: String,
    pub model: String,
    pub effort: Option<String>,
}

pub(crate) struct CuratorFireError {
    pub reason: CuratorFailure,
    detail: String,
}
impl From<CuratorFailure> for CuratorFireError {
    fn from(reason: CuratorFailure) -> Self {
        Self {
            reason,
            detail: reason.as_str().into(),
        }
    }
}
impl From<String> for CuratorFireError {
    fn from(detail: String) -> Self {
        Self {
            reason: CuratorFailure::LaunchFailed,
            detail,
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

impl ServiceContext {
    fn wake_curator_schedule(&self) {
        if let Some(wake) = self
            .extensions
            .try_get::<crate::cron::scheduler::CronWake>()
        {
            wake.notify_all();
        }
    }
    pub(super) fn available_mirror(&self, mirror: Mirror) -> Result<Mirror, CuratorFailure> {
        if self
            .agent_registry
            .with_provider(&mirror.provider, |_| ())
            .is_none()
            && !self
                .chatbackend_registry
                .get(&mirror.provider)
                .is_some_and(|backend| backend.spawnable())
        {
            Err(CuratorFailure::ProviderUnavailable)
        } else {
            Ok(mirror)
        }
    }
    pub(crate) fn curator_fire_failed(
        &self,
        plan_id: &str,
        reason: CuratorFailure,
        now: u64,
    ) -> Result<(), String> {
        let round = self
            .plan_store
            .with_plan(plan_id, now, |plan| {
                let state = &mut plan.control_loop;
                state.consecutive_failures += 1;
                let fallback = state.consecutive_failures >= 2
                    || reason == CuratorFailure::ProviderUnavailable;
                if fallback {
                    state.fallback_reason = Some(reason);
                }
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
                    outcome: if fallback {
                        codeterm_types::plan::CuratorOutcome::Fallback
                    } else {
                        codeterm_types::plan::CuratorOutcome::Failed
                    },
                    summary: reason.as_str().into(),
                    reason: reason.as_str().into(),
                    next_delay_secs: curator_delay(None),
                    ..Default::default()
                };
                curator::append_round(plan, round.clone());
                Ok(round)
            })
            .map_err(|e| e.to_string())?;
        self.journal_curator(plan_id, &round);
        Ok(())
    }

    pub(super) fn reconcile_curator_plan(
        &self,
        cron: &crate::CronState,
        plan: &Plan,
        now: u64,
    ) -> Result<(), String> {
        let suspended = self.curator_suspended_reason(plan);
        let eligible = curator::eligible(
            plan,
            self.configuration().orchestration.plan_reminders,
            suspended.is_some(),
        );
        let mut loop_state = plan.control_loop.clone();
        if eligible && loop_state.running.is_some() {
            loop_state.agent_states = super::movement_sources::observed_agents(self, plan, now);
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
                if loop_state.consecutive_failures >= 2 {
                    loop_state.fallback_reason = Some(CuratorFailure::ConsecutiveFailures);
                }
                loop_state.last_real_round_at = Some(now);
                loop_state.next_at = Some(now + curator_delay(None) * 1000);
                let applied = self
                    .plan_store
                    .with_plan(&plan.id, now, |p| {
                        if p.control_loop != plan.control_loop {
                            return Ok(false);
                        }
                        p.control_loop = loop_state.clone();
                        curator::append_round(p, round.clone());
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
        if !eligible {
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
            let tab = plan
                .owner
                .tab_id
                .as_ref()
                .and_then(|id| self.mux.read().get_tab(id).cloned());
            let mirror = tab
                .as_ref()
                .ok_or(CuratorFailure::OrchestratorAbsent)
                .and_then(resolve_mirror)
                .and_then(|mirror| self.available_mirror(mirror));
            match mirror {
                Err(reason) => {
                    loop_state.fallback_reason = Some(reason);
                    loop_state.next_at = None;
                }
                Ok(_) => {
                    let close = curator::synthesis::group_close(plan);
                    if close.is_some_and(|at| {
                        loop_state
                            .last_synthesis_group_close_at
                            .is_none_or(|prior| at > prior)
                    }) {
                        let wake = now.saturating_add(1000);
                        loop_state.next_at =
                            Some(loop_state.next_at.map_or(wake, |next| next.min(wake)));
                    }
                    let next = *loop_state
                        .next_at
                        .get_or_insert(now + curator_delay(None) * 1000);
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
            }
        }
        if loop_state.next_at.is_none() && loop_state.running.is_none() {
            let job = loop_state
                .cron_id
                .as_ref()
                .and_then(|id| cron.lock().get(id).cloned());
            if let Some(mut job) = job.filter(|job| job.enabled) {
                job.enabled = false;
                job.next_run = None;
                commit(cron, |_| {
                    Ok(Prepared::new(CronChange::Patch(job.clone()), ()))
                })
                .map_err(|e| e.to_string())?;
                self.wake_curator_schedule();
            }
        }
        if loop_state != plan.control_loop {
            let fallback = loop_state
                .fallback_reason
                .as_ref()
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
                    summary: reason.as_str().into(),
                    next_delay_secs: curator_delay(None),
                    ..Default::default()
                });
            let applied = self
                .plan_store
                .with_plan(&plan.id, now, |p| {
                    if p.control_loop != plan.control_loop {
                        return Ok(false);
                    }
                    let next = loop_state.next_at;
                    p.control_loop = loop_state;
                    if let Some(round) = &fallback {
                        curator::append_round(p, round.clone());
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
            self.ui.emit("plan-changed", json!({"plan_id":plan.id}));
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
        let plan = self.plan_store.get(&job.task).ok_or("plan absent")?;
        if !curator::eligible(
            &plan,
            self.configuration().orchestration.plan_reminders,
            self.curator_suspended_reason(&plan).is_some(),
        ) || plan.control_loop.running.is_some()
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
                .with_plan(&plan.id, now, |plan| {
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
        let since = plan
            .control_loop
            .last_round
            .as_ref()
            .map(|r| r.ended_at)
            .unwrap_or(now.saturating_sub(CURATOR_INITIAL_WINDOW_MS));
        let mut rows = digest::plan_rows(&plan, since);
        super::movement_sources::append(self, &plan, since, now, &mut rows)?;
        let candidates = self.curator_save_candidates(&plan.id, now)?;
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
        super::digest_sources::memory(self, &plan, since, &mut rows);
        let mut observation = crate::service::hierarchy::heartbeat_gate::Observation::default();
        observation.workers.insert(
            owner.into(),
            crate::service::hierarchy::heartbeat_gate::Worker {
                session_key: plan.control_loop.owner_session_key.clone(),
                turn_cursor: plan.control_loop.owner_turn_cursor.clone(),
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
        if let Some(cursor) = cursor {
            self.plan_store
                .with_plan(&plan.id, now, |p| {
                    p.control_loop.owner_turn_cursor = cursor.turn_cursor;
                    p.control_loop.owner_session_key = cursor.session_key;
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
        }
        let flagged = super::digest_sources::blueprints(self, &plan, since, now, &mut rows);
        let number = plan
            .control_loop
            .last_round
            .as_ref()
            .map(|r| r.number + 1)
            .unwrap_or(1);
        let counts = codeterm_plan::service_plan::state::task_counts(&plan);
        let lanes = plan
            .tasks
            .iter()
            .filter(|t| {
                t.lane.worktree_path.is_some() && t.lane.cleanup_status.as_deref() != Some("done")
            })
            .count();
        let handoff_age = match self.plan_store.list_handoffs(&plan.id) {
            Ok(handoffs) => handoffs
                .into_iter()
                .filter_map(|handoff| handoff.ts)
                .max()
                .map(|at| now.saturating_sub(at)),
            Err(error) => {
                log::debug!("[curator] handoff digest {}: {error}", plan.id);
                None
            }
        };
        let mut header=format!("plan {} | {:?} | control loop curator | orchestrator {} activity {:?} | open {} in_progress {} review {} blocked {} | lanes {} pressure {} | handoff_age_ms {:?} | round {}",plan.id,plan.mode,owner,self.resolved_tab_status(&tab),counts.pending,counts.in_progress,counts.in_review,counts.blocked,lanes,if lanes>CURATOR_HIGH_LANES {"high"}else if lanes>CURATOR_ELEVATED_LANES {"elevated"}else{"normal"},handoff_age,number);
        header.push_str(&format!("\nask: {}", super::ask::command(owner)));
        let rubric = codeterm_prompts::prompts::CURATOR_RUBRIC;
        let real_rounds = curator::synthesis::real_rounds_started(&plan);
        let group_close = curator::synthesis::group_close(&plan).filter(|at| {
            plan.control_loop
                .last_synthesis_group_close_at
                .is_none_or(|prior| *at > prior)
        });
        let synthesis = curator::synthesis::due(
            real_rounds,
            group_close,
            plan.control_loop.last_synthesis_group_close_at,
        );
        header.push_str(if synthesis {
            "\nsynthesis: yes"
        } else {
            "\nsynthesis: no"
        });
        let header = format!("{header}\n\n{rubric}");
        let header = if synthesis {
            curator::synthesis::context_header(&plan, &header)
        } else {
            header
        };
        let digest = digest::build(&header, &rows);
        let action = curator::due_action(
            digest.activity || group_close.is_some() || plan.control_loop.manual_run_requested,
            !codeterm_plan::service_plan::state::orchestrator_actionable_task_ids(&plan).is_empty(),
            plan.control_loop
                .last_real_round_at
                .or(Some(plan.created_at)),
            now,
        );
        if action != DueAction::Spawn {
            if action == DueAction::Nudge {
                let body = self.curator_heartbeat_body(&plan);
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
            .with_plan(&plan.id, now, |p| {
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
            .get(plan_id)
            .map(|plan| curator::synthesis::real_rounds_started(&plan))
            .ok_or("plan absent")?;
        let number = self
            .plan_store
            .with_plan(plan_id, now, |p| {
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
        let plan = self.plan_store.get(plan_id).ok_or("plan absent")?;
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

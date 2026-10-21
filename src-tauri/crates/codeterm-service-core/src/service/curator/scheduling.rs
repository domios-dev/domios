use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mirror {
    pub provider: String,
    pub model: String,
    pub effort: Option<String>,
}

pub fn resolve_mirror(tab: &crate::multiplexer::types::Tab) -> Result<Mirror, String> {
    if crate::service::hierarchy::heartbeat_gate::binding_digest(tab, None).is_none() {
        return Err("orchestrator_unbound".into());
    }
    Ok(Mirror {
        provider: tab
            .agent_type
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or("mirror_unavailable")?,
        model: tab
            .agent_session_model
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or("mirror_unavailable")?,
        effort: tab
            .reasoning_effort_identity
            .actual
            .clone()
            .or_else(|| tab.reasoning_effort_identity.resolved.clone()),
    })
}

impl ServiceContext {
    pub(crate) fn curator_fire_failed(
        &self,
        plan_id: &str,
        reason: &str,
        now: u64,
    ) -> Result<(), String> {
        let round = self
            .plan_store
            .with_plan(plan_id, now, |plan| {
                let state = &mut plan.control_loop;
                state.consecutive_failures += 1;
                let fallback = state.consecutive_failures >= 2 || reason == "provider_unavailable";
                if fallback {
                    state.fallback_reason = Some(reason.into());
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
                    outcome: if fallback { "fallback" } else { "failed" }.into(),
                    summary: reason.into(),
                    reason: reason.into(),
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

    pub(crate) fn reconcile_curators(&self, now: u64) {
        let Some(cron) = self.extensions.try_get::<crate::CronState>() else {
            return;
        };
        for plan in self.plan_store.list_full() {
            if let Err(error) = self.reconcile_curator_plan(cron, &plan, now) {
                log::debug!("[curator] reconcile {}: {error}", plan.id);
            }
        }
    }

    fn reconcile_curator_plan(
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
        loop_state.agent_states = super::movement_sources::observed_agents(self, plan, now);
        if loop_state.cron_id.is_none() {
            loop_state.cron_id = cron
                .lock()
                .list()
                .iter()
                .find(|job| {
                    job.protected
                        && job.role.as_deref() == Some("curator")
                        && job.team_id == plan.team_id
                        && job.name == format!("curator:{}", plan.id)
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
            if ended.is_some() || now.saturating_sub(run.started_at) >= 900_000 {
                let deadline = now.saturating_sub(run.started_at) >= 900_000;
                let reason = if deadline {
                    "deadline"
                } else {
                    "run_ended_without_finish"
                };
                let round = CuratorRound {
                    number: run.number,
                    started_at: run.started_at,
                    ended_at: now,
                    outcome: if deadline { "partial" } else { "failed" }.into(),
                    reason: reason.into(),
                    summary: reason.into(),
                    next_delay_secs: curator_delay(None),
                    digest_hash: run.digest_hash.clone(),
                    section_sizes: run.section_sizes.clone(),
                    turns: run.turns,
                    output_tokens: run.output_tokens,
                    save_id: run.save_id.clone(),
                    duration_ms: now.saturating_sub(run.started_at),
                    ..Default::default()
                };
                loop_state.running = None;
                loop_state.consecutive_failures += 1;
                if loop_state.consecutive_failures >= 2 {
                    loop_state.fallback_reason = Some("consecutive_failures".into());
                }
                loop_state.last_real_round_at = Some(now);
                loop_state.next_at = Some(now + curator_delay(None) * 1000);
                self.plan_store
                    .with_plan(&plan.id, now, |p| {
                        p.control_loop = loop_state.clone();
                        curator::append_round(p, round.clone());
                        Ok(())
                    })
                    .map_err(|e| e.to_string())?;
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
                .ok_or_else(|| "orchestrator_absent".to_string())
                .and_then(resolve_mirror)
                .and_then(|mirror| {
                    if self
                        .agent_registry
                        .with_provider(&mirror.provider, |_| ())
                        .is_none()
                        && !self
                            .chatbackend_registry
                            .get(&mirror.provider)
                            .is_some_and(|b| b.spawnable())
                    {
                        Err("provider_unavailable".into())
                    } else {
                        Ok(mirror)
                    }
                });
            match mirror {
                Err(reason) => {
                    loop_state.fallback_reason = Some(reason);
                    loop_state.next_at = None;
                }
                Ok(_) => {
                    let next = *loop_state
                        .next_at
                        .get_or_insert(now + curator_delay(None) * 1000);
                    let existing = loop_state
                        .cron_id
                        .as_ref()
                        .and_then(|id| cron.lock().get(id).cloned());
                    if existing
                        .as_ref()
                        .is_some_and(|job| cron.lock().is_job_running(&job.id))
                    {
                        return Ok(());
                    }
                    if existing
                        .as_ref()
                        .is_none_or(|job| job.next_run != Some(next) || !job.enabled)
                    {
                        let workspace = plan
                            .team_id
                            .as_deref()
                            .and_then(|team| self.mux.read().team_workspace(team))
                            .ok_or("Team workspace absent")?;
                        let params: CronCreateParams = serde_json::from_value(json!({"name":format!("curator:{}",plan.id),"workspace_id":workspace,"team_id":plan.team_id,"schedule":next.to_string(),"task":plan.id,"agent":"mirror:orchestrator","role":"curator","oneshot":true,"enabled":true,"timeout_seconds":900,"jitter_max_ms":0,"creator_tab_id":plan.owner.tab_id,"cwd":plan.project_path})).map_err(|e|e.to_string())?;
                        let job = commit(cron, |store| {
                            if let Some(mut job) = existing {
                                job.enabled = true;
                                job.next_run = Some(next);
                                job.schedule = next.to_string();
                                Ok(Prepared::new(CronChange::Patch(job.clone()), job))
                            } else {
                                let mut prepared = store.prepare_create(params)?;
                                prepared.value.protected = true;
                                prepared.changes = vec![CronChange::Create(prepared.value.clone())];
                                Ok(prepared)
                            }
                        })
                        .map_err(|e| e.to_string())?
                        .value;
                        loop_state.cron_id = Some(job.id);
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
                commit(cron, |_| Ok(Prepared::new(CronChange::Patch(job), ())))
                    .map_err(|e| e.to_string())?;
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
                    outcome: "fallback".into(),
                    reason: reason.clone(),
                    summary: reason.clone(),
                    next_delay_secs: curator_delay(None),
                    ..Default::default()
                });
            self.plan_store
                .with_plan(&plan.id, now, |p| {
                    let next = loop_state.next_at;
                    p.control_loop = loop_state;
                    if let Some(round) = &fallback {
                        curator::append_round(p, round.clone());
                        p.control_loop.next_at = next;
                    }
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
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
    ) -> Result<Option<(Mirror, String)>, String> {
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
        let owner = plan.owner.tab_id.as_deref().ok_or("orchestrator_absent")?;
        let tab = self
            .mux
            .read()
            .get_tab(owner)
            .cloned()
            .ok_or("orchestrator_absent")?;
        let mirror = resolve_mirror(&tab)?;
        if self
            .agent_registry
            .with_provider(&mirror.provider, |_| ())
            .is_none()
            && !self
                .chatbackend_registry
                .get(&mirror.provider)
                .is_some_and(|b| b.spawnable())
        {
            return Err("provider_unavailable".into());
        }
        let since = plan
            .control_loop
            .last_round
            .as_ref()
            .map(|r| r.ended_at)
            .unwrap_or(now.saturating_sub(7_200_000));
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
        let handoff_age = self
            .plan_store
            .list_handoffs(&plan.id)
            .ok()
            .and_then(|h| h.into_iter().filter_map(|h| h.ts).max())
            .map(|at| now.saturating_sub(at));
        let mut header=format!("plan {} | {:?} | control loop curator | orchestrator {} activity {:?} | open {} in_progress {} review {} blocked {} | lanes {} pressure {} | handoff_age_ms {:?} | round {}",plan.id,plan.mode,owner,self.resolved_tab_status(&tab),counts.pending,counts.in_progress,counts.in_review,counts.blocked,lanes,if lanes>10 {"high"}else if lanes>5 {"elevated"}else{"normal"},handoff_age,number);
        if let Some(resources) = super::resources::line() {
            header.push('\n');
            header.push_str(&resources);
        }
        let rubric = codeterm_prompts::prompts::CURATOR_RUBRIC;
        let digest = digest::build(&format!("{header}\n\n{rubric}"), &rows);
        let action = curator::due_action(
            digest.activity,
            !codeterm_plan::service_plan::state::orchestrator_actionable_task_ids(&plan).is_empty(),
            plan.control_loop
                .last_real_round_at
                .or(Some(plan.created_at)),
            now,
        );
        if action != DueAction::Spawn {
            if action == DueAction::Nudge {
                let body = self.curator_heartbeat_body(&plan);
                self.deliver_cron_report(
                    owner,
                    super::super::messenger::MessageEnvelope::default(),
                    &body,
                )?;
            }
            let round = CuratorRound {
                number,
                started_at: now,
                ended_at: now,
                outcome: if action == DueAction::Quiet {
                    "quiet"
                } else {
                    "nudged"
                }
                .into(),
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
                    number,
                    started_at: now,
                    digest_hash: hash,
                    section_sizes: digest.section_sizes,
                    flagged_blueprints: flagged,
                    ..Default::default()
                });
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
        now: u64,
    ) -> Result<(), String> {
        let number =
            self.plan_store
                .with_plan(plan_id, now, |p| {
                    let run = p.control_loop.running.as_mut().ok_or_else(|| {
                        crate::service::plan::PlanError::Io("round absent".into())
                    })?;
                    run.tab_id = Some(tab_id.into());
                    Ok(run.number)
                })
                .map_err(|e| e.to_string())?;
        let plan = self.plan_store.get(plan_id).ok_or("plan absent")?;
        if let Some(owner) = plan.owner.tab_id {
            let body=format!("Curator round {number} spawned (tab {tab_id}); it may ask you up to three questions in one message; answer with codeterm send --tab {tab_id} or skip; grants: codeterm plan curator grant {plan_id} {tab_id} --blueprint <id> | --plan-skill <slug>.");
            self.deliver_cron_report(
                &owner,
                crate::service::messenger::MessageEnvelope::default(),
                &body,
            )?;
        }
        Ok(())
    }
}

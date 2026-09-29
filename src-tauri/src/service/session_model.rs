//! Runtime session-model evidence: provider detection, refresh scheduling, and
//! the current-session guard every runtime write passes through.

use super::ServiceContext;

impl ServiceContext {
    pub(crate) fn start_focused_tab_model_poller(&self) {
        let svc = self.clone();
        std::thread::spawn(move || {
            let step = if cfg!(debug_assertions) {
                std::time::Duration::from_secs(3)
            } else {
                std::time::Duration::from_secs(15)
            };
            loop {
                std::thread::sleep(step);
                let target = {
                    let mux = svc.mux.read();
                    let focused = mux.snapshot().focused_tab_id;
                    focused.and_then(|id| {
                        mux.get_tab(&id)
                            .and_then(|p| p.agent_type.clone())
                            .map(|t| (id, t))
                    })
                };
                if let Some((pid, at)) = target {
                    svc.refresh_session_model_once(&pid, &at);
                }
            }
        });
    }

    /// Bam-refresh model on the newly focused tab so the badge updates the
    /// instant the user switches tabs, instead of waiting up to one tick.
    pub fn refresh_focused_tab_model(&self) {
        let target: Option<(String, String)> = {
            let mux = self.mux.read();
            mux.snapshot().focused_tab_id.and_then(|id| {
                mux.get_tab(&id)
                    .and_then(|p| p.agent_type.clone())
                    .map(|t| (id, t))
            })
        };
        if let Some((pid, at)) = target {
            self.refresh_session_model_once(&pid, &at);
        }
    }

    pub(crate) fn schedule_focused_tab_model_refresh(&self) {
        let svc = self.clone();
        std::thread::spawn(move || {
            svc.refresh_focused_tab_model();
        });
    }

    pub(crate) fn refresh_session_model_for_current_tab(&self, tab_id: &str) {
        let binding = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .and_then(|tab| Some((tab.agent_type.clone()?, tab.cwd.clone()?)))
        };
        let Some((agent_id, cwd)) = binding else {
            return;
        };
        self.refresh_session_model(tab_id, &agent_id, &cwd, false);
    }

    pub(crate) fn schedule_session_model_refresh_for_current_tab(&self, tab_id: &str) {
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        if let Err(error) = std::thread::Builder::new()
            .name("session-model-event-refresh".into())
            .spawn(move || svc.refresh_session_model_for_current_tab(&tab_id))
        {
            log::warn!("[session-model] failed to start event refresh: {error}");
        }
    }

    pub(crate) fn schedule_session_model_refresh_after_stop(&self, tab_id: &str) -> bool {
        if self.observe_post_launch_turn_after_stop(tab_id) {
            return true;
        }
        let (model_known, binding) = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return false;
            };
            if tab.agent_type.is_none() || tab.cwd.is_none() || tab.agent_session_id.is_none() {
                return false;
            }
            let binding = super::task_assignment::authenticated_tab_binding(tab);
            let model_known = tab.agent_session_model.is_some()
                && binding.as_ref().is_some_and(|binding| {
                    tab.agent_session_model_identity
                        .as_ref()
                        .is_some_and(|identity| {
                            identity.session_id == binding.session_id
                                && identity.session_binding_nonce == binding.session_binding_nonce
                        })
                });
            (model_known, binding)
        };
        if !model_known {
            self.schedule_session_model_refresh_for_current_tab(tab_id);
            return true;
        }
        let attempt_model_unknown = binding.is_some_and(|binding| {
            self.plan_store
                .open_attempt_task_for_tab(
                    tab_id,
                    &binding.session_id,
                    &binding.session_binding_nonce,
                )
                .is_some()
        });
        if attempt_model_unknown {
            self.persist_current_attempt_session_model(tab_id);
        }
        false
    }

    /// The model this tab's own claimed session is running. Runtime evidence first,
    /// else the provider re-reads its own record for that exact session id — the same
    /// authority the poller writes from, never a provider or launch-time guess.
    /// Read-only, so an observation path can call it without touching the badge.
    pub(crate) fn verified_session_model(&self, tab_id: &str) -> Option<String> {
        let (recorded, agent_id, cwd, session_id) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id)?;
            let session_id = tab.agent_session_id.as_ref()?;
            let nonce = tab.session_binding_nonce.as_ref();
            let recorded = nonce
                .zip(tab.reported_session_binding_nonce.as_ref())
                .filter(|(nonce, reported)| nonce == reported)
                .and_then(|(nonce, _)| {
                    tab.agent_session_model_identity
                        .as_ref()
                        .filter(|identity| {
                            identity.session_id == *session_id
                                && identity.session_binding_nonce == *nonce
                        })
                })
                .and_then(|_| tab.agent_session_model.clone());
            (
                recorded,
                tab.agent_type.clone()?,
                tab.cwd.clone()?,
                session_id.clone(),
            )
        };
        if recorded.is_some() {
            return recorded;
        }
        let detected = self
            .agent_registry
            .with_provider(&agent_id, |p| p.detect_session_model(&cwd, &session_id))
            .flatten()
            .map(|model| crate::service::spawn::normalize_session_model(&model));
        if detected.is_none() {
            // A tab that records no model must not read like a tab that ran none.
            log::debug!(
                "[session-model] tab={tab_id} provider={agent_id} session={session_id}: no model reported"
            );
        }
        detected
    }

    pub(crate) fn refresh_session_model_once(&self, tab_id: &str, agent_id: &str) {
        let cwd = {
            let mux = self.mux.read();
            let Some(p) = mux.get_tab(tab_id) else {
                return;
            };
            p.cwd.clone()
        };
        let Some(cwd) = cwd else { return };
        self.refresh_session_model(tab_id, agent_id, &cwd, true);
    }

    fn refresh_session_model(
        &self,
        tab_id: &str,
        agent_id: &str,
        cwd: &str,
        detect_active_session: bool,
    ) {
        let session_id = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .and_then(|tab| tab.agent_session_id.clone())
        };
        // Provider rolled its live session (e.g. opencode /model swap): follow it to the new sid before scanning the model. Provider-owned signal (screen/event), guarded so it never steals a sibling tab's session.
        if detect_active_session {
            if let Some(ref current) = session_id {
                if let Some(new_sid) = self
                    .agent_registry
                    .with_provider(agent_id, |p| p.detect_active_session(cwd, current))
                    .flatten()
                {
                    let claimed = {
                        let mut mux = self.mux.write();
                        if new_sid == *current || mux.other_session_ids(tab_id).contains(&new_sid) {
                            false
                        } else if let Some(p) = mux.get_tab_mut(tab_id) {
                            if p.agent_type.as_deref() == Some(agent_id) {
                                p.archive_context_epoch(agent_id, current);
                                p.agent_session_id = Some(new_sid.clone());
                                clear_runtime_model_evidence(p);
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    };
                    if claimed {
                        log::info!(
                            "[session-detect] rolled Tab {} from {} → {} ({})",
                            tab_id,
                            current,
                            new_sid,
                            agent_id,
                        );
                        self.emit_state_changed();
                        if let Err(error) = super::session_persist::persist_session_after_mutation(
                            self,
                            super::session_persist::SessionPersistUrgency::Prompt,
                        ) {
                            log::warn!("[context-epoch] failed to persist reset boundary: {error}");
                        }
                    }
                }
            }
        }
        // Model badge needs a bound session. Binding is provider-reported only — we never guess one from the filesystem here; an unbound tab just has no model badge yet.
        let sid = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).and_then(|p| p.agent_session_id.clone())
        };
        let Some(sid) = sid else { return };
        self.refresh_session_reasoning_effort_once(tab_id, agent_id, cwd, &sid);
        let launch_report = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .and_then(|tab| current_launch_report(tab, &sid))
        };
        let pending_switch = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .and_then(|tab| tab.pending_model_switch_observation.clone())
                .filter(|pending| pending.session_id == sid)
        };
        let pending_binding_nonce = pending_switch.as_ref().and_then(|_| {
            let mux = self.mux.read();
            mux.get_tab(tab_id).and_then(|tab| {
                tab.session_binding_nonce
                    .as_ref()
                    .filter(|nonce| tab.reported_session_binding_nonce.as_ref() == Some(nonce))
                    .cloned()
            })
        });
        if pending_switch.is_some()
            && (launch_report.is_none() || pending_binding_nonce.is_none())
        {
            return;
        }
        let transcript_model = self
            .agent_registry
            .with_provider(agent_id, |p| p.detect_session_model(cwd, &sid))
            .flatten()
            .map(|model| crate::service::spawn::normalize_session_model(&model));
        // A failed detection (None) must not clobber a known model — restore seeds it from the snapshot and the session file may not name the model yet.
        let pending_report_without_baseline = pending_switch.is_some()
            && launch_report
                .as_ref()
                .is_some_and(|report| report.transcript_model_at_launch.is_none());
        let evidence = if pending_report_without_baseline {
            launch_report
                .as_ref()
                .map(|report| (report.model.clone(), Some(report.clone())))
        } else {
            session_model_evidence(launch_report.clone(), transcript_model.clone())
        };
        let Some((model, launch_report)) = evidence
        else {
            return;
        };
        let first_turn_observed = pending_switch.is_some()
            && transcript_model.is_some()
            && launch_report.is_none();
        self.apply_session_model(
            tab_id,
            agent_id,
            &sid,
            model,
            launch_report,
            pending_binding_nonce.as_deref(),
        );
        if let (true, Some(pending), Some(nonce)) = (
            first_turn_observed,
            pending_switch.as_ref(),
            pending_binding_nonce.as_deref(),
        ) {
            if self.clear_pending_model_switch(tab_id, pending, nonce) {
                super::boot_wait::notify_boot_changed();
            }
        }
    }

    /// The provider named the model this binding runs at session start. That
    /// report outranks a transcript whose tail still predates the launch.
    pub(crate) fn record_launch_reported_model(
        &self,
        tab_id: &str,
        agent_id: &str,
        session_id: &str,
        reported_model: &str,
    ) {
        let model = crate::service::spawn::normalize_session_model(reported_model);
        if model.is_empty() {
            return;
        }
        let cwd = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .filter(|tab| runtime_model_update_matches_session(tab, agent_id, session_id))
                .and_then(|tab| tab.cwd.clone())
        };
        let Some(cwd) = cwd else { return };
        let transcript_model_at_launch = self
            .agent_registry
            .with_provider(agent_id, |p| p.detect_session_model(&cwd, session_id))
            .flatten()
            .map(|model| crate::service::spawn::normalize_session_model(&model));
        let report = crate::multiplexer::types::LaunchModelReport {
            model: model.clone(),
            transcript_model_at_launch,
        };
        self.apply_session_model(tab_id, agent_id, session_id, model, Some(report), None);
        super::boot_wait::notify_boot_changed();
    }

    fn record_post_launch_turn_model(
        &self,
        tab_id: &str,
        observation: crate::multiplexer::types::PendingModelSwitchObservation,
        binding_nonce: &str,
    ) {
        let (agent_id, cwd) = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return;
            };
            let verified_binding = tab.session_binding_nonce.as_deref() == Some(binding_nonce)
                && tab.reported_session_binding_nonce.as_deref() == Some(binding_nonce);
            let pending_observation_matches =
                tab.pending_model_switch_observation.as_ref() == Some(&observation);
            if !verified_binding
                || tab.agent_session_id.as_deref() != Some(&observation.session_id)
                || !pending_observation_matches
            {
                return;
            }
            (tab.agent_type.clone(), tab.cwd.clone())
        };
        let Some(agent_id) = agent_id else { return };
        let Some(cwd) = cwd else { return };
        let Some(model) = self
            .agent_registry
            .with_provider(&agent_id, |provider| {
                provider.detect_session_model(&cwd, &observation.session_id)
            })
            .flatten()
            .map(|model| crate::service::spawn::normalize_session_model(&model))
            .filter(|model| !model.is_empty())
        else {
            return;
        };
        self.apply_session_model(
            tab_id,
            &agent_id,
            &observation.session_id,
            model,
            None,
            Some(binding_nonce),
        );
        if self.clear_pending_model_switch(tab_id, &observation, binding_nonce) {
            super::boot_wait::notify_boot_changed();
        }
    }

    fn clear_pending_model_switch(
        &self,
        tab_id: &str,
        observation: &crate::multiplexer::types::PendingModelSwitchObservation,
        binding_nonce: &str,
    ) -> bool {
        let mut mux = self.mux.write();
        let Some(tab) = mux.get_tab_mut(tab_id) else {
            return false;
        };
        if tab.agent_session_id.as_deref() != Some(&observation.session_id)
            || tab.session_binding_nonce.as_deref() != Some(binding_nonce)
            || tab.reported_session_binding_nonce.as_deref() != Some(binding_nonce)
            || tab.pending_model_switch_observation.as_ref() != Some(observation)
        {
            return false;
        }
        tab.pending_model_switch_observation = None;
        true
    }

    pub(crate) fn observe_post_launch_turn_after_stop(&self, tab_id: &str) -> bool {
        let (observation, binding_nonce) = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return false;
            };
            let Some(binding_nonce) = tab
                .session_binding_nonce
                .as_ref()
                .filter(|nonce| tab.reported_session_binding_nonce.as_ref() == Some(nonce))
            else {
                return false;
            };
            let Some(observation) = tab
                .pending_model_switch_observation
                .as_ref()
                .filter(|pending| {
                    tab.agent_session_id.as_deref() == Some(pending.session_id.as_str())
                })
            else {
                return false;
            };
            (observation.clone(), binding_nonce.clone())
        };
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        if let Err(error) = std::thread::Builder::new()
            .name("post-launch-session-model-refresh".into())
            .spawn(move || svc.record_post_launch_turn_model(&tab_id, observation, &binding_nonce))
        {
            log::warn!("[session-model] failed to start post-launch turn refresh: {error}");
            return false;
        }
        true
    }

    fn apply_session_model(
        &self,
        tab_id: &str,
        agent_id: &str,
        sid: &str,
        model: String,
        launch_report: Option<crate::multiplexer::types::LaunchModelReport>,
        binding_nonce: Option<&str>,
    ) {
        let (
            current_model,
            planned_model,
            requested_model,
            provider_resolved_model,
            current_fallback,
            current_mismatch,
        ) = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return;
            };
            if !runtime_model_update_matches_binding(tab, agent_id, sid, binding_nonce) {
                return;
            }
            (
                tab.agent_session_model.clone(),
                tab.agent_planned_model.clone(),
                tab.agent_requested_model.clone(),
                tab.agent_provider_resolved_model.clone(),
                tab.model_fallback.clone(),
                tab.model_identity_mismatch.clone(),
            )
        };
        let identity_update = model_identity_update(
            current_model.as_deref(),
            planned_model.as_deref(),
            requested_model.as_deref(),
            provider_resolved_model.as_deref(),
            current_fallback.clone(),
            &model,
            requested_model.as_deref().is_none_or(|requested| {
                session_model_matches(&self.agent_registry, agent_id, requested, &model)
            }),
        );
        let mismatch_changed = identity_update.mismatch != current_mismatch;
        let fallback_changed = identity_update.fallback != current_fallback;
        if !identity_update.model_changed && !mismatch_changed && !fallback_changed {
            let model_binding_updated = {
                let mut mux = self.mux.write();
                if let Some(tab) = mux.get_tab_mut(tab_id) {
                    if runtime_model_update_matches_binding(tab, agent_id, sid, binding_nonce) {
                        let identity = session_model_identity(tab, sid, launch_report.clone());
                        tab.agent_session_model_identity = identity;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };
            if !model_binding_updated {
                return;
            }
            self.persist_attempt_session_model(tab_id, agent_id, sid, &model);
            return;
        }
        let should_notify = identity_update.mismatch.is_some() && current_mismatch.is_none();
        if should_notify {
            if let Some(mismatch) = identity_update.mismatch.as_ref() {
                let severity = crate::service::spawn::unrequested_substitution_severity(
                    mismatch.planned_model.as_deref(),
                );
                let title = format!(
                    "model fallback: requested {}, actual {}",
                    mismatch.requested_model, mismatch.actual_model
                );
                // A tab nobody watches keeps the fallback in its log, off the user's inbox.
                if self.tab_is_user_facing(tab_id) {
                    self.create_notification_ext(
                        title,
                        Some(
                            "Provider runtime session metadata reported a different model than the launch request."
                                .to_string(),
                        ),
                        Some(severity),
                        None,
                        None,
                        crate::service::notification::NotificationOrigin::local_tab(tab_id.to_string()),
                        crate::service::notification::PushIntent::default(),
                        None,
                    );
                }
                log::warn!(
                    "[session-model] tab={} requested_model={} actual_model={} severity={} fallback_reason=runtime_session_model_mismatch",
                    tab_id,
                    mismatch.requested_model,
                    mismatch.actual_model,
                    severity
                );
            }
        }
        let model = Some(model);
        let updated = {
            let mut mux = self.mux.write();
            if let Some(tab) = mux.get_tab_mut(tab_id) {
                if runtime_model_update_matches_binding(tab, agent_id, sid, binding_nonce) {
                    if identity_update.model_changed {
                        log::info!(
                            "[session-model] tab={} {:?} -> {:?}",
                            tab_id,
                            tab.agent_session_model,
                            model
                        );
                    }
                    tab.agent_session_model = model.clone();
                    tab.agent_provider_resolved_model = model.clone();
                    let model_identity = session_model_identity(tab, sid, launch_report);
                    tab.agent_session_model_identity = model_identity;
                    tab.model_fallback = identity_update.fallback;
                    tab.model_identity_mismatch = identity_update.mismatch;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if updated {
            self.emit_state_changed();
            if let Some(model) = model.as_deref() {
                self.persist_attempt_session_model(tab_id, agent_id, sid, model);
            }
        }
    }

    pub(crate) fn persist_attempt_session_model(
        &self,
        tab_id: &str,
        agent_id: &str,
        session_id: &str,
        model: &str,
    ) {
        if let Some(plan_id) =
            self.write_attempt_model_identity(tab_id, agent_id, session_id, Some(model.to_string()))
        {
            crate::api::events::emit_everywhere(
                &self.app_handle,
                "plan-changed",
                serde_json::json!({ "plan_id": plan_id }),
            );
        }
    }

    fn write_attempt_model_identity(
        &self,
        tab_id: &str,
        agent_id: &str,
        session_id: &str,
        detected_model: Option<String>,
    ) -> Option<String> {
        let identity = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return None;
            };
            if !runtime_model_update_matches_session(tab, agent_id, session_id) {
                return None;
            }
            let Some(binding) = super::task_assignment::authenticated_tab_binding(tab) else {
                return None;
            };
            let actual_model = binding.session_model.clone().filter(|model| {
                detected_model
                    .as_ref()
                    .is_none_or(|detected| detected == model)
            });
            let mismatch = tab.model_identity_mismatch.as_ref().map(|mismatch| {
                crate::plan::AttemptModelIdentityMismatch {
                    requested_model: mismatch.requested_model.clone(),
                    provider_resolved_model: mismatch.provider_resolved_model.clone(),
                    actual_model: mismatch.actual_model.clone(),
                    reason: mismatch.reason.clone(),
                }
            });
            (
                binding,
                tab.agent_requested_model.clone(),
                tab.agent_provider_resolved_model.clone(),
                actual_model,
                mismatch,
            )
        };
        let (binding, requested_model, provider_resolved_model, session_model, mismatch) = identity;
        if binding.provider.as_deref() != Some(agent_id)
            || binding.session_id != session_id
            || binding.session_binding_nonce.trim().is_empty()
        {
            return None;
        }
        let Some((plan_id, task_id, attempt_id)) =
            self.plan_store.open_attempt_task_for_effort_for_tab(
                tab_id,
                &binding.session_id,
                &binding.session_binding_nonce,
            )
        else {
            return None;
        };
        match self.plan_store.update_attempt_model_identity(
            &plan_id,
            &task_id,
            tab_id,
            &attempt_id,
            &binding.session_id,
            &binding.session_binding_nonce,
            requested_model,
            provider_resolved_model,
            session_model,
            mismatch,
            session_model_epoch_ms(),
        ) {
            Ok(true) => Some(plan_id),
            Ok(false) => None,
            Err(error) => {
                log::warn!(
                    "[session-model] failed to persist task attempt model tab={tab_id}: {error}"
                );
                None
            }
        }
    }

    pub(crate) fn persist_current_attempt_session_model(&self, tab_id: &str) {
        let identity = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .and_then(|tab| Some((tab.agent_type.clone()?, tab.agent_session_id.clone()?)))
        };
        if let Some((agent_id, session_id)) = identity {
            if let Some(plan_id) =
                self.write_attempt_model_identity(tab_id, &agent_id, &session_id, None)
            {
                crate::api::events::emit_everywhere(
                    &self.app_handle,
                    "plan-changed",
                    serde_json::json!({ "plan_id": plan_id }),
                );
            }
        }
    }
}

fn session_model_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn spawn_startup_model_refresh(
    task: impl FnOnce() + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("startup-session-model-refresh".into())
        .spawn(task)
}

fn models_match(requested: &str, actual: &str) -> bool {
    requested.trim().eq_ignore_ascii_case(actual.trim())
}

/// One equivalence for every "is the tab running the model it asked for" check.
pub(crate) fn session_model_matches(
    registry: &crate::agent::registry::AgentRegistry,
    agent_id: &str,
    requested: &str,
    actual: &str,
) -> bool {
    models_match(requested, actual)
        || crate::service::spawn::normalize_session_model(requested).eq_ignore_ascii_case(actual)
        || crate::service::spawn::catalogue_models_match(registry, agent_id, requested, actual)
}

fn session_model_evidence(
    launch_report: Option<crate::multiplexer::types::LaunchModelReport>,
    transcript_model: Option<String>,
) -> Option<(String, Option<crate::multiplexer::types::LaunchModelReport>)> {
    match (launch_report, transcript_model) {
        (Some(report), transcript)
            if transcript.as_deref().is_none_or(|transcript| {
                report
                    .transcript_model_at_launch
                    .as_deref()
                    .is_some_and(|at_launch| models_match(at_launch, transcript))
            }) =>
        {
            Some((report.model.clone(), Some(report)))
        }
        (_, Some(transcript)) => Some((transcript, None)),
        _ => None,
    }
}

/// This binding's launch report, only while the tab still holds the exact
/// session and verified nonce the report was recorded under.
pub(crate) fn current_launch_report(
    tab: &crate::multiplexer::types::Tab,
    session_id: &str,
) -> Option<crate::multiplexer::types::LaunchModelReport> {
    let identity = tab.agent_session_model_identity.as_ref()?;
    let current = session_model_identity(tab, session_id, None)?;
    (current.session_id == identity.session_id
        && current.session_binding_nonce == identity.session_binding_nonce)
        .then(|| identity.launch_report.clone())
        .flatten()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelIdentityUpdate {
    model_changed: bool,
    fallback: Option<crate::multiplexer::types::ModelFallbackNotice>,
    mismatch: Option<crate::multiplexer::types::ModelIdentityMismatch>,
}

fn model_identity_update(
    current_model: Option<&str>,
    planned_model: Option<&str>,
    requested_model: Option<&str>,
    provider_resolved_model: Option<&str>,
    current_fallback: Option<crate::multiplexer::types::ModelFallbackNotice>,
    actual_model: &str,
    requested_matches_actual: bool,
) -> ModelIdentityUpdate {
    let actual = actual_model.trim().to_string();
    let planned = planned_model
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let provider_resolved = provider_resolved_model
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mismatch = requested_model
        .map(str::trim)
        .filter(|requested| !requested.is_empty())
        .and_then(|requested| {
            (!requested_matches_actual).then(|| crate::multiplexer::types::ModelIdentityMismatch {
                planned_model: planned.map(str::to_string),
                requested_model: requested.to_string(),
                provider_resolved_model: provider_resolved.map(str::to_string),
                actual_model: actual.clone(),
                reason: "runtime_session_model_mismatch".to_string(),
            })
        });
    let fallback = current_fallback
        .map(|mut fallback| {
            fallback.actual_model = Some(actual.clone());
            fallback
        })
        .or_else(|| {
            mismatch
                .as_ref()
                .map(|mismatch| crate::multiplexer::types::ModelFallbackNotice {
                    planned_model: mismatch.planned_model.clone(),
                    requested_model: mismatch.requested_model.clone(),
                    provider_resolved_model: mismatch
                        .provider_resolved_model
                        .clone()
                        .unwrap_or_else(|| mismatch.actual_model.clone()),
                    actual_model: Some(mismatch.actual_model.clone()),
                    reason: mismatch.reason.clone(),
                })
        });
    ModelIdentityUpdate {
        model_changed: current_model.is_none_or(|current| !models_match(current, &actual)),
        fallback,
        mismatch,
    }
}

pub(crate) fn clear_runtime_model_evidence(tab: &mut crate::multiplexer::types::Tab) {
    tab.agent_session_model = None;
    tab.agent_session_model_identity = None;
    tab.pending_model_switch_observation = None;
    tab.model_identity_mismatch = None;
    if let Some(fallback) = tab.model_fallback.as_mut() {
        fallback.actual_model = None;
    }
    super::reasoning_effort::clear_runtime_evidence(tab);
}

fn session_model_identity(
    tab: &crate::multiplexer::types::Tab,
    session_id: &str,
    launch_report: Option<crate::multiplexer::types::LaunchModelReport>,
) -> Option<crate::multiplexer::types::SessionModelIdentity> {
    let nonce = tab.session_binding_nonce.as_ref()?;
    (tab.agent_session_id.as_deref() == Some(session_id)
        && tab.reported_session_binding_nonce.as_ref() == Some(nonce))
    .then(|| crate::multiplexer::types::SessionModelIdentity {
        session_id: session_id.into(),
        session_binding_nonce: nonce.clone(),
        launch_report,
    })
}

pub(crate) fn has_current_session_model(
    tab: &crate::multiplexer::types::Tab,
    session_id: &str,
) -> bool {
    session_model_identity(tab, session_id, None).is_some() && tab.agent_session_model.is_some()
}

fn runtime_model_update_matches_session(
    tab: &crate::multiplexer::types::Tab,
    agent_id: &str,
    session_id: &str,
) -> bool {
    tab.agent_type.as_deref() == Some(agent_id)
        && tab.agent_session_id.as_deref() == Some(session_id)
}

fn runtime_model_update_matches_binding(
    tab: &crate::multiplexer::types::Tab,
    agent_id: &str,
    session_id: &str,
    binding_nonce: Option<&str>,
) -> bool {
    runtime_model_update_matches_session(tab, agent_id, session_id)
        && binding_nonce.is_none_or(|nonce| {
            tab.session_binding_nonce.as_deref() == Some(nonce)
                && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
        })
}

#[cfg(test)]
mod tests;

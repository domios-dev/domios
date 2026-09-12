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

    /// A session the tab just bound is the current-session event: the commit
    /// cleared runtime evidence, so re-read it or an unfocused tab keeps none.
    pub(crate) fn refresh_session_model_after_binding(&self, tab_id: &str) {
        let agent_id = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).and_then(|tab| tab.agent_type.clone())
        };
        let Some(agent_id) = agent_id else { return };
        self.refresh_session_model_once(tab_id, &agent_id);
    }

    pub(crate) fn schedule_session_model_refresh_after_binding(&self, tab_id: &str) {
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        if let Err(error) = std::thread::Builder::new()
            .name("session-binding-model-refresh".into())
            .spawn(move || svc.refresh_session_model_after_binding(&tab_id))
        {
            log::warn!("[session-model] failed to start post-binding refresh: {error}");
        }
    }

    pub(crate) fn refresh_session_model_once(&self, tab_id: &str, agent_id: &str) {
        let (cwd, session_id) = {
            let mux = self.mux.read();
            let Some(p) = mux.get_tab(tab_id) else {
                return;
            };
            (p.cwd.clone(), p.agent_session_id.clone())
        };
        let Some(cwd) = cwd else { return };
        // Provider rolled its live session (e.g. opencode /model swap): follow it to the new sid before scanning the model. Provider-owned signal (screen/event), guarded so it never steals a sibling tab's session.
        if let Some(ref current) = session_id {
            if let Some(new_sid) = self
                .agent_registry
                .with_provider(agent_id, |p| p.detect_active_session(&cwd, current))
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
        // Model badge needs a bound session. Binding is provider-reported only — we never guess one from the filesystem here; an unbound tab just has no model badge yet.
        let sid = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).and_then(|p| p.agent_session_id.clone())
        };
        let Some(sid) = sid else { return };
        self.refresh_session_reasoning_effort_once(tab_id, agent_id, &cwd, &sid);
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
            (
                tab.agent_session_model.clone(),
                tab.agent_planned_model.clone(),
                tab.agent_requested_model.clone(),
                tab.agent_provider_resolved_model.clone(),
                tab.model_fallback.clone(),
                tab.model_identity_mismatch.clone(),
            )
        };
        // A failed detection (None) must not clobber a known model — restore seeds it from the snapshot and the session file may not name the model yet.
        let Some(model) = self
            .agent_registry
            .with_provider(agent_id, |p| p.detect_session_model(&cwd, &sid))
            .flatten()
            .map(|model| crate::service::spawn::normalize_session_model(&model))
        else {
            return;
        };
        let identity_update = model_identity_update(
            current_model.as_deref(),
            planned_model.as_deref(),
            requested_model.as_deref(),
            provider_resolved_model.as_deref(),
            current_fallback.clone(),
            &model,
        );
        let mismatch_changed = identity_update.mismatch != current_mismatch;
        let fallback_changed = identity_update.fallback != current_fallback;
        if !identity_update.model_changed && !mismatch_changed && !fallback_changed {
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
                self.create_notification(
                    title,
                    Some(
                        "Provider runtime session metadata reported a different model than the launch request."
                            .to_string(),
                    ),
                    Some(severity),
                );
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
        let mut mux = self.mux.write();
        if let Some(p) = mux.get_tab_mut(tab_id) {
            if runtime_model_update_matches_session(p, agent_id, &sid) {
                if identity_update.model_changed {
                    log::info!(
                        "[session-model] tab={} {:?} -> {:?}",
                        tab_id,
                        p.agent_session_model,
                        model
                    );
                }
                p.agent_session_model = model;
                p.model_fallback = identity_update.fallback;
                p.model_identity_mismatch = identity_update.mismatch;
                drop(mux);
                self.emit_state_changed();
            }
        }
    }
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
            (!models_match(requested, &actual)).then(|| {
                crate::multiplexer::types::ModelIdentityMismatch {
                    planned_model: planned.map(str::to_string),
                    requested_model: requested.to_string(),
                    provider_resolved_model: provider_resolved.map(str::to_string),
                    actual_model: actual.clone(),
                    reason: "runtime_session_model_mismatch".to_string(),
                }
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
                        .unwrap_or_else(|| mismatch.requested_model.clone()),
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
    tab.model_identity_mismatch = None;
    if let Some(fallback) = tab.model_fallback.as_mut() {
        fallback.actual_model = None;
    }
    super::reasoning_effort::clear_runtime_evidence(tab);
}

fn runtime_model_update_matches_session(
    tab: &crate::multiplexer::types::Tab,
    agent_id: &str,
    session_id: &str,
) -> bool {
    tab.agent_type.as_deref() == Some(agent_id)
        && tab.agent_session_id.as_deref() == Some(session_id)
}

#[cfg(test)]
mod tests;

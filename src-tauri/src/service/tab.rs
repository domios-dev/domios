//! Tab lifecycle: close, rename, focus.

use super::ServiceContext;
use crate::multiplexer::types::{AgentRole, TabCloseReason, TitleLevel};
use crate::plan::peer::{resolve_planner_ref, PeerResolution};
use crate::service::session_persist::{self, SessionPersistUrgency};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_TAB_DESCRIPTION_CHARS: usize = 200;

/// Prefix on every self-termination refusal so transports can classify it.
pub(crate) const SELF_CLOSE_REFUSAL_PREFIX: &str = "self-termination refused";

/// A coordinator must not self-terminate. Planner identity comes from the plan
/// graph, not from the structural role.
pub(crate) fn is_coordinator(
    role: Option<AgentRole>,
    role_profile: Option<&str>,
    is_plan_owner: bool,
    is_bound_planner: bool,
) -> bool {
    codeterm_types::agent::role_kind::anchors_team(role, role_profile)
        || is_plan_owner
        || is_bound_planner
}

pub fn normalize_tab_description(value: &str) -> Result<Option<String>, String> {
    let trimmed = value.trim();
    if trimmed.chars().count() > MAX_TAB_DESCRIPTION_CHARS {
        return Err(format!(
            "description must be {} characters or fewer",
            MAX_TAB_DESCRIPTION_CHARS
        ));
    }
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

pub(crate) fn clear_agent_identity_for_lifecycle_exit(
    tab: &mut crate::multiplexer::types::Tab,
) -> bool {
    let has_identity = tab.agent_type.is_some()
        || tab.agent_type_locked
        || tab.agent_session_id.is_some()
        || tab.agent_session_name.is_some()
        || tab.session_binding_nonce.is_some()
        || tab.reported_session_binding_nonce.is_some()
        || tab.agent_state.is_some()
        || tab.pending_prompt.is_some();
    if !has_identity {
        return false;
    }
    tab.agent_type = None;
    tab.agent_type_locked = false;
    tab.agent_set_at = None;
    tab.agent_booting = false;
    // Tag the retiring nonce with its still-current session BEFORE clearing it,
    // so a resumed session can prove same-session lineage after relaunch.
    tab.remember_superseded_binding_nonce();
    tab.agent_session_id = None;
    tab.agent_session_name = None;
    tab.session_binding_nonce = None;
    tab.reported_session_binding_nonce = None;
    tab.agent_state = None;
    tab.pending_prompt = None;
    tab.exit_suspected_at = None;
    crate::service::boot_stage::apply_boot_stage(
        tab,
        codeterm_types::pty::BootStage::ShellReady,
        None,
    );
    true
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ServiceContext {
    pub(crate) fn prepare_tab_close(&self, tab_id: &str) -> Result<(), String> {
        self.chat_registry.shutdown_tab(tab_id);

        if let Some((plugin_id, session_id)) = self.tab_chat_backing(tab_id) {
            let _ = self.chatbackend_close(&plugin_id, &session_id);
        }

        self.record_lifecycle_end(
            tab_id,
            crate::session::lifecycle::LifecycleTransition::Closed,
        );
        self.discard_pending_delivery(tab_id)
            .map_err(|error| format!("pending delivery discard failed: {error}"))?;
        self.invalidate_guarded_agent_input(tab_id);
        self.release_worker_persona_for_tab(tab_id);
        Ok(())
    }

    pub(crate) fn finish_tab_close(&self, tab_id: &str) -> Result<(), String> {
        let pad_result = self.revoke_agent_pad(tab_id);
        self.clear_dead_planner_binding(tab_id);
        self.flag_bound_tasks_for_gone_tab(tab_id, epoch_ms());
        self.prompt_detector.lock().remove_tab(tab_id);
        crate::service::structured_prompt::clear_tab(tab_id);
        self.remote_workers.forget_orchestrator(tab_id);
        pad_result
    }

    /// True when `tab_id` is a coordinator (Orchestrator role, plan owner, or
    /// bound Planner) and so must not close itself. Keys off caller identity in
    /// the mux + plan graph, never a display name.
    pub(crate) fn is_self_close_protected(&self, tab_id: &str) -> bool {
        let (role, role_profile) = self
            .mux
            .read()
            .get_tab(tab_id)
            .map(|tab| (tab.role, tab.role_profile.clone()))
            .unwrap_or_default();
        if is_coordinator(role, role_profile.as_deref(), false, false) {
            return true;
        }
        for plan in self.plan_store.list_full() {
            let is_owner = plan.owner.tab_id.as_deref() == Some(tab_id);
            let is_planner =
                match resolve_planner_ref(&plan.planner, &plan.owner, |p| self.tab_probe(p)) {
                    PeerResolution::Planner { tab_id: p } => p == tab_id,
                    _ => false,
                };
            if is_coordinator(None, None, is_owner, is_planner) {
                return true;
            }
        }
        false
    }

    /// Close a tab on behalf of `caller_tab_id`. Refuses a coordinator closing
    /// its OWN tab; Workers self-close on report+exit and closing any other
    /// tab is unaffected. Fail-safe: an absent caller against a coordinator
    /// target is refused (we cannot prove it is not the target closing itself),
    /// while non-coordinator targets close normally.
    pub fn close_tab_guarded(
        &self,
        caller_tab_id: Option<&str>,
        target_tab_id: &str,
    ) -> Result<(), String> {
        let (canon_target, canon_caller) = {
            let mux = self.mux.read();
            let target = mux
                .resolve_tab_id(target_tab_id)
                .unwrap_or_else(|| target_tab_id.to_string());
            let caller = caller_tab_id.and_then(|c| mux.resolve_tab_id(c));
            (target, caller)
        };
        // Absent (or unresolvable) caller is treated as self — fail-safe.
        let is_self = canon_caller
            .as_deref()
            .map(|c| c == canon_target.as_str())
            .unwrap_or(true);
        if is_self && self.is_self_close_protected(&canon_target) {
            return Err(format!(
                "{SELF_CLOSE_REFUSAL_PREFIX}: a coordinator cannot close its own Tab \
                 ({canon_target}) — have your parent or another Tab close it, or move first. \
                 Workers self-close on report+exit."
            ));
        }
        self.close_tab(target_tab_id)
    }

    pub fn close_tab(&self, tab_id: &str) -> Result<(), String> {
        self.close_tab_with_reason(tab_id, TabCloseReason::User)
    }

    /// Close a tab, tagging why. A `PolicyWorker` close of a task-owning hidden
    /// worker records a restorable recently-closed entry despite `hidden`.
    pub fn close_tab_with_reason(
        &self,
        tab_id: &str,
        reason: TabCloseReason,
    ) -> Result<(), String> {
        let (pre_focus, reservation) = {
            let mut mux = self.mux.write();
            (
                mux.get_focused_tab_id().map(str::to_string),
                mux.reserve_tab_close(tab_id)?,
            )
        };
        if let Err(error) = self.prepare_tab_close(tab_id) {
            self.mux.write().abort_tab_close(&reservation);
            return Err(error);
        }
        let pty_ids = self
            .mux
            .write()
            .close_reserved_tab_with_reason(reservation, reason)?;
        let post_focus = self.mux.read().get_focused_tab_id().map(str::to_string);
        self.kill_ptys(&pty_ids);
        self.finish_tab_close(tab_id)?;
        {
            let mut notif = self.notif.lock();
            notif.close_push_scope(&crate::notification::push::tab_scope(tab_id));
            // An unanswered question outlives its tab as a visibly undeliverable
            // card, never as buttons that would quietly do nothing.
            notif.forget_answer_tab(tab_id);
        }
        session_persist::persist_session_after_close(self);
        self.emit_state_changed();
        if pre_focus != post_focus {
            if let Some(new) = post_focus {
                self.emit_tab_focus_changed(&new);
            }
        }
        Ok(())
    }

    pub fn rename_tab(&self, tab_id: &str, title: &str) -> Result<(), String> {
        self.mux.write().rename_tab(tab_id, title)?;
        self.emit_tab_title_pinned(tab_id, title);
        self.emit_state_changed();
        session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Prompt)?;
        Ok(())
    }

    pub fn set_tab_title(&self, tab_id: &str, title: &str) -> Result<(), String> {
        self.mux
            .write()
            .rename_tab_at_level(tab_id, title, TitleLevel::Agent)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn set_tab_description(&self, tab_id: &str, description: &str) -> Result<(), String> {
        let description = normalize_tab_description(description)?;
        self.mux.write().set_tab_description(tab_id, description)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn focus_tab(&self, tab_id: &str) -> Result<(), String> {
        self.user_last_activity.mark();
        focus_tab_hot_path(
            &self.mux,
            tab_id,
            |focused| self.emit_tab_focus_changed(focused),
            || self.schedule_focused_tab_model_refresh(),
        )
    }

    pub fn link_pty(&self, tab_id: &str, pty_id: String) -> String {
        let claim = self.pty_ownership.acquire(tab_id);
        self.link_pty_with_claim(&claim, tab_id, pty_id, false)
            .pty_id
    }

    pub(crate) fn link_pty_with_claim(
        &self,
        claim: &crate::service::pty_ownership::TabPtyClaim,
        tab_id: &str,
        pty_id: String,
        launch_owned: bool,
    ) -> crate::service::pty_ownership::PtySelection {
        claim.settle(
            pty_id,
            launch_owned,
            |candidate| self.pty.lock().get_session(candidate).is_some(),
            |selected| {
                let mut mux = self.mux.write();
                if mux.get_tab(tab_id).is_none() {
                    crate::service::pty_ownership::LinkOutcome::TabMissing
                } else {
                    crate::service::pty_ownership::LinkOutcome::Linked(
                        mux.link_pty(tab_id, selected.to_string()),
                    )
                }
            },
            |orphan| {
                log::warn!(
                    "[link_pty] Tab {} rejected orphan pty {}",
                    tab_id,
                    &orphan[..orphan.len().min(8)]
                );
                self.kill_ptys(&[orphan.to_string()]);
            },
            |_| {
                crate::service::orchestrator::tab_relink_signal::notify_tab_relinked();
                self.emit_state_changed();
            },
        )
    }

    pub fn update_tab_cwd(&self, tab_id: &str, cwd: String) {
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            tab.cwd = Some(cwd);
        }
    }

    pub fn clear_tab_restored_flag(&self, tab_id: &str) {
        let mut mux = self.mux.write();
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            tab.restored_from_snapshot = false;
        }
    }

    pub fn set_tab_agent_type(&self, tab_id: &str, agent_type: Option<String>) {
        match agent_type {
            Some(agent_type) => self.write_tab_agent_type(tab_id, agent_type, false),
            None => {
                self.clear_tab_agent_identity_for_lifecycle_exit(tab_id);
            }
        }
    }

    pub(crate) fn invalidate_guarded_agent_input(&self, tab_id: &str) {
        let pty_id = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pty_id.clone());
        if let Some(pty_id) = pty_id {
            self.pty.lock().invalidate_guarded_input(&pty_id);
        }
    }

    pub(crate) fn rearm_authenticated_agent_input(&self, tab_id: &str) {
        let pty_id = self.mux.read().get_tab(tab_id).and_then(|tab| {
            (tab.agent_type.is_some()
                && tab.agent_session_id.is_some()
                && tab.session_binding_nonce == tab.reported_session_binding_nonce)
                .then(|| tab.pty_id.clone())
                .flatten()
        });
        if let Some(pty_id) = pty_id {
            let _ = self.pty.lock().rearm_guarded_input(&pty_id);
            crate::service::boot_wait::notify_boot_changed();
        }
    }

    /// Same as `set_tab_agent_type` but also pins `agent_type_locked = true`.
    /// Used by spawn paths and the typed-launch input parser to declare explicit
    /// user intent. While locked, output-based re-detection is ignored.
    pub fn set_and_lock_tab_agent_type(&self, tab_id: &str, agent_type: String) {
        self.write_tab_agent_type(tab_id, agent_type, true);
    }

    /// Release the lock without changing `agent_type`. Public so a future Tauri
    /// command can disconnect an agent identity without clearing it.
    pub fn unlock_tab_agent_type(&self, tab_id: &str) {
        let mut mux = self.mux.write();
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            tab.agent_type_locked = false;
        }
    }

    fn write_tab_agent_type(&self, tab_id: &str, agent_type: String, lock: bool) {
        let transition = {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return;
            };
            let prev = tab.agent_type.clone();
            tab.agent_set_at = Some(std::time::Instant::now());
            tab.exit_suspected_at = None;
            tab.agent_type = Some(agent_type.clone());
            if lock {
                tab.agent_type_locked = true;
            }
            (prev, agent_type)
        };
        log::info!(
            "[set_tab_agent_type] tab={} prev={:?} new={}",
            tab_id,
            transition.0,
            transition.1,
        );
        let (prev, next) = transition;
        if prev.is_none() {
            self.replay_inbound_buffer(tab_id, &next);
        }
        if prev.as_deref() != Some(next.as_str()) {
            self.kick_off_session_name_detection(tab_id, &next);
            self.refresh_session_model_once(tab_id, &next);
        }
    }

    pub(crate) fn clear_tab_agent_identity_for_lifecycle_exit(&self, tab_id: &str) -> bool {
        let cleared = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id)
                .is_some_and(clear_agent_identity_for_lifecycle_exit)
        };
        if !cleared {
            return false;
        }
        self.record_lifecycle_end(
            tab_id,
            crate::session::lifecycle::LifecycleTransition::RolledBack {
                reason: "agent identity cleared on lifecycle exit".to_string(),
            },
        );
        self.invalidate_guarded_agent_input(tab_id);
        log::info!(
            "[agent-lifecycle] cleared Tab identity after explicit exit: {}",
            tab_id
        );
        self.signal_worker_interruption(
            tab_id,
            crate::service::worker_interruption::WorkerInterruptionEvent::ProcessExited,
        );
        self.clear_dead_planner_binding(tab_id);
        self.prompt_detector.lock().clear(tab_id);
        crate::service::structured_prompt::clear_tab(tab_id);
        self.emit_state_changed();
        true
    }

    /// Boot path: refresh model once for every tab that already has an
    /// agent_type. Snapshot restore brings these back without a fresh
    /// `set_tab_agent_type` transition, so the per-set hook never fires.
    /// Also kicks the global focused-tab model poller — one thread total.
    pub fn kick_off_pollers_for_existing_agents(&self) {
        let restored: Vec<(String, String, bool)> = {
            let mux = self.mux.read();
            mux.snapshot()
                .tabs
                .values()
                .filter_map(|p| {
                    p.agent_type
                        .clone()
                        .map(|t| (p.id.clone(), t, p.agent_session_id.is_none()))
                })
                .collect()
        };
        for (tab_id, agent_id, needs_session) in restored {
            if needs_session {
                let mut mux = self.mux.write();
                if let Some(tab) = mux.get_tab_mut(&tab_id) {
                    if tab.agent_session_id.is_none() && tab.agent_set_at.is_none() {
                        tab.agent_set_at = Some(std::time::Instant::now());
                    }
                }
            }
            let seeded = {
                use crate::agent::offline_state_classifier::{
                    AgentLiveState, TabVerdict, TimelineState,
                };
                let idle: AgentLiveState = TabVerdict {
                    state: TimelineState::Idle,
                    subtype: None,
                }
                .into();
                let mut mux = self.mux.write();
                match mux.get_tab_mut(&tab_id) {
                    Some(tab) if tab.agent_type.is_some() && tab.agent_state.is_none() => {
                        tab.agent_state = Some(idle);
                        true
                    }
                    _ => false,
                }
            };
            if seeded {
                self.emit_state_changed();
            }
            self.kick_off_session_name_detection(&tab_id, &agent_id);
            self.refresh_session_model_once(&tab_id, &agent_id);
        }
        self.heal_recovery_state_if_ready();
        self.start_focused_tab_model_poller();
    }

    fn start_focused_tab_model_poller(&self) {
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

    fn schedule_focused_tab_model_refresh(&self) {
        let svc = self.clone();
        std::thread::spawn(move || {
            svc.refresh_focused_tab_model();
        });
    }

    fn refresh_session_model_once(&self, tab_id: &str, agent_id: &str) {
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
                    Some("warning"),
                );
                log::warn!(
                    "[session-model] tab={} requested_model={} actual_model={} fallback_reason=runtime_session_model_mismatch",
                    tab_id,
                    mismatch.requested_model,
                    mismatch.actual_model
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

    /// Poll the screen for the provider's session display name once the agent
    /// is settled. Stops as soon as a name is found (or after a budget) so
    /// the regex doesn't spin on every output chunk. See AgentProvider::
    /// extract_session_name; Claude returns "vivid-bhabha-1234" etc. (#99).
    fn kick_off_session_name_detection(&self, tab_id: &str, agent_id: &str) {
        if tab_id.is_empty() || agent_id.is_empty() {
            return;
        }
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        let agent_id = agent_id.to_string();
        std::thread::spawn(move || {
            let step = std::time::Duration::from_millis(750);
            // 30 attempts × 750ms = ~22s — same magnitude as readiness strategy, long enough for the splash to settle on slow hosts.
            for _ in 0..30u32 {
                std::thread::sleep(step);
                let (pty_id, already, current_agent) = {
                    let mux = svc.mux.read();
                    let Some(p) = mux.get_tab(&tab_id) else {
                        return;
                    };
                    (
                        p.pty_id.clone(),
                        p.agent_session_name.clone(),
                        p.agent_type.clone(),
                    )
                };
                if already.is_some() {
                    return;
                }
                if current_agent.as_deref() != Some(agent_id.as_str()) {
                    return;
                }
                let Some(pty_id) = pty_id else { continue };
                let screen = svc
                    .screen_state
                    .lock()
                    .get_screen_text(&pty_id)
                    .unwrap_or_default();
                if screen.is_empty() {
                    continue;
                }
                let name = svc
                    .agent_registry
                    .with_provider(&agent_id, |p| p.extract_session_name(&screen))
                    .flatten();
                if let Some(name) = name {
                    let mut mux = svc.mux.write();
                    if let Some(p) = mux.get_tab_mut(&tab_id) {
                        if p.agent_type.as_deref() == Some(agent_id.as_str())
                            && p.agent_session_name.is_none()
                        {
                            log::info!(
                                "[session-name] tab={} captured {} (provider={})",
                                tab_id,
                                name,
                                agent_id,
                            );
                            p.agent_session_name = Some(name);
                            drop(mux);
                            svc.emit_state_changed();
                        }
                    }
                    return;
                }
            }
        });
    }

    fn replay_inbound_buffer(&self, tab_id: &str, new_type: &str) {
        let queued = crate::bridge::inbound_buffer::drain(tab_id);
        if queued.is_empty() {
            return;
        }
        let pty_id = match self.resolve_pty_id(tab_id) {
            Ok(id) => id,
            Err(e) => {
                log::warn!(
                    "[bridge] cannot replay {} queued for tab={}: {}",
                    queued.len(),
                    tab_id,
                    e,
                );
                for msg in queued {
                    crate::bridge::inbound_buffer::push(tab_id, msg);
                }
                return;
            }
        };
        log::info!(
            "[bridge] replayed {} queued messages to tab={} after agent_type transition None -> Some({:?})",
            queued.len(), tab_id, new_type,
        );
        let pty = self.pty.clone();
        std::thread::spawn(move || {
            for msg in queued {
                let paste = format!("\x1b[200~{}\x1b[201~", msg);
                if let Err(e) = pty.lock().write(&pty_id, paste.as_bytes()) {
                    log::warn!("[bridge] replay write failed for pty={}: {}", pty_id, e);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                let _ = pty.lock().write(&pty_id, b"\r");
                std::thread::sleep(std::time::Duration::from_millis(2000));
                let _ = pty.lock().write(&pty_id, b"\r");
            }
        });
    }

    /// Set chat view state on a tab.
    pub fn set_chat_view(&self, tab_id: &str, enabled: bool) {
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            tab.chat_view = enabled;
        }
    }

    /// Default prompt navigation: arrow down + Enter (standard ANSI).
    fn default_prompt_response(&self, option_index: usize) -> Vec<Vec<u8>> {
        let mut steps = Vec::new();
        for _ in 0..option_index {
            steps.push(b"\x1b[B".to_vec());
        }
        steps.push(b"\r".to_vec());
        steps
    }

    /// Backward-compat alias for `set_agent_session_id`.
    pub fn set_claude_session_id(&self, tab_id: &str, session_id: Option<String>) {
        self.set_agent_session_id(tab_id, session_id);
    }

    /// Clear pending_prompt state without sending anything to PTY.
    /// Used for test cleanup and state reset.
    pub fn clear_prompt(&self, tab_id: &str) -> Result<(), String> {
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            tab.pending_prompt = None;
        }
        // Reset PromptDetector state so next inject is accepted (not blocked by same_options check)
        self.prompt_detector.lock().clear(tab_id);
        self.emit_state_changed();
        Ok(())
    }

    /// Respond to a pending prompt — sends the selection to PTY and clears the prompt.
    /// Only sends to PTY if there's actually a pending prompt (avoids phantom input).
    pub fn respond_prompt(&self, tab_id: &str, value: &str) -> Result<(), String> {
        let had_prompt = {
            let mut mux = self.mux.write();
            let tab = mux
                .get_tab_mut(tab_id)
                .ok_or_else(|| format!("Tab not found: {}", tab_id))?;
            tab.pending_prompt.take().is_some()
        };

        // Transition PromptDetector to Cooldown — suppresses re-detection of the same prompt from PTY output that hasn't been cleared yet.
        self.prompt_detector.lock().mark_responded(tab_id);

        if let Some(result) = crate::service::structured_prompt::respond(self, tab_id, value) {
            self.emit_state_changed();
            return result;
        }

        if had_prompt {
            let pty_id = self.resolve_pty_id(tab_id)?;

            let screen = {
                let buf = self.output_buf.lock();
                buf.get_text(&pty_id).unwrap_or_default()
            };
            let clean = crate::pty::ansi::clean_terminal_output(&screen);
            let agent_type = self
                .mux
                .read()
                .get_tab(tab_id)
                .and_then(|p| p.agent_type.clone());
            let prompt_on_screen = crate::agent::detector::detect_interactive_prompt(&clean)
                || agent_type.as_deref().is_some_and(|at| {
                    crate::agent::prompt::parse_prompt_safely(|| {
                        self.agent_registry.parse_prompt(at, &clean)
                    })
                    .is_some()
                });

            if prompt_on_screen {
                let option_index = value.parse::<usize>().unwrap_or(1).saturating_sub(1);

                let steps = if let Some(ref at) = agent_type {
                    self.agent_registry
                        .build_prompt_response(at, option_index, &clean)
                        .unwrap_or_else(|| self.default_prompt_response(option_index))
                } else {
                    self.default_prompt_response(option_index)
                };

                let pty = self.pty.clone();
                let pid = pty_id.clone();
                std::thread::spawn(move || {
                    for step in steps {
                        let _ = pty.lock().write(&pid, &step);
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                });
            } else {
                log::debug!(
                    "[respond_prompt] Prompt not on screen — skipping PTY input for Tab {}",
                    tab_id
                );
            }
        }

        self.emit_state_changed();
        Ok(())
    }
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

pub(crate) fn focus_tab_hot_path(
    mux: &crate::multiplexer::commands::MuxState,
    tab_id: &str,
    emit_focus_changed: impl FnOnce(&str),
    schedule_model_refresh: impl FnOnce(),
) -> Result<(), String> {
    mux.write().set_focused_tab(tab_id)?;
    emit_focus_changed(tab_id);
    schedule_model_refresh();
    Ok(())
}

#[cfg(test)]
mod self_close_tests {
    use super::*;

    #[test]
    fn orchestrator_is_coordinator() {
        assert!(is_coordinator(
            Some(AgentRole::Manager),
            Some("orchestrator"),
            false,
            false
        ));
    }

    #[test]
    fn plan_owner_is_coordinator() {
        assert!(is_coordinator(Some(AgentRole::Worker), None, true, false));
        assert!(is_coordinator(None, None, true, false));
    }

    #[test]
    fn bound_planner_is_coordinator() {
        assert!(is_coordinator(Some(AgentRole::Worker), None, false, true));
    }

    #[test]
    fn plain_worker_is_not_coordinator() {
        assert!(!is_coordinator(Some(AgentRole::Worker), None, false, false));
        assert!(!is_coordinator(None, None, false, false));
    }
}

#[cfg(test)]
mod model_identity_tests {
    use super::*;

    #[test]
    fn runtime_model_mismatch_records_actual_and_structured_reason() {
        let update = model_identity_update(
            None,
            Some("planned-codex"),
            Some("gpt-5.6-luna"),
            Some("gpt-5.6-luna"),
            None,
            "gpt-5.6-terra",
        );

        assert!(update.model_changed);
        let mismatch = update.mismatch.expect("mismatch must be structured");
        assert_eq!(mismatch.requested_model, "gpt-5.6-luna");
        assert_eq!(mismatch.planned_model.as_deref(), Some("planned-codex"));
        assert_eq!(
            mismatch.provider_resolved_model.as_deref(),
            Some("gpt-5.6-luna")
        );
        assert_eq!(mismatch.actual_model, "gpt-5.6-terra");
        assert_eq!(mismatch.reason, "runtime_session_model_mismatch");
        let fallback = update.fallback.expect("fallback must be surfaced");
        assert_eq!(fallback.actual_model.as_deref(), Some("gpt-5.6-terra"));
    }

    #[test]
    fn repeated_runtime_mismatch_keeps_actual_without_second_model_change() {
        let update = model_identity_update(
            Some("gpt-5.6-terra"),
            None,
            Some("gpt-5.6-luna"),
            Some("gpt-5.6-luna"),
            None,
            "gpt-5.6-terra",
        );

        assert!(!update.model_changed);
        assert_eq!(
            update.mismatch,
            Some(crate::multiplexer::types::ModelIdentityMismatch {
                planned_model: None,
                requested_model: "gpt-5.6-luna".into(),
                provider_resolved_model: Some("gpt-5.6-luna".into()),
                actual_model: "gpt-5.6-terra".into(),
                reason: "runtime_session_model_mismatch".into(),
            })
        );
    }

    #[test]
    fn matching_runtime_model_clears_mismatch_shape() {
        let update = model_identity_update(
            Some("gpt-5.6-luna"),
            None,
            Some("gpt-5.6-luna"),
            Some("gpt-5.6-luna"),
            None,
            "GPT-5.6-LUNA",
        );

        assert!(!update.model_changed);
        assert!(update.mismatch.is_none());
        assert!(update.fallback.is_none());
    }

    #[test]
    fn session_change_clears_stale_actual_but_keeps_launch_provenance() {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_planned_model = Some("codex/gpt-planned".into());
        tab.agent_requested_model = Some("gpt-requested".into());
        tab.agent_provider_resolved_model = Some("gpt-resolved".into());
        tab.agent_session_model = Some("gpt-actual-old".into());
        tab.model_identity_mismatch = Some(crate::multiplexer::types::ModelIdentityMismatch {
            planned_model: Some("codex/gpt-planned".into()),
            requested_model: "gpt-requested".into(),
            provider_resolved_model: Some("gpt-resolved".into()),
            actual_model: "gpt-actual-old".into(),
            reason: "runtime_session_model_mismatch".into(),
        });

        clear_runtime_model_evidence(tab);

        assert_eq!(
            tab.agent_planned_model.as_deref(),
            Some("codex/gpt-planned")
        );
        assert_eq!(tab.agent_requested_model.as_deref(), Some("gpt-requested"));
        assert_eq!(
            tab.agent_provider_resolved_model.as_deref(),
            Some("gpt-resolved")
        );
        assert!(tab.agent_session_model.is_none());
        assert!(tab.model_identity_mismatch.is_none());
    }

    #[test]
    fn delayed_runtime_model_result_cannot_cross_session_boundary() {
        let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("provider-a".into());
        tab.agent_session_id = Some("session-new".into());

        assert!(!runtime_model_update_matches_session(
            tab,
            "provider-a",
            "session-old"
        ));
        assert!(!runtime_model_update_matches_session(
            tab,
            "provider-b",
            "session-new"
        ));
        assert!(runtime_model_update_matches_session(
            tab,
            "provider-a",
            "session-new"
        ));
    }
}

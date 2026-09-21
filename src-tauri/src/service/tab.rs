//! Tab lifecycle: close, rename, focus.

use super::ServiceContext;
use crate::multiplexer::types::{AgentRole, TabCloseReason, TitleLevel};
use crate::plan::peer::{resolve_planner_ref, PeerResolution};
use crate::service::session_model::{clear_runtime_model_evidence, spawn_startup_model_refresh};
use crate::service::session_persist::{self, SessionPersistUrgency};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_TAB_DESCRIPTION_CHARS: usize = 200;
pub(crate) const TAB_TITLE_ERROR: &str =
    "tab title must be one word with no whitespace; put descriptive text in description";

pub(crate) fn validate_tab_title(title: &str) -> Result<(), String> {
    if title.is_empty() || title.chars().any(char::is_whitespace) {
        Err(TAB_TITLE_ERROR.to_string())
    } else {
        Ok(())
    }
}

pub(crate) fn normalize_optional_tab_description(
    value: Option<String>,
) -> Result<Option<String>, String> {
    value
        .as_deref()
        .map(normalize_tab_description)
        .transpose()
        .map(Option::flatten)
}

/// Prefix on every self-termination refusal so transports can classify it.
pub(crate) const SELF_CLOSE_REFUSAL_PREFIX: &str = "self-termination refused";
mod rename_guard;

pub(crate) use rename_guard::refuse_task_bound_rename;

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
    // Provider and model are one identity: a tab must never keep a model label
    // for a provider it no longer has.
    clear_runtime_model_evidence(tab);
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
    pub(crate) fn prepare_tab_close(
        &self,
        tab_id: &str,
        reason: TabCloseReason,
    ) -> Result<(), String> {
        self.chat_registry.shutdown_tab(tab_id);
        self.deferred_session_bindings.clear(tab_id);

        if let Some((plugin_id, session_id)) = self.tab_chat_backing(tab_id) {
            let _ = self.chatbackend_close(&plugin_id, &session_id);
        }

        log::info!("[tab-close] tab={tab_id} reason={}", reason.label());
        self.record_lifecycle_end(
            tab_id,
            crate::session::lifecycle::LifecycleTransition::Closed {
                reason: reason.label().to_string(),
            },
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
        self.forget_permission_auto_confirm(tab_id);
        self.forget_directory_trust(tab_id);
        crate::service::structured_prompt::clear_tab(tab_id);
        self.remote_workers.forget_orchestrator(tab_id);
        self.forget_receiver_targets_for_tab(tab_id);
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
        if let Err(error) = self.prepare_tab_close(tab_id, reason) {
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
        self.reap_orphaned_startup_teams();
        {
            let mut notif = self.notif.lock();
            notif.close_push_scope(&crate::notification::push::tab_scope(tab_id));
            // An unanswered question outlives its tab as a visibly undeliverable
            // card, never as buttons that would quietly do nothing.
            notif.forget_answer_tab(tab_id);
        }
        self.release_bridge_threads(tab_id);
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
        validate_tab_title(title)?;
        {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| format!("Tab not found: {tab_id}"))?;
            refuse_task_bound_rename(tab_id, tab.task_binding.as_ref())?;
        }
        self.mux.write().rename_tab(tab_id, title)?;
        self.emit_tab_title_pinned(tab_id, title);
        self.emit_state_changed();
        session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Prompt)?;
        Ok(())
    }

    pub fn set_tab_title(&self, tab_id: &str, title: &str) -> Result<(), String> {
        validate_tab_title(title)?;
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
            tab.launch_session_authenticated()
                .then(|| tab.pty_id.clone())
                .flatten()
        });
        if let Some(pty_id) = pty_id {
            let _ = self.pty.lock().rearm_guarded_input(&pty_id);
            crate::service::boot_wait::notify_boot_changed();
        }
    }

    pub(crate) fn clear_tab_agent_identity_for_lifecycle_exit(&self, tab_id: &str) -> bool {
        self.clear_tab_agent_identity_for_lifecycle_exit_with_cause(
            tab_id,
            "agent identity cleared on lifecycle exit",
        )
    }

    /// Same teardown, with the observed cause of death written to the lifecycle
    /// journal instead of a generic label.
    pub(crate) fn clear_tab_agent_identity_for_lifecycle_exit_with_cause(
        &self,
        tab_id: &str,
        cause: &str,
    ) -> bool {
        self.clear_tab_agent_identity_for_lifecycle_exit_during(
            tab_id,
            cause,
            crate::crash::shutdown::active_shutdown(),
        )
    }

    pub(crate) fn clear_tab_agent_identity_for_lifecycle_exit_during(
        &self,
        tab_id: &str,
        cause: &str,
        shutdown: Option<crate::crash::lifecycle::ShutdownReason>,
    ) -> bool {
        // An armed shutdown killed the provider itself; the snapshot saved at
        // exit must still carry the identity the next boot resumes.
        if let Some(reason) = shutdown {
            log::info!(
                "[agent-lifecycle] preserving Tab identity during shutdown ({reason:?}): tab={tab_id} cause={cause}"
            );
            return false;
        }
        // Every observed lifecycle exit funnels here, so the observed launch is
        // claimed before the clear below retires it.
        let exited = self.claim_launch_exit(tab_id, None, None);
        let cleared = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id)
                .is_some_and(clear_agent_identity_for_lifecycle_exit)
        };
        // After the clear, so the launch verdict is the stage that stands.
        if let Some(exited) = exited {
            self.publish_launch_exit(&exited);
        }
        if !cleared {
            return false;
        }
        self.record_lifecycle_end(
            tab_id,
            crate::session::lifecycle::LifecycleTransition::RolledBack {
                reason: cause.to_string(),
            },
        );
        self.invalidate_guarded_agent_input(tab_id);
        log::info!("[agent-lifecycle] cleared Tab identity: tab={tab_id} cause={cause}");
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
        let mut model_refreshes = Vec::with_capacity(restored.len());
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
            model_refreshes.push((tab_id, agent_id));
        }
        let svc = self.clone();
        if let Err(error) = spawn_startup_model_refresh(move || {
            for (tab_id, agent_id) in model_refreshes {
                svc.refresh_session_model_once(&tab_id, &agent_id);
            }
        }) {
            log::warn!("[session-model] failed to start restored-tab refresh: {error}");
        }
        self.heal_recovery_state_if_ready();
        self.start_focused_tab_model_poller();
    }

    /// Poll the screen for the provider's session display name once the agent
    /// is settled. Stops as soon as a name is found (or after a budget) so
    /// the regex doesn't spin on every output chunk. See AgentProvider::
    /// extract_session_name; Claude returns "vivid-bhabha-1234" etc. (#99).
    pub(crate) fn kick_off_session_name_detection(&self, tab_id: &str, agent_id: &str) {
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

    pub(crate) fn replay_inbound_buffer(&self, tab_id: &str, new_type: &str) {
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

    pub fn respond_prompt(
        &self,
        tab_id: &str,
        value: &str,
    ) -> Result<RespondPromptOutcome, String> {
        if self.mux.read().get_tab(tab_id).is_none() {
            return Err(format!("Tab not found: {}", tab_id));
        }
        let Some(option_index) = value
            .parse::<usize>()
            .ok()
            .and_then(|index| index.checked_sub(1))
        else {
            return Ok(RespondPromptOutcome::InvalidOption);
        };

        if let Some(prompt) = crate::service::structured_prompt::pending_for_tab(tab_id) {
            if option_index >= prompt.options.len() {
                return Ok(RespondPromptOutcome::InvalidOption);
            }
        }
        if let Some(result) = crate::service::structured_prompt::respond(self, tab_id, value) {
            result?;
            self.mark_prompt_responded(tab_id);
            self.emit_state_changed();
            return Ok(RespondPromptOutcome::Structured);
        }

        let Ok(pty_id) = self.resolve_pty_id(tab_id) else {
            return Ok(RespondPromptOutcome::NoPty);
        };
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
        let provider_prompt = agent_type.as_deref().and_then(|at| {
            crate::agent::prompt::parse_prompt_safely(|| {
                self.agent_registry.parse_prompt(at, &clean)
            })
        });
        let generic_option_count = crate::agent::detector::parse_permission_prompt(&clean)
            .map(|(_, options)| options.len());
        let option_count = provider_prompt
            .as_ref()
            .map(|prompt| prompt.options.len())
            .or(generic_option_count);
        let prompt_on_screen = provider_prompt.is_some()
            || generic_option_count.is_some()
            || crate::agent::detector::detect_interactive_prompt(&clean);

        if prompt_on_screen {
            let Some(option_count) = option_count else {
                return Ok(RespondPromptOutcome::PromptNotOnScreen);
            };
            if option_index >= option_count {
                return Ok(RespondPromptOutcome::InvalidOption);
            }

            let steps = if let Some(ref at) = agent_type {
                self.agent_registry
                    .build_prompt_response(at, option_index, &clean)
                    .unwrap_or_else(|| self.default_prompt_response(option_index))
            } else {
                self.default_prompt_response(option_index)
            };
            if steps.is_empty() {
                return Ok(RespondPromptOutcome::PromptNotOnScreen);
            }

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
            return Ok(RespondPromptOutcome::PromptNotOnScreen);
        }

        self.mark_prompt_responded(tab_id);
        self.emit_state_changed();
        Ok(RespondPromptOutcome::Sent)
    }

    fn mark_prompt_responded(&self, tab_id: &str) {
        if let Some(tab) = self.mux.write().get_tab_mut(tab_id) {
            tab.pending_prompt = None;
        }
        self.prompt_detector.lock().mark_responded(tab_id);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RespondPromptOutcome {
    Sent,
    Structured,
    PromptNotOnScreen,
    NoPty,
    InvalidOption,
}

#[cfg(test)]
mod respond_prompt_tests {
    use super::*;
    use crate::multiplexer::types::{PendingPrompt, PromptOption};
    use std::path::PathBuf;

    fn pending_prompt() -> PendingPrompt {
        PendingPrompt {
            question: "Choose an option".into(),
            options: vec![
                PromptOption {
                    label: "No".into(),
                    value: "1".into(),
                },
                PromptOption {
                    label: "Yes".into(),
                    value: "2".into(),
                },
            ],
            sections: None,
            multi_select: Some(false),
            submit_available: Some(true),
            selected_values: None,
        }
    }

    fn fixture_with_screen(
        screen: &str,
    ) -> (crate::service::test_fixture::ServiceFixture, String, String) {
        use crate::multiplexer::state::MultiplexerState;

        let (mut mux, tab_id) = MultiplexerState::new();
        let pty_id = format!("respond-prompt-{}", uuid::Uuid::new_v4());
        assert_eq!(mux.link_pty(&tab_id, pty_id.clone()), None);
        let prompt = pending_prompt();
        let tab = mux.get_tab_mut(&tab_id).expect("tab");
        tab.agent_type = Some("claude".into());
        tab.pending_prompt = Some(prompt.clone());
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture
            .service
            .prompt_detector
            .lock()
            .hook_detected_scraped(&tab_id, prompt);
        fixture
            .service
            .output_buf
            .lock()
            .append(&pty_id, screen.as_bytes());
        fixture
            .service
            .pty
            .lock()
            .spawn_with_id(
                Some(pty_id.clone()),
                None,
                None,
                80,
                24,
                Default::default(),
            )
            .expect("test PTY");
        (fixture, tab_id, pty_id)
    }

    fn has_input_events(cast: PathBuf) -> bool {
        std::fs::read_to_string(cast)
            .expect("capture file")
            .lines()
            .skip(1)
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<Result<Vec<_>, _>>()
            .expect("capture events")
            .iter()
            .any(|event| event.get(1).and_then(serde_json::Value::as_str) == Some("i"))
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime and live PTY")]
    #[test]
    fn respond_prompt_answers_the_new_claude_trust_shape() {
        let screen = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../plugins/provider-claude/tests/fixtures/folder-trust")
                .join("quick-safety-no-first.txt"),
        )
        .expect("captured folder-trust fixture");
        let (fixture, tab_id, pty_id) = fixture_with_screen(&screen);
        assert_eq!(
            fixture.service.respond_prompt(&tab_id, "3"),
            Ok(RespondPromptOutcome::InvalidOption)
        );
        assert_eq!(
            fixture.service.respond_prompt(&tab_id, "two"),
            Ok(RespondPromptOutcome::InvalidOption)
        );
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .pending_prompt
            .is_some());
        assert!(fixture
            .service
            .prompt_detector
            .lock()
            .active_prompt(&tab_id)
            .is_some());

        assert_eq!(
            fixture.service.respond_prompt(&tab_id, "2"),
            Ok(RespondPromptOutcome::Sent)
        );
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .pending_prompt
            .is_none());
        assert!(fixture
            .service
            .prompt_detector
            .lock()
            .active_prompt(&tab_id)
            .is_none());
        let _ = fixture.service.pty.lock().kill(&pty_id);
    }

    #[cfg_attr(not(windows), ignore = "requires a desktop runtime and live PTY")]
    #[tokio::test]
    async fn respond_prompt_reports_not_on_screen_instead_of_ok() {
        let (fixture, tab_id, pty_id) = fixture_with_screen("");
        let capture_path = fixture.temp.path().join("respond-prompt.cast");
        let capture = crate::pty::capture::CaptureRegistry::global();
        capture
            .start(&pty_id, capture_path, 80, 24, 0, Vec::new())
            .expect("capture input writes");

        let result = fixture.service.respond_prompt(&tab_id, "1");
        let cast = capture.stop(&pty_id).expect("capture stopped");
        let _ = fixture.service.pty.lock().kill(&pty_id);
        assert_eq!(result, Ok(RespondPromptOutcome::PromptNotOnScreen));
        assert!(!has_input_events(cast));
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .pending_prompt
            .is_some());
        assert!(fixture
            .service
            .prompt_detector
            .lock()
            .active_prompt(&tab_id)
            .is_some());

        let response = crate::api::agent_routes::respond_prompt_response(
            RespondPromptOutcome::PromptNotOnScreen,
        );
        assert_eq!(response.status(), axum::http::StatusCode::CONFLICT);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).expect("response JSON"),
            serde_json::json!({"ok": false, "reason": "prompt_not_on_screen"})
        );

        let invalid = crate::api::agent_routes::respond_prompt_response(
            RespondPromptOutcome::InvalidOption,
        );
        assert_eq!(invalid.status(), axum::http::StatusCode::BAD_REQUEST);
    }
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
mod tab_title_tests {
    use super::{validate_tab_title, TAB_TITLE_ERROR};

    #[test]
    fn accepts_one_word_and_rejects_prose() {
        assert!(validate_tab_title("Dalton").is_ok());
        assert_eq!(
            validate_tab_title("Warehouse party").unwrap_err(),
            TAB_TITLE_ERROR
        );
        assert_eq!(validate_tab_title("Review\u{00a0}queue").unwrap_err(), TAB_TITLE_ERROR);
    }
}

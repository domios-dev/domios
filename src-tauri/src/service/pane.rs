//! Pane lifecycle: close, rename, focus.

use super::ServiceContext;
use crate::multiplexer::types::{AgentRole, TitleLevel};
use crate::plan::peer::{resolve_planner_ref, PeerResolution};
use crate::service::session_persist::{self, SessionPersistUrgency};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_PANE_DESCRIPTION_CHARS: usize = 200;

/// Prefix on every self-termination refusal so transports can classify it.
pub(crate) const SELF_CLOSE_REFUSAL_PREFIX: &str = "self-termination refused";

/// A coordinator must not self-terminate: an Orchestrator role, an owner of a
/// plan, or a bound Planner of a plan. There is no AgentRole::Planner — planner
/// identity comes from the plan graph, not a pane role.
pub(crate) fn is_coordinator(
    role: Option<AgentRole>,
    is_plan_owner: bool,
    is_bound_planner: bool,
) -> bool {
    matches!(role, Some(AgentRole::Orchestrator)) || is_plan_owner || is_bound_planner
}

pub fn normalize_pane_description(value: &str) -> Result<Option<String>, String> {
    let trimmed = value.trim();
    if trimmed.chars().count() > MAX_PANE_DESCRIPTION_CHARS {
        return Err(format!(
            "description must be {} characters or fewer",
            MAX_PANE_DESCRIPTION_CHARS
        ));
    }
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ServiceContext {
    /// True when `pane_id` is a coordinator (Orchestrator role, plan owner, or
    /// bound Planner) and so must not close itself. Keys off caller identity in
    /// the mux + plan graph, never a display name.
    pub(crate) fn is_self_close_protected(&self, pane_id: &str) -> bool {
        let role = self.mux.read().get_pane(pane_id).and_then(|p| p.role);
        if is_coordinator(role, false, false) {
            return true;
        }
        for plan in self.plan_store.list_full() {
            let is_owner = plan.owner.pane_id.as_deref() == Some(pane_id);
            let is_planner =
                match resolve_planner_ref(&plan.planner, &plan.owner, |p| self.pane_probe(p)) {
                    PeerResolution::Planner { pane_id: p } => p == pane_id,
                    _ => false,
                };
            if is_coordinator(None, is_owner, is_planner) {
                return true;
            }
        }
        false
    }

    /// Close a pane on behalf of `caller_pane_id`. Refuses a coordinator closing
    /// its OWN pane; Workers self-close on report+exit and closing any other
    /// pane is unaffected. Fail-safe: an absent caller against a coordinator
    /// target is refused (we cannot prove it is not the target closing itself),
    /// while non-coordinator targets close normally.
    pub fn close_pane_guarded(
        &self,
        caller_pane_id: Option<&str>,
        target_pane_id: &str,
    ) -> Result<(), String> {
        let (canon_target, canon_caller) = {
            let mux = self.mux.read();
            let target = mux
                .resolve_pane_id(target_pane_id)
                .unwrap_or_else(|| target_pane_id.to_string());
            let caller = caller_pane_id.and_then(|c| mux.resolve_pane_id(c));
            (target, caller)
        };
        // Absent (or unresolvable) caller is treated as self — fail-safe.
        let is_self = canon_caller
            .as_deref()
            .map(|c| c == canon_target.as_str())
            .unwrap_or(true);
        if is_self && self.is_self_close_protected(&canon_target) {
            return Err(format!(
                "{SELF_CLOSE_REFUSAL_PREFIX}: a coordinator cannot close its own pane \
                 ({canon_target}) — have your parent or another pane close it, or move first. \
                 Workers self-close on report+exit."
            ));
        }
        self.close_pane(target_pane_id)
    }

    pub fn close_pane(&self, pane_id: &str) -> Result<(), String> {
        // iteration; once close_pane drops the entry, future polls would see Provider::None and bail anyway, but explicit stop joins the thread cleanly and frees the JSONL handle.
        self.chat_registry.shutdown_pane(pane_id);

        if let Some((plugin_id, session_id)) = self.pane_chat_backing(pane_id) {
            let _ = self.chatbackend_close(&plugin_id, &session_id);
        }

        let pending_digest = self
            .mux
            .read()
            .get_pane(pane_id)
            .and_then(|pane| pane.pending_delivery.as_ref())
            .map(|record| record.content_digest.clone());
        let pre_focus = self.mux.read().get_focused_pane_id().map(str::to_string);
        let pty_ids = self.mux.write().close_pane(pane_id)?;
        let post_focus = self.mux.read().get_focused_pane_id().map(str::to_string);
        self.kill_ptys(&pty_ids);
        self.flag_bound_tasks_for_gone_pane(pane_id, epoch_ms());
        self.prompt_detector.lock().remove_pane(pane_id);
        crate::service::structured_prompt::clear_pane(pane_id);
        if let Some(digest) = pending_digest {
            if let Err(error) = self.pending_delivery_payloads.delete(&digest) {
                log::warn!(
                    "[pending-delivery] close cleanup failed for digest {}: {}",
                    digest,
                    error
                );
            }
        }
        // Drop any remote-worker ghosts that reported to this (now-closed) pane.
        self.remote_workers.forget_orchestrator(pane_id);
        self.emit_state_changed();
        if pre_focus != post_focus {
            if let Some(new) = post_focus {
                self.emit_pane_focus_changed(&new);
            }
        }
        Ok(())
    }

    pub fn rename_pane(&self, pane_id: &str, title: &str) -> Result<(), String> {
        self.mux.write().rename_pane(pane_id, title)?;
        self.emit_pane_title_pinned(pane_id, title);
        self.emit_state_changed();
        session_persist::persist_session_after_mutation(self, SessionPersistUrgency::Prompt)?;
        Ok(())
    }

    pub fn set_pane_title(&self, pane_id: &str, title: &str) -> Result<(), String> {
        self.mux
            .write()
            .rename_pane_at_level(pane_id, title, TitleLevel::Agent)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn set_pane_description(&self, pane_id: &str, description: &str) -> Result<(), String> {
        let description = normalize_pane_description(description)?;
        self.mux
            .write()
            .set_pane_description(pane_id, description)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn focus_pane(&self, pane_id: &str) -> Result<(), String> {
        self.user_last_activity.mark();
        focus_pane_hot_path(
            &self.mux,
            pane_id,
            |focused| self.emit_pane_focus_changed(focused),
            || self.schedule_focused_pane_model_refresh(),
        )
    }

    pub fn link_pty(&self, pane_id: &str, pty_id: String) -> String {
        let claim = self.pty_ownership.acquire(pane_id);
        self.link_pty_with_claim(&claim, pane_id, pty_id, false)
            .pty_id
    }

    pub(crate) fn link_pty_with_claim(
        &self,
        claim: &crate::service::pty_ownership::PanePtyClaim,
        pane_id: &str,
        pty_id: String,
        launch_owned: bool,
    ) -> crate::service::pty_ownership::PtySelection {
        claim.settle(
            pty_id,
            launch_owned,
            |candidate| self.pty.lock().get_session(candidate).is_some(),
            |selected| {
                let mut mux = self.mux.write();
                if mux.get_pane(pane_id).is_none() {
                    crate::service::pty_ownership::LinkOutcome::PaneMissing
                } else {
                    crate::service::pty_ownership::LinkOutcome::Linked(
                        mux.link_pty(pane_id, selected.to_string()),
                    )
                }
            },
            |orphan| {
                log::warn!(
                    "[link_pty] pane {} rejected orphan pty {}",
                    pane_id,
                    &orphan[..orphan.len().min(8)]
                );
                self.kill_ptys(&[orphan.to_string()]);
            },
            |_| {
                crate::service::orchestrator::pane_relink_signal::notify_pane_relinked();
                self.emit_state_changed();
            },
        )
    }

    pub fn update_pane_cwd(&self, pane_id: &str, cwd: String) {
        if let Some(pane) = self.mux.write().get_pane_mut(pane_id) {
            pane.cwd = Some(cwd);
        }
    }

    pub fn clear_pane_restored_flag(&self, pane_id: &str) {
        let mut mux = self.mux.write();
        if let Some(pane) = mux.get_pane_mut(pane_id) {
            pane.restored_from_snapshot = false;
        }
    }

    pub fn set_pane_agent_type(&self, pane_id: &str, agent_type: Option<String>) {
        self.write_pane_agent_type(pane_id, agent_type, false);
    }

    /// Same as `set_pane_agent_type` but also pins `agent_type_locked = true`.
    /// Used by spawn paths and the typed-launch input parser to declare explicit
    /// user intent. While locked, output-based re-detection is ignored.
    pub fn set_and_lock_pane_agent_type(&self, pane_id: &str, agent_type: String) {
        self.write_pane_agent_type(pane_id, Some(agent_type), true);
    }

    /// Release the lock without changing `agent_type`. Public so a future Tauri
    /// command can disconnect an agent identity without clearing it.
    pub fn unlock_pane_agent_type(&self, pane_id: &str) {
        let mut mux = self.mux.write();
        if let Some(pane) = mux.get_pane_mut(pane_id) {
            pane.agent_type_locked = false;
        }
    }

    fn write_pane_agent_type(&self, pane_id: &str, agent_type: Option<String>, lock: bool) {
        let transition = {
            let mut mux = self.mux.write();
            let Some(pane) = mux.get_pane_mut(pane_id) else {
                return;
            };
            let prev = pane.agent_type.clone();
            if agent_type.is_some() {
                pane.agent_set_at = Some(std::time::Instant::now());
            }
            pane.agent_type = agent_type.clone();
            // Clearing agent_type also clears the lock — agent died/disconnected.
            if agent_type.is_none() {
                pane.agent_type_locked = false;
                pane.session_binding_nonce = None;
                pane.reported_session_binding_nonce = None;
                pane.agent_session_id = None;
                pane.agent_state = None;
                pane.pending_prompt = None;
                // apply_boot_stage, not a raw assignment: a failure reason latched
                // by an earlier spawn must not outlive the stage that recorded it.
                crate::service::boot_stage::apply_boot_stage(
                    pane,
                    codeterm_types::pty::BootStage::ShellReady,
                    None,
                );
            } else if lock {
                pane.agent_type_locked = true;
            }
            (prev, agent_type)
        };
        log::info!(
            "[set_pane_agent_type] pane={} prev={:?} new={:?}",
            pane_id,
            transition.0,
            transition.1,
        );
        let (prev, next) = transition;
        if prev.is_none() && next.is_some() {
            self.replay_inbound_buffer(pane_id, next.as_deref().unwrap_or(""));
        }
        if let Some(new_type) = next {
            if prev.as_deref() != Some(new_type.as_str()) {
                self.kick_off_session_name_detection(pane_id, &new_type);
                self.refresh_session_model_once(pane_id, &new_type);
            }
        } else {
            self.clear_dead_planner_binding(pane_id);
            // Drop friendly name on agent clear; keep the last-seen model so the tab doesn't flicker blank between sessions.
            let mut mux = self.mux.write();
            if let Some(pane) = mux.get_pane_mut(pane_id) {
                pane.agent_session_name = None;
            }
        }
    }

    /// Boot path: refresh model once for every pane that already has an
    /// agent_type. Snapshot restore brings these back without a fresh
    /// `set_pane_agent_type` transition, so the per-set hook never fires.
    /// Also kicks the global focused-pane model poller — one thread total.
    pub fn kick_off_pollers_for_existing_agents(&self) {
        let restored: Vec<(String, String, bool)> = {
            let mux = self.mux.read();
            mux.snapshot()
                .panes
                .values()
                .filter_map(|p| {
                    p.agent_type
                        .clone()
                        .map(|t| (p.id.clone(), t, p.agent_session_id.is_none()))
                })
                .collect()
        };
        for (pane_id, agent_id, needs_session) in restored {
            if needs_session {
                let mut mux = self.mux.write();
                if let Some(pane) = mux.get_pane_mut(&pane_id) {
                    if pane.agent_session_id.is_none() && pane.agent_set_at.is_none() {
                        pane.agent_set_at = Some(std::time::Instant::now());
                    }
                }
            }
            let seeded = {
                use crate::agent::offline_state_classifier::{
                    AgentLiveState, PaneVerdict, TimelineState,
                };
                let idle: AgentLiveState = PaneVerdict {
                    state: TimelineState::Idle,
                    subtype: None,
                }
                .into();
                let mut mux = self.mux.write();
                match mux.get_pane_mut(&pane_id) {
                    Some(pane) if pane.agent_type.is_some() && pane.agent_state.is_none() => {
                        pane.agent_state = Some(idle);
                        true
                    }
                    _ => false,
                }
            };
            if seeded {
                self.emit_state_changed();
            }
            self.kick_off_session_name_detection(&pane_id, &agent_id);
            self.refresh_session_model_once(&pane_id, &agent_id);
        }
        self.heal_recovery_state_if_ready();
        self.start_focused_pane_model_poller();
    }

    fn start_focused_pane_model_poller(&self) {
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
                    let focused = mux.snapshot().focused_pane_id;
                    focused.and_then(|id| {
                        mux.get_pane(&id)
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

    /// Bam-refresh model on the newly focused pane so the badge updates the
    /// instant the user switches tabs, instead of waiting up to one tick.
    pub fn refresh_focused_pane_model(&self) {
        let target: Option<(String, String)> = {
            let mux = self.mux.read();
            mux.snapshot().focused_pane_id.and_then(|id| {
                mux.get_pane(&id)
                    .and_then(|p| p.agent_type.clone())
                    .map(|t| (id, t))
            })
        };
        if let Some((pid, at)) = target {
            self.refresh_session_model_once(&pid, &at);
        }
    }

    fn schedule_focused_pane_model_refresh(&self) {
        let svc = self.clone();
        std::thread::spawn(move || {
            svc.refresh_focused_pane_model();
        });
    }

    fn refresh_session_model_once(&self, pane_id: &str, agent_id: &str) {
        let (cwd, session_id) = {
            let mux = self.mux.read();
            let Some(p) = mux.get_pane(pane_id) else {
                return;
            };
            (p.cwd.clone(), p.agent_session_id.clone())
        };
        let Some(cwd) = cwd else { return };
        // Provider rolled its live session (e.g. opencode /model swap): follow it to the new sid before scanning the model. Provider-owned signal (screen/event), guarded so it never steals a sibling pane's session.
        if let Some(ref current) = session_id {
            if let Some(new_sid) = self
                .agent_registry
                .with_provider(agent_id, |p| p.detect_active_session(&cwd, current))
                .flatten()
            {
                let claimed = {
                    let mut mux = self.mux.write();
                    if new_sid == *current || mux.other_session_ids(pane_id).contains(&new_sid) {
                        false
                    } else if let Some(p) = mux.get_pane_mut(pane_id) {
                        if p.agent_type.as_deref() == Some(agent_id) {
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
                        "[session-detect] rolled pane {} from {} → {} ({})",
                        pane_id,
                        current,
                        new_sid,
                        agent_id,
                    );
                    self.emit_state_changed();
                }
            }
        }
        // Model badge needs a bound session. Binding is provider-reported only — we never guess one from the filesystem here; an unbound pane just has no model badge yet.
        let sid = {
            let mux = self.mux.read();
            mux.get_pane(pane_id)
                .and_then(|p| p.agent_session_id.clone())
        };
        let Some(sid) = sid else { return };
        let (
            current_model,
            planned_model,
            requested_model,
            provider_resolved_model,
            current_fallback,
            current_mismatch,
        ) = {
            let mux = self.mux.read();
            let Some(pane) = mux.get_pane(pane_id) else {
                return;
            };
            (
                pane.agent_session_model.clone(),
                pane.agent_planned_model.clone(),
                pane.agent_requested_model.clone(),
                pane.agent_provider_resolved_model.clone(),
                pane.model_fallback.clone(),
                pane.model_identity_mismatch.clone(),
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
                    "[session-model] pane={} requested_model={} actual_model={} fallback_reason=runtime_session_model_mismatch",
                    pane_id,
                    mismatch.requested_model,
                    mismatch.actual_model
                );
            }
        }
        let model = Some(model);
        let mut mux = self.mux.write();
        if let Some(p) = mux.get_pane_mut(pane_id) {
            if runtime_model_update_matches_session(p, agent_id, &sid) {
                if identity_update.model_changed {
                    log::info!(
                        "[session-model] pane={} {:?} -> {:?}",
                        pane_id,
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
    fn kick_off_session_name_detection(&self, pane_id: &str, agent_id: &str) {
        if pane_id.is_empty() || agent_id.is_empty() {
            return;
        }
        let svc = self.clone();
        let pane_id = pane_id.to_string();
        let agent_id = agent_id.to_string();
        std::thread::spawn(move || {
            let step = std::time::Duration::from_millis(750);
            // 30 attempts × 750ms = ~22s — same magnitude as readiness strategy, long enough for the splash to settle on slow hosts.
            for _ in 0..30u32 {
                std::thread::sleep(step);
                let (pty_id, already, current_agent) = {
                    let mux = svc.mux.read();
                    let Some(p) = mux.get_pane(&pane_id) else {
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
                    if let Some(p) = mux.get_pane_mut(&pane_id) {
                        if p.agent_type.as_deref() == Some(agent_id.as_str())
                            && p.agent_session_name.is_none()
                        {
                            log::info!(
                                "[session-name] pane={} captured {} (provider={})",
                                pane_id,
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

    fn replay_inbound_buffer(&self, pane_id: &str, new_type: &str) {
        let queued = crate::bridge::inbound_buffer::drain(pane_id);
        if queued.is_empty() {
            return;
        }
        let pty_id = match self.resolve_pty_id(pane_id) {
            Ok(id) => id,
            Err(e) => {
                log::warn!(
                    "[bridge] cannot replay {} queued for pane={}: {}",
                    queued.len(),
                    pane_id,
                    e,
                );
                for msg in queued {
                    crate::bridge::inbound_buffer::push(pane_id, msg);
                }
                return;
            }
        };
        log::info!(
            "[bridge] replayed {} queued messages to pane={} after agent_type transition None -> Some({:?})",
            queued.len(), pane_id, new_type,
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

    /// Set agent session ID on a pane. Kept as `set_agent_session_id`.
    ///
    /// Rejects assignments where the same session_id is already claimed by a
    /// different pane — opencode/codex can have multiple panes detecting in
    /// the same cwd in parallel and the cheap "exclude list" race lets two
    /// of them lock onto the same fresh session unless we deduplicate
    /// atomically. Resume/snapshot rehydrate paths intentionally re-set the
    /// same id on the same pane and are unaffected.
    pub fn set_agent_session_id(&self, pane_id: &str, session_id: Option<String>) {
        let session_bound = session_id.is_some();
        let mut mux = self.mux.write();
        if let Some(ref new_sid) = session_id {
            let conflict = mux
                .pane_ids_with_session(new_sid)
                .into_iter()
                .any(|other| other != pane_id);
            if conflict {
                log::info!(
                    "[set_agent_session_id] rejecting duplicate sid {} for pane {} (claimed elsewhere)",
                    new_sid, pane_id,
                );
                return;
            }
        }
        if let Some(pane) = mux.get_pane_mut(pane_id) {
            let changed = pane.agent_session_id != session_id;
            pane.agent_session_id = session_id;
            pane.reported_session_binding_nonce = None;
            if changed {
                clear_runtime_model_evidence(pane);
            }
        }
        drop(mux);
        if session_bound {
            crate::service::general_agent::binding_wait::notify_session_bound();
        }
        self.bind_planner_session(pane_id);
        self.heal_recovery_state_if_ready();
    }

    /// Rotate the per-launch binding nonce before a provider command is written.
    pub fn rotate_session_binding_nonce(&self, pane_id: &str) -> String {
        let nonce = uuid::Uuid::new_v4().to_string();
        if let Some(pane) = self.mux.write().get_pane_mut(pane_id) {
            pane.session_binding_nonce = Some(nonce.clone());
            pane.reported_session_binding_nonce = None;
        }
        nonce
    }

    /// Per-launch binding nonce for a pane. Injected as CODETERM_SESSION_BINDING_NONCE so a provider's session-start report can prove it owns this pane; a report carrying a different nonce is a stale/foreign claim and gets rejected.
    pub fn ensure_session_binding_nonce(&self, pane_id: &str) -> String {
        let mut mux = self.mux.write();
        if let Some(pane) = mux.get_pane_mut(pane_id) {
            if let Some(ref n) = pane.session_binding_nonce {
                return n.clone();
            }
            let nonce = uuid::Uuid::new_v4().to_string();
            pane.session_binding_nonce = Some(nonce.clone());
            pane.reported_session_binding_nonce = None;
            nonce
        } else {
            uuid::Uuid::new_v4().to_string()
        }
    }

    /// Per-session shell-integration nonce, generated once and reused. Exported as
    /// CODETERM_SHELL_NONCE so the injected shell script can stamp it onto OSC 633;E;
    /// the marker consumer (OSC3) rejects command-line text whose nonce mismatches.
    pub fn ensure_shell_integration_nonce(&self, pane_id: &str) -> String {
        let mut mux = self.mux.write();
        if let Some(pane) = mux.get_pane_mut(pane_id) {
            if let Some(ref n) = pane.shell_integration_nonce {
                return n.clone();
            }
            let nonce = uuid::Uuid::new_v4().to_string();
            pane.shell_integration_nonce = Some(nonce.clone());
            nonce
        } else {
            uuid::Uuid::new_v4().to_string()
        }
    }

    /// Set chat view state on a pane.
    pub fn set_chat_view(&self, pane_id: &str, enabled: bool) {
        if let Some(pane) = self.mux.write().get_pane_mut(pane_id) {
            pane.chat_view = enabled;
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
    pub fn set_claude_session_id(&self, pane_id: &str, session_id: Option<String>) {
        self.set_agent_session_id(pane_id, session_id);
    }

    /// Clear pending_prompt state without sending anything to PTY.
    /// Used for test cleanup and state reset.
    pub fn clear_prompt(&self, pane_id: &str) -> Result<(), String> {
        if let Some(pane) = self.mux.write().get_pane_mut(pane_id) {
            pane.pending_prompt = None;
        }
        // Reset PromptDetector state so next inject is accepted (not blocked by same_options check)
        self.prompt_detector.lock().clear(pane_id);
        self.emit_state_changed();
        Ok(())
    }

    /// Respond to a pending prompt — sends the selection to PTY and clears the prompt.
    /// Only sends to PTY if there's actually a pending prompt (avoids phantom input).
    pub fn respond_prompt(&self, pane_id: &str, value: &str) -> Result<(), String> {
        let had_prompt = {
            let mut mux = self.mux.write();
            let pane = mux
                .get_pane_mut(pane_id)
                .ok_or_else(|| format!("Pane not found: {}", pane_id))?;
            let had = pane.pending_prompt.is_some();
            pane.pending_prompt = None;
            had
        };

        // Transition PromptDetector to Cooldown — suppresses re-detection of the same prompt from PTY output that hasn't been cleared yet.
        self.prompt_detector.lock().mark_responded(pane_id);

        if let Some(result) = crate::service::structured_prompt::respond(self, pane_id, value) {
            self.emit_state_changed();
            return result;
        }

        if had_prompt {
            let pty_id = self.resolve_pty_id(pane_id)?;

            let screen = {
                let buf = self.output_buf.lock();
                buf.get_text(&pty_id).unwrap_or_default()
            };
            let clean = crate::pty::ansi::clean_terminal_output(&screen);
            let agent_type = self
                .mux
                .read()
                .get_pane(pane_id)
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
                    "[respond_prompt] Prompt not on screen — skipping PTY input for pane {}",
                    pane_id
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

pub(crate) fn clear_runtime_model_evidence(pane: &mut crate::multiplexer::types::Pane) {
    pane.agent_session_model = None;
    pane.model_identity_mismatch = None;
    if let Some(fallback) = pane.model_fallback.as_mut() {
        fallback.actual_model = None;
    }
}

fn runtime_model_update_matches_session(
    pane: &crate::multiplexer::types::Pane,
    agent_id: &str,
    session_id: &str,
) -> bool {
    pane.agent_type.as_deref() == Some(agent_id)
        && pane.agent_session_id.as_deref() == Some(session_id)
}

pub(crate) fn focus_pane_hot_path(
    mux: &crate::multiplexer::commands::MuxState,
    pane_id: &str,
    emit_focus_changed: impl FnOnce(&str),
    schedule_model_refresh: impl FnOnce(),
) -> Result<(), String> {
    mux.write().set_focused_pane(pane_id)?;
    emit_focus_changed(pane_id);
    schedule_model_refresh();
    Ok(())
}

#[cfg(test)]
mod self_close_tests {
    use super::*;

    #[test]
    fn orchestrator_is_coordinator() {
        assert!(is_coordinator(Some(AgentRole::Orchestrator), false, false));
    }

    #[test]
    fn plan_owner_is_coordinator() {
        assert!(is_coordinator(Some(AgentRole::Worker), true, false));
        assert!(is_coordinator(None, true, false));
    }

    #[test]
    fn bound_planner_is_coordinator() {
        assert!(is_coordinator(Some(AgentRole::Worker), false, true));
    }

    #[test]
    fn plain_worker_is_not_coordinator() {
        assert!(!is_coordinator(Some(AgentRole::Worker), false, false));
        assert!(!is_coordinator(None, false, false));
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
        let (mut mux, pane_id) = crate::multiplexer::state::MultiplexerState::new();
        let pane = mux.get_pane_mut(&pane_id).unwrap();
        pane.agent_planned_model = Some("codex/gpt-planned".into());
        pane.agent_requested_model = Some("gpt-requested".into());
        pane.agent_provider_resolved_model = Some("gpt-resolved".into());
        pane.agent_session_model = Some("gpt-actual-old".into());
        pane.model_identity_mismatch = Some(crate::multiplexer::types::ModelIdentityMismatch {
            planned_model: Some("codex/gpt-planned".into()),
            requested_model: "gpt-requested".into(),
            provider_resolved_model: Some("gpt-resolved".into()),
            actual_model: "gpt-actual-old".into(),
            reason: "runtime_session_model_mismatch".into(),
        });

        clear_runtime_model_evidence(pane);

        assert_eq!(
            pane.agent_planned_model.as_deref(),
            Some("codex/gpt-planned")
        );
        assert_eq!(pane.agent_requested_model.as_deref(), Some("gpt-requested"));
        assert_eq!(
            pane.agent_provider_resolved_model.as_deref(),
            Some("gpt-resolved")
        );
        assert!(pane.agent_session_model.is_none());
        assert!(pane.model_identity_mismatch.is_none());
    }

    #[test]
    fn delayed_runtime_model_result_cannot_cross_session_boundary() {
        let (mut mux, pane_id) = crate::multiplexer::state::MultiplexerState::new();
        let pane = mux.get_pane_mut(&pane_id).unwrap();
        pane.agent_type = Some("provider-a".into());
        pane.agent_session_id = Some("session-new".into());

        assert!(!runtime_model_update_matches_session(
            pane,
            "provider-a",
            "session-old"
        ));
        assert!(!runtime_model_update_matches_session(
            pane,
            "provider-b",
            "session-new"
        ));
        assert!(runtime_model_update_matches_session(
            pane,
            "provider-a",
            "session-new"
        ));
    }
}

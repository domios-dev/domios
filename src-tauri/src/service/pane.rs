//! Pane lifecycle: close, rename, focus.

use super::ServiceContext;

impl ServiceContext {
    pub fn close_pane(&self, pane_id: &str) -> Result<(), String> {
        // Tear down the chat tailer (if any) BEFORE pane state goes away — the tailer thread reads provider/cwd/session_id from the mux on its first
        // iteration; once close_pane drops the entry, future polls would see Provider::None and bail anyway, but explicit stop joins the thread cleanly and frees the JSONL handle.
        self.chat_registry.stop(pane_id);

        // Tear down the plugin chatBackend session BEFORE the pane entry is
        // dropped — otherwise `pane_chat_backing` can no longer resolve it and
        // the plugin leaks the session (model/context) until its own reaper.
        // Best-effort: a failing closeSession must not block pane teardown.
        if let Some((plugin_id, session_id)) = self.pane_chat_backing(pane_id) {
            let _ = self.chatbackend_close(&plugin_id, &session_id);
        }

        let pre_focus = self.mux.write().get_focused_pane_id().map(str::to_string);
        let pty_ids = self.mux.write().close_pane(pane_id)?;
        let post_focus = self.mux.write().get_focused_pane_id().map(str::to_string);
        self.kill_ptys(&pty_ids);
        self.prompt_detector.lock().remove_pane(pane_id);
        self.emit_state_changed();
        if pre_focus != post_focus {
            if let Some(new) = post_focus {
                self.emit_pane_focus_changed(&new);
            }
        }
        Ok(())
    }

    pub fn rename_pane(&self, pane_id: &str, title: &str) -> Result<(), String> {
        // Sync tab.title so the sidebar label matches.
        let tab_sync = {
            let mux = self.mux.write();
            let mut sync = None;
            'outer: for ws in mux.snapshot().workspaces.iter() {
                for tab in &ws.tabs {
                    if tab.pane_id == pane_id {
                        sync = Some((ws.id.clone(), tab.id.clone()));
                        break 'outer;
                    }
                }
            }
            sync
        };
        self.mux.write().rename_pane(pane_id, title)?;
        if let Some((ws_id, tab_id)) = tab_sync {
            let _ = self.mux.write().rename_tab(&ws_id, &tab_id, title);
        }
        self.emit_pane_title_pinned(pane_id, title);
        self.emit_state_changed();
        Ok(())
    }

    /// Label only the pane, leaving the tab's (auto-generated, unique) name untouched.
    pub fn set_pane_title(&self, pane_id: &str, title: &str) -> Result<(), String> {
        self.mux.write().rename_pane(pane_id, title)?;
        self.emit_pane_title_pinned(pane_id, title);
        self.emit_state_changed();
        Ok(())
    }

    pub fn focus_pane(&self, pane_id: &str) -> Result<(), String> {
        self.mux.write().set_focused_pane(pane_id)?;
        self.emit_pane_focus_changed(pane_id);
        self.refresh_focused_pane_model();
        Ok(())
    }

    pub fn link_pty(&self, pane_id: &str, pty_id: String) {
        if let Some(orphan) = self.mux.write().link_pty(pane_id, pty_id) {
            log::warn!(
                "[link_pty] pane {} relinked — killing orphan pty {}",
                pane_id,
                &orphan[..orphan.len().min(8)]
            );
            self.kill_ptys(&[orphan]);
        }
        self.emit_state_changed();
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
            let mux = self.mux.write();
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
            self.kick_off_session_name_detection(&pane_id, &agent_id);
            self.refresh_session_model_once(&pane_id, &agent_id);
        }
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
                    let mux = svc.mux.write();
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
            let mux = self.mux.write();
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

    fn refresh_session_model_once(&self, pane_id: &str, agent_id: &str) {
        let (cwd, session_id, current_model) = {
            let mux = self.mux.write();
            let Some(p) = mux.get_pane(pane_id) else {
                return;
            };
            (
                p.cwd.clone(),
                p.agent_session_id.clone(),
                p.agent_session_model.clone(),
            )
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
                    if mux.other_session_ids(pane_id).contains(&new_sid) {
                        false
                    } else if let Some(p) = mux.get_pane_mut(pane_id) {
                        if p.agent_type.as_deref() == Some(agent_id) {
                            p.agent_session_id = Some(new_sid.clone());
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
            let mux = self.mux.write();
            mux.get_pane(pane_id)
                .and_then(|p| p.agent_session_id.clone())
        };
        let Some(sid) = sid else { return };
        // A failed detection (None) must not clobber a known model — restore seeds it from the snapshot and the session file may not name the model yet.
        let Some(model) = self
            .agent_registry
            .with_provider(agent_id, |p| p.detect_session_model(&cwd, &sid))
            .flatten()
        else {
            return;
        };
        if Some(&model) == current_model.as_ref() {
            return;
        }
        let model = Some(model);
        let mut mux = self.mux.write();
        if let Some(p) = mux.get_pane_mut(pane_id) {
            if p.agent_type.as_deref() == Some(agent_id) {
                log::info!(
                    "[session-model] pane={} {:?} -> {:?}",
                    pane_id,
                    p.agent_session_model,
                    model
                );
                p.agent_session_model = model;
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
                    let mux = svc.mux.write();
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
            pane.agent_session_id = session_id;
        }
    }

    /// Per-launch binding nonce for a pane, generated once and reused. Injected as CODETERM_SESSION_BINDING_NONCE so a provider's session-start report can prove it owns this pane; a report carrying a different nonce is a stale/foreign claim and gets rejected.
    pub fn ensure_session_binding_nonce(&self, pane_id: &str) -> String {
        let mut mux = self.mux.write();
        if let Some(pane) = mux.get_pane_mut(pane_id) {
            if let Some(ref n) = pane.session_binding_nonce {
                return n.clone();
            }
            let nonce = uuid::Uuid::new_v4().to_string();
            pane.session_binding_nonce = Some(nonce.clone());
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

        // Only send to PTY if there was an actual prompt — prevents phantom input
        if had_prompt {
            let pty_id = self.resolve_pty_id(pane_id)?;

            // Verify prompt is on screen before sending keystrokes
            let screen = {
                let buf = self.output_buf.lock();
                buf.get_text(&pty_id).unwrap_or_default()
            };
            let clean = crate::pty::ansi::clean_terminal_output(&screen);
            let prompt_on_screen = crate::agent::detector::detect_interactive_prompt(&clean);

            if prompt_on_screen {
                let option_index = value.parse::<usize>().unwrap_or(1).saturating_sub(1);

                let agent_type = self
                    .mux
                    .write()
                    .get_pane(pane_id)
                    .and_then(|p| p.agent_type.clone());
                let steps = if let Some(ref at) = agent_type {
                    self.agent_registry
                        .build_prompt_response(at, option_index)
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

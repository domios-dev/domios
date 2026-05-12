//! Pane lifecycle: split, close, rename, focus, browser.

use crate::multiplexer::types::Orientation;
use super::ServiceContext;

impl ServiceContext {
    pub fn split_pane(&self, pane_id: &str, orientation: Orientation) -> Result<String, String> {
        let new_pane_id = self.mux.lock().split_pane(pane_id, orientation)?;
        self.emit_state_changed();
        Ok(new_pane_id)
    }

    pub fn close_pane(&self, pane_id: &str) -> Result<(), String> {
        let pre_focus = self.mux.lock().get_focused_pane_id().map(str::to_string);
        let pty_ids = self.mux.lock().close_pane(pane_id)?;
        let post_focus = self.mux.lock().get_focused_pane_id().map(str::to_string);
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
        self.mux.lock().rename_pane(pane_id, title)?;
        self.emit_pane_title_pinned(pane_id, title);
        self.emit_state_changed();
        Ok(())
    }

    pub fn focus_pane(&self, pane_id: &str) -> Result<(), String> {
        self.mux.lock().set_focused_pane(pane_id)?;
        self.emit_pane_focus_changed(pane_id);
        Ok(())
    }

    pub fn open_browser(&self, url: String, activate: bool) -> Result<String, String> {
        let pane_id = self.mux.lock().open_browser(Some(url), activate)?;
        self.emit_state_changed();
        Ok(pane_id)
    }

    pub fn link_pty(&self, pane_id: &str, pty_id: String) {
        if let Some(orphan) = self.mux.lock().link_pty(pane_id, pty_id) {
            log::warn!("[link_pty] pane {} relinked — killing orphan pty {}", pane_id, &orphan[..orphan.len().min(8)]);
            self.kill_ptys(&[orphan]);
        }
        self.emit_state_changed();
    }

    pub fn update_pane_cwd(&self, pane_id: &str, cwd: String) {
        if let Some(pane) = self.mux.lock().get_pane_mut(pane_id) {
            pane.cwd = Some(cwd);
        }
    }

    pub fn clear_pane_restored_flag(&self, pane_id: &str) {
        let mut mux = self.mux.lock();
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
        let mut mux = self.mux.lock();
        if let Some(pane) = mux.get_pane_mut(pane_id) {
            pane.agent_type_locked = false;
        }
    }

    fn write_pane_agent_type(&self, pane_id: &str, agent_type: Option<String>, lock: bool) {
        let transition = {
            let mut mux = self.mux.lock();
            let Some(pane) = mux.get_pane_mut(pane_id) else { return };
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
            pane_id, transition.0, transition.1,
        );
        let (prev, next) = transition;
        if prev.is_none() && next.is_some() {
            self.replay_inbound_buffer(pane_id, next.as_deref().unwrap_or(""));
        }
        // Trigger session polling on every fresh None→Some OR Some→Some(diff)
        // transition so codex/opencode panes (no OSC title emit, never trip
        // the xterm-based frontend detection) still get an agent_session_id
        // saved without depending on each provider hitting a permission prompt
        // before their `detect_from_output` patterns can match.
        if let Some(new_type) = next {
            if prev.as_deref() != Some(new_type.as_str()) {
                self.kick_off_session_detection(pane_id, &new_type);
            }
        }
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
                    queued.len(), pane_id, e,
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

    /// Background loop that calls `detect_session_id` until it returns Some
    /// or the budget expires. Provider plugins are responsible for the actual
    /// filesystem walk; this wrapper just retries with backoff because the
    /// session file is written *after* the agent CLI accepts its first prompt.
    fn kick_off_session_detection(&self, pane_id: &str, agent_id: &str) {
        if pane_id.is_empty() || agent_id.is_empty() {
            return;
        }
        let svc = self.clone();
        let pane_id = pane_id.to_string();
        let agent_id = agent_id.to_string();
        // Wall-clock floor for session matches. Sessions created BEFORE this
        // detection thread spawned are never the right answer for a fresh
        // pane — they belong to whatever pane was running before. Without
        // this floor, a fresh pane in a busy cwd would happily claim a
        // session created seconds earlier by a sibling/rehydrated pane.
        let detection_started = std::time::SystemTime::now();
        // Tiny grace so a session created a hair before set_pane_agent_type
        // (e.g. provider TUI starts ~ms before our agent_type write lands)
        // still matches.
        let grace = std::time::Duration::from_secs(2);
        std::thread::spawn(move || {
            // 15min budget — codex writes its rollout only on first user message.
            let max_attempts = 300u32;
            let step = std::time::Duration::from_secs(3);
            for _ in 0..max_attempts {
                std::thread::sleep(step);
                let (cwd, sid_already, exclude) = {
                    let mux = svc.mux.lock();
                    let Some(p) = mux.get_pane(&pane_id) else { return };
                    if p.agent_type.as_deref() != Some(agent_id.as_str()) {
                        return; // user/system swapped agent, abort
                    }
                    (p.cwd.clone(), p.agent_session_id.clone(), mux.other_session_ids(&pane_id))
                };
                if sid_already.is_some() {
                    return;
                }
                let Some(cwd) = cwd else { continue };
                // Dynamic max_age: only consider sessions created since this
                // detection thread spawned (plus a small grace). As the
                // thread ages, the window grows naturally — any session the
                // matching CLI process is going to create will fall in it,
                // anything older won't.
                let elapsed = std::time::SystemTime::now()
                    .duration_since(detection_started)
                    .unwrap_or(std::time::Duration::ZERO);
                let max_age = Some(elapsed + grace);
                let sid = svc
                    .agent_registry
                    .with_provider(&agent_id, |p| p.detect_session_id(&cwd, &exclude, max_age))
                    .flatten();
                if let Some(sid) = sid {
                    // Atomic claim under the mux lock — between releasing the
                    // lock above and detection returning, another pane's
                    // detector may have already saved the same sid (it shows
                    // up in `other_session_ids` only after the save). Re-check
                    // and skip if claimed; otherwise commit.
                    enum ClaimOutcome { Wrote, AlreadySet, LostRace, PaneGone }
                    let outcome = {
                        let mut mux = svc.mux.lock();
                        if mux.other_session_ids(&pane_id).contains(&sid) {
                            ClaimOutcome::LostRace
                        } else if let Some(p) = mux.get_pane_mut(&pane_id) {
                            if p.agent_session_id.is_none() {
                                p.agent_session_id = Some(sid.clone());
                                ClaimOutcome::Wrote
                            } else {
                                ClaimOutcome::AlreadySet
                            }
                        } else {
                            ClaimOutcome::PaneGone
                        }
                    };
                    match outcome {
                        ClaimOutcome::Wrote => {
                            log::info!(
                                "[session-detect] auto-saved {} for pane {} (provider={})",
                                sid, pane_id, agent_id,
                            );
                            svc.emit_state_changed();
                            return;
                        }
                        ClaimOutcome::AlreadySet => {
                            log::debug!(
                                "[session-detect] pane {} already had a session_id from another path — exiting",
                                pane_id,
                            );
                            return;
                        }
                        ClaimOutcome::PaneGone => return,
                        ClaimOutcome::LostRace => {
                            log::info!(
                                "[session-detect] candidate {} already claimed by another pane — retrying for pane {}",
                                sid, pane_id,
                            );
                        }
                    }
                }
            }
            let total_secs = max_attempts as u64 * step.as_secs();
            log::warn!(
                "[session-detect] gave up after {}s for pane {} (provider={})",
                total_secs, pane_id, agent_id,
            );
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
        let mut mux = self.mux.lock();
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

    /// Set chat view state on a pane.
    pub fn set_chat_view(&self, pane_id: &str, enabled: bool) {
        if let Some(pane) = self.mux.lock().get_pane_mut(pane_id) {
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
        if let Some(pane) = self.mux.lock().get_pane_mut(pane_id) {
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
            let mut mux = self.mux.lock();
            let pane = mux.get_pane_mut(pane_id)
                .ok_or_else(|| format!("Pane not found: {}", pane_id))?;
            let had = pane.pending_prompt.is_some();
            pane.pending_prompt = None;
            had
        };

        // Transition PromptDetector to Cooldown — suppresses re-detection of the
        // same prompt from PTY output that hasn't been cleared yet.
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

                let agent_type = self.mux.lock().get_pane(pane_id)
                    .and_then(|p| p.agent_type.clone());
                let steps = if let Some(ref at) = agent_type {
                    self.agent_registry.build_prompt_response(at, option_index)
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
                log::debug!("[respond_prompt] Prompt not on screen — skipping PTY input for pane {}", pane_id);
            }
        }

        self.emit_state_changed();
        Ok(())
    }
}

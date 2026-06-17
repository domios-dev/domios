//! Agent restart: graceful /exit + auto-resume, or force-kill PTY + relaunch.
//!
//! Wraps the manual /exit + resume dance documented in AGENT.md as a single
//! atomic primitive callable from the CLI (`codeterm agent restart`) and from
//! the self-update flow (`feat/auto-update`) which restarts every agent pane
//! around binary swap.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::ServiceContext;
use crate::agent::provider::AgentLaunchParams;

/// Minimum seconds between consecutive restarts for a single pane.
pub const RESTART_RATE_LIMIT_SECS: u64 = 30;

/// Default grace period before force-killing the agent process.
pub const DEFAULT_GRACE_SECS: u64 = 10;

/// Tracks the last successful restart per pane for rate limiting.
/// Lives on `ServiceContext` and survives across requests.
pub type RestartTracker = Arc<Mutex<HashMap<String, Instant>>>;

pub fn new_restart_tracker() -> RestartTracker {
    Arc::new(Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone)]
pub enum AgentRestartError {
    NotFound(String),
    BadRequest(String),
    RateLimited { retry_after_secs: u64 },
    Internal(String),
}

#[derive(Debug, Clone)]
pub struct RestartOutcome {
    pub pane_id: String,
    pub provider: String,
    pub session_id: Option<String>,
    pub session_was_stale: bool,
    pub force: bool,
}

/// Snapshot of the bits of pane state we need to drive a restart.
/// Captured once under the mux lock so the async task does not need to
/// re-acquire it just to read fields.
#[derive(Debug, Clone)]
struct PaneRestartSnapshot {
    pane_id: String,
    pty_id: String,
    provider: String,
    session_id: Option<String>,
    cwd: Option<String>,
    auto_approve: bool,
}

impl ServiceContext {
    /// Atomic agent restart. Validates pane + rate limit synchronously,
    /// then spawns a tokio task that drives the actual /exit → wait → resume
    /// sequence. Returns immediately with 202-style outcome.
    pub fn agent_restart(
        &self,
        pane_id: &str,
        force: bool,
        grace_secs: u64,
    ) -> Result<RestartOutcome, AgentRestartError> {
        let snap = self.snapshot_pane_for_restart(pane_id)?;

        self.check_and_record_restart(&snap.pane_id)?;

        let session_was_stale = !self.session_file_present(&snap);

        let outcome = RestartOutcome {
            pane_id: snap.pane_id.clone(),
            provider: snap.provider.clone(),
            session_id: snap.session_id.clone(),
            session_was_stale,
            force,
        };

        let svc = self.clone();
        let snap_async = snap.clone();
        let grace = Duration::from_secs(grace_secs.max(1));
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                drive_restart(svc, snap_async, force, grace).await;
            });
        } else {
            std::thread::spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        log::error!("[agent_restart] failed to build runtime: {}", e);
                        return;
                    }
                };
                rt.block_on(async move {
                    drive_restart(svc, snap_async, force, grace).await;
                });
            });
        }

        Ok(outcome)
    }

    fn snapshot_pane_for_restart(
        &self,
        pane_id: &str,
    ) -> Result<PaneRestartSnapshot, AgentRestartError> {
        let mux = self.mux.lock();
        let resolved = mux
            .resolve_pane_id(pane_id)
            .ok_or_else(|| AgentRestartError::NotFound(format!("pane {} not found", pane_id)))?;
        let pane = mux
            .get_pane(&resolved)
            .ok_or_else(|| AgentRestartError::NotFound(format!("pane {} not found", pane_id)))?;

        let provider = pane
            .agent_type
            .clone()
            .ok_or_else(|| AgentRestartError::BadRequest("no agent in this pane".to_string()))?;
        let pty_id = pane
            .pty_id
            .clone()
            .ok_or_else(|| AgentRestartError::BadRequest("pane has no PTY".to_string()))?;

        if self
            .agent_registry
            .with_provider(&provider, |_| ())
            .is_none()
        {
            return Err(AgentRestartError::BadRequest(format!(
                "unknown agent provider: {}",
                provider
            )));
        }

        Ok(PaneRestartSnapshot {
            pane_id: resolved,
            pty_id,
            provider,
            session_id: pane.agent_session_id.clone(),
            cwd: pane.cwd.clone(),
            auto_approve: pane.auto_approve,
        })
    }

    fn check_and_record_restart(&self, pane_id: &str) -> Result<(), AgentRestartError> {
        let mut tracker = self.restart_tracker.lock();
        let now = Instant::now();
        if let Some(last) = tracker.get(pane_id) {
            let elapsed = now.duration_since(*last);
            let limit = Duration::from_secs(RESTART_RATE_LIMIT_SECS);
            if elapsed < limit {
                let retry = (limit - elapsed).as_secs().max(1);
                return Err(AgentRestartError::RateLimited {
                    retry_after_secs: retry,
                });
            }
        }
        tracker.insert(pane_id.to_string(), now);
        Ok(())
    }

    fn session_file_present(&self, snap: &PaneRestartSnapshot) -> bool {
        let (Some(cwd), Some(sid)) = (snap.cwd.as_deref(), snap.session_id.as_deref()) else {
            return false;
        };
        self.agent_registry
            .with_provider(&snap.provider, |p| p.session_exists(cwd, sid))
            .unwrap_or(false)
    }
}

/// Build the shell command that re-launches the agent inside the existing PTY.
/// If the recorded session id is still valid, prefer the provider's resume
/// command. Otherwise fall back to a fresh launch — the spec calls for graceful
/// degradation on stale ids, not failure.
fn build_relaunch_command(
    registry: &crate::agent::registry::AgentRegistry,
    provider: &str,
    session_id: Option<&str>,
    auto_approve: bool,
    session_present: bool,
) -> Option<String> {
    if session_present {
        if let Some(sid) = session_id {
            if let Some(cmd) = registry
                .with_provider(provider, |p| p.build_resume_command(sid, auto_approve))
                .flatten()
            {
                return Some(cmd);
            }
        }
    }
    registry.with_provider(provider, |p| {
        p.build_launch_command(&AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: auto_approve,
            session_id: None,
        })
    })
}

async fn drive_restart(
    svc: ServiceContext,
    snap: PaneRestartSnapshot,
    force: bool,
    grace: Duration,
) {
    let session_present = svc.session_file_present(&snap);
    if !session_present && snap.session_id.is_some() {
        log::warn!(
            "[agent_restart] pane={} session_id={:?} not found on disk — will start fresh",
            snap.pane_id,
            snap.session_id
        );
    }

    if !force {
        if let Some(exit_cmd) = svc
            .agent_registry
            .with_provider(&snap.provider, |p| p.exit_command().map(|s| s.to_string()))
            .flatten()
        {
            log::info!(
                "[agent_restart] pane={} sending {} (graceful)",
                snap.pane_id,
                exit_cmd
            );
            if let Err(e) = svc.send_text(&snap.pane_id, &exit_cmd, true) {
                log::warn!(
                    "[agent_restart] graceful exit send failed for pane={}: {}",
                    snap.pane_id,
                    e
                );
            }
        } else {
            log::info!(
                "[agent_restart] pane={} provider={} has no exit_command — falling through to grace wait",
                snap.pane_id,
                snap.provider
            );
        }
    }

    let shell_ready = wait_for_shell_after_exit(&svc, &snap, force, grace).await;
    if !shell_ready {
        log::error!(
            "[agent_restart] pane={} agent TUI still on screen after exit + Ctrl-C escalation — \
             aborting relaunch to avoid piping launch command into the TUI's input box. \
             User can re-run restart with force=true or close the pane manually.",
            snap.pane_id
        );
        crate::logs::record(
            crate::logs::LogLevel::Warn,
            "agent.restart",
            format!(
                "restart aborted on pane {} — agent did not exit cleanly; re-run with force=true",
                snap.pane_id
            ),
            Some(serde_json::json!({
                "pane_id": snap.pane_id,
                "provider": snap.provider,
                "reason": "tui_still_visible",
            })),
        );
        return;
    }

    let Some(launch_cmd) = build_relaunch_command(
        &svc.agent_registry,
        &snap.provider,
        snap.session_id.as_deref(),
        snap.auto_approve,
        session_present,
    ) else {
        log::error!(
            "[agent_restart] pane={} provider={} returned no launch command — restart aborted",
            snap.pane_id,
            snap.provider
        );
        return;
    };

    log::info!(
        "[agent_restart] pane={} launching: {}",
        snap.pane_id,
        launch_cmd
    );
    if let Err(e) = svc.send_text(&snap.pane_id, &launch_cmd, true) {
        log::error!(
            "[agent_restart] pane={} relaunch send failed: {}",
            snap.pane_id,
            e
        );
    }
}

async fn wait_for_shell_after_exit(
    svc: &ServiceContext,
    snap: &PaneRestartSnapshot,
    force: bool,
    grace: Duration,
) -> bool {
    if force {
        log::info!(
            "[agent_restart] pane={} force=true — sending Ctrl-C and waiting briefly",
            snap.pane_id
        );
        let _ = svc.write_raw(&snap.pane_id, b"\x03");
        tokio::time::sleep(Duration::from_millis(300)).await;
        let _ = svc.write_raw(&snap.pane_id, b"\x03");
        return poll_bare_shell_prompt(svc, &snap.pty_id, Duration::from_secs(3)).await;
    }

    if poll_bare_shell_prompt(svc, &snap.pty_id, grace).await {
        return true;
    }

    log::warn!(
        "[agent_restart] pane={} graceful exit timed out — escalating to Ctrl-C",
        snap.pane_id
    );
    let _ = svc.write_raw(&snap.pane_id, b"\x03");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let _ = svc.write_raw(&snap.pane_id, b"\x03");
    poll_bare_shell_prompt(svc, &snap.pty_id, Duration::from_secs(3)).await
}

/// Polls the screen until a bare shell prompt is visible (no agent TUI on screen)
/// or the timeout elapses. Unlike the generic `poll_shell_prompt`, this rejects
/// screens where the agent TUI's input cursor (`❯`) is still drawn inside a
/// box-border — the calling code must KNOW the agent is gone, not just see an
/// arrow somewhere.
async fn poll_bare_shell_prompt(svc: &ServiceContext, pty_id: &str, timeout: Duration) -> bool {
    let start = Instant::now();
    let poll = Duration::from_millis(200);
    while start.elapsed() < timeout {
        if bare_shell_prompt_visible(svc, pty_id) {
            return true;
        }
        tokio::time::sleep(poll).await;
    }
    bare_shell_prompt_visible(svc, pty_id)
}

fn bare_shell_prompt_visible(svc: &ServiceContext, pty_id: &str) -> bool {
    let text = svc
        .screen_state
        .lock()
        .get_screen_text(pty_id)
        .unwrap_or_default();
    // Shell prompt must be present …
    if !crate::pty::screen_state::screen_has_shell_prompt(&text) {
        return false;
    }
    // … with no universal TUI glyphs (box-drawing, block chars) …
    if crate::pty::screen_state::screen_has_agent_tui(&text) {
        return false;
    }
    // … and no provider-specific TUI text patterns.
    !svc.agent_registry.any_provider_screen_has_tui(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_tracker() -> RestartTracker {
        new_restart_tracker()
    }

    fn check(tracker: &RestartTracker, pane_id: &str) -> Result<(), AgentRestartError> {
        let mut guard = tracker.lock();
        let now = Instant::now();
        if let Some(last) = guard.get(pane_id) {
            let elapsed = now.duration_since(*last);
            let limit = Duration::from_secs(RESTART_RATE_LIMIT_SECS);
            if elapsed < limit {
                let retry = (limit - elapsed).as_secs().max(1);
                return Err(AgentRestartError::RateLimited {
                    retry_after_secs: retry,
                });
            }
        }
        guard.insert(pane_id.to_string(), now);
        Ok(())
    }

    #[test]
    fn rate_limit_first_call_succeeds() {
        let t = fresh_tracker();
        assert!(check(&t, "pane-1").is_ok());
    }

    #[test]
    fn rate_limit_second_call_blocked() {
        let t = fresh_tracker();
        check(&t, "pane-1").unwrap();
        match check(&t, "pane-1") {
            Err(AgentRestartError::RateLimited { retry_after_secs }) => {
                assert!(retry_after_secs > 0);
                assert!(retry_after_secs <= RESTART_RATE_LIMIT_SECS);
            }
            other => panic!("expected RateLimited, got {:?}", other),
        }
    }

    #[test]
    fn rate_limit_independent_panes() {
        let t = fresh_tracker();
        check(&t, "pane-a").unwrap();
        assert!(check(&t, "pane-b").is_ok());
    }

    #[test]
    fn rate_limit_resets_after_window() {
        let t = fresh_tracker();
        let past = Instant::now() - Duration::from_secs(RESTART_RATE_LIMIT_SECS + 1);
        t.lock().insert("pane-1".to_string(), past);
        assert!(check(&t, "pane-1").is_ok());
    }

    fn registry_with_claude() -> crate::agent::registry::AgentRegistry {
        let reg = crate::agent::registry::AgentRegistry::new();
        reg.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
        reg
    }

    #[test]
    fn build_relaunch_falls_back_to_fresh_when_session_stale() {
        let reg = registry_with_claude();
        let cmd = build_relaunch_command(&reg, "claude", Some("ghost-id"), true, false);
        let cmd = cmd.expect("fallback launch command");
        assert!(
            !cmd.contains("--resume"),
            "stale-session fallback must not include --resume; got: {}",
            cmd
        );
        assert!(
            cmd.contains("claude"),
            "expected claude in launch cmd; got: {}",
            cmd
        );
    }

    #[test]
    fn build_relaunch_uses_resume_when_session_present() {
        let reg = registry_with_claude();
        let cmd = build_relaunch_command(&reg, "claude", Some("abc-123"), true, true)
            .expect("resume command");
        assert!(
            cmd.contains("--resume"),
            "resume cmd should contain --resume; got: {}",
            cmd
        );
        assert!(
            cmd.contains("abc-123"),
            "resume cmd should embed session id; got: {}",
            cmd
        );
    }

    #[test]
    fn build_relaunch_unknown_provider_returns_none() {
        let reg = registry_with_claude();
        assert!(build_relaunch_command(&reg, "nonsuch", None, true, false).is_none());
    }

    #[test]
    fn build_relaunch_drops_resume_without_session_id() {
        let reg = registry_with_claude();
        let cmd = build_relaunch_command(&reg, "claude", None, true, true)
            .expect("fresh launch when no session id");
        assert!(
            !cmd.contains("--resume"),
            "no session id must mean no --resume; got: {}",
            cmd
        );
    }
}

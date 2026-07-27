//! Agent restart: graceful /exit + auto-resume, or force-kill PTY + relaunch.
//!
//! Wraps the manual /exit + resume dance documented in AGENT.md as a single
//! atomic primitive callable from the CLI (`codeterm agent restart`) and from
//! the self-update flow (`feat/auto-update`) which restarts every agent tab
//! around binary swap.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::ServiceContext;
use crate::agent::provider::{AgentLaunchParams, StructuredPromptServer};

/// Minimum seconds between consecutive restarts for a single tab.
pub const RESTART_RATE_LIMIT_SECS: u64 = 30;

/// Default grace period before force-killing the agent process.
pub const DEFAULT_GRACE_SECS: u64 = 10;

/// Tracks the last successful restart per tab for rate limiting.
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
    pub tab_id: String,
    pub provider: String,
    pub session_id: Option<String>,
    pub session_was_stale: bool,
    pub force: bool,
    pub auto_approve: bool,
}

impl RestartOutcome {
    /// Explicit session contract: resume the recorded session, or start fresh.
    pub fn session_action(&self) -> &'static str {
        match (&self.session_id, self.session_was_stale) {
            (Some(_), false) => "resume",
            _ => "fresh",
        }
    }

    /// Permission/sandbox profile the relaunch preserves from the tab.
    pub fn permission_profile(&self) -> &'static str {
        if self.auto_approve {
            "yolo"
        } else {
            "sandboxed"
        }
    }
}

/// Snapshot of the bits of tab state we need to drive a restart.
/// Captured once under the mux lock so the async task does not need to
/// re-acquire it just to read fields.
#[derive(Debug, Clone)]
struct TabRestartSnapshot {
    tab_id: String,
    pty_id: String,
    provider: String,
    session_id: Option<String>,
    cwd: Option<String>,
    auto_approve: bool,
}

impl ServiceContext {
    /// Atomic agent restart. Validates tab + rate limit synchronously,
    /// then spawns a tokio task that drives the actual /exit → wait → resume
    /// sequence. Returns immediately with 202-style outcome.
    pub fn agent_restart(
        &self,
        tab_id: &str,
        force: bool,
        grace_secs: u64,
    ) -> Result<RestartOutcome, AgentRestartError> {
        if let Some(outcome) = self.recover_general_agent_tab(tab_id, force)? {
            return Ok(outcome);
        }
        let snap = self.snapshot_tab_for_restart(tab_id)?;

        self.check_and_record_restart(&snap.tab_id)?;

        let session_was_stale = !self.session_file_present(&snap);

        let outcome = RestartOutcome {
            tab_id: snap.tab_id.clone(),
            provider: snap.provider.clone(),
            session_id: snap.session_id.clone(),
            session_was_stale,
            force,
            auto_approve: snap.auto_approve,
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

    /// The protected General `Main` tab below `AgentReady` has no live agent to
    /// `/exit`, so the generic restart path can only report "no agent in this
    /// Tab". Route it to the guarded General Agent relaunch instead — the one
    /// recovery that rebuilds the PTY. Rate limiting still applies, and the
    /// relaunch claim is what prevents a second launch, so this cannot storm.
    fn recover_general_agent_tab(
        &self,
        tab_id: &str,
        force: bool,
    ) -> Result<Option<RestartOutcome>, AgentRestartError> {
        let Some(target) = self.general_tab_awaiting_agent(tab_id) else {
            return Ok(None);
        };
        self.check_and_record_restart(&target)?;

        let mux = self.mux.clone();
        let claimed = crate::service::general_agent::claim_general_agent_tab_retry(&target, &mux)
            .map_err(AgentRestartError::BadRequest)?;
        if !claimed {
            return Err(AgentRestartError::BadRequest(
                "General Agent launch already in progress".to_string(),
            ));
        }

        let (provider, session_id) = {
            let m = self.mux.read();
            let tab = m.get_tab(&target);
            (
                tab.and_then(|p| p.agent_type.clone()),
                tab.and_then(|p| p.agent_session_id.clone()),
            )
        };

        let svc = Arc::new(self.clone());
        let pty = self.pty.clone();
        let screen_state = self.screen_state.clone();
        let app = self.app_handle.clone();
        let config = {
            use tauri::Manager;
            self.app_handle
                .state::<crate::config::ConfigState>()
                .inner()
                .clone()
        };
        let provider =
            provider.unwrap_or_else(|| config.lock().general_agent.agent_provider.clone());
        std::thread::spawn(move || {
            crate::service::general_agent::restart_general_agent(
                &svc,
                &mux,
                &pty,
                &screen_state,
                &config,
                &app,
            );
        });

        log::info!(
            "[agent_restart] Tab {} recovered via General Agent relaunch",
            target
        );
        Ok(Some(RestartOutcome {
            tab_id: target,
            provider,
            session_id,
            session_was_stale: false,
            force,
            auto_approve: true,
        }))
    }

    /// `Some(tab_id)` when this request targets the protected General tab and
    /// that tab has not reached `AgentReady` — the stuck state a plain restart
    /// cannot address.
    fn general_tab_awaiting_agent(&self, tab_id: &str) -> Option<String> {
        use codeterm_types::pty::BootStage;
        let m = self.mux.read();
        let general = m.resolve_tab_id(crate::service::general_agent::GENERAL_ALIAS)?;
        if m.resolve_tab_id(tab_id)? != general {
            return None;
        }
        let tab = m.get_tab(&general)?;
        (tab.protected && tab.boot_stage != Some(BootStage::AgentReady)).then_some(general)
    }

    fn snapshot_tab_for_restart(
        &self,
        tab_id: &str,
    ) -> Result<TabRestartSnapshot, AgentRestartError> {
        let mux = self.mux.write();
        let resolved = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", tab_id)))?;
        let tab = mux
            .get_tab(&resolved)
            .ok_or_else(|| AgentRestartError::NotFound(format!("Tab {} not found", tab_id)))?;

        let provider = tab
            .agent_type
            .clone()
            .ok_or_else(|| AgentRestartError::BadRequest("no agent in this Tab".to_string()))?;
        let pty_id = tab
            .pty_id
            .clone()
            .ok_or_else(|| AgentRestartError::BadRequest("Tab has no PTY".to_string()))?;

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

        Ok(TabRestartSnapshot {
            tab_id: resolved,
            pty_id,
            provider,
            session_id: tab.agent_session_id.clone(),
            cwd: tab.cwd.clone(),
            auto_approve: tab.auto_approve,
        })
    }

    fn check_and_record_restart(&self, tab_id: &str) -> Result<(), AgentRestartError> {
        let mut tracker = self.restart_tracker.lock();
        let now = Instant::now();
        if let Some(last) = tracker.get(tab_id) {
            let elapsed = now.duration_since(*last);
            let limit = Duration::from_secs(RESTART_RATE_LIMIT_SECS);
            if elapsed < limit {
                let retry = (limit - elapsed).as_secs().max(1);
                return Err(AgentRestartError::RateLimited {
                    retry_after_secs: retry,
                });
            }
        }
        tracker.insert(tab_id.to_string(), now);
        Ok(())
    }

    fn session_file_present(&self, snap: &TabRestartSnapshot) -> bool {
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
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    session_present: bool,
) -> Option<String> {
    let provider = &snap.provider;
    let auto_approve = snap.auto_approve;
    let session_id = snap.session_id.as_deref();
    let cwd = snap.cwd.as_deref().unwrap_or_default();
    let structured_prompt_server = svc
        .agent_registry
        .with_provider(provider, |p| p.needs_structured_prompt_server())
        .unwrap_or(false)
        .then(|| {
            crate::service::structured_prompt::ensure_for_spawn(svc, provider, &snap.tab_id, cwd)
        })
        .flatten();
    build_relaunch_command_with_registry(
        &svc.agent_registry,
        provider,
        session_id,
        auto_approve,
        session_present,
        structured_prompt_server,
    )
}

fn build_relaunch_command_with_registry(
    registry: &crate::agent::registry::AgentRegistry,
    provider: &str,
    session_id: Option<&str>,
    auto_approve: bool,
    session_present: bool,
    structured_prompt_server: Option<StructuredPromptServer>,
) -> Option<String> {
    if session_present && structured_prompt_server.is_some() {
        if let Some(sid) = session_id {
            return registry.with_provider(provider, |p| {
                p.build_launch_command(&AgentLaunchParams {
                    agent_name: None,
                    task: None,
                    starter_prompt: Default::default(),
                    args: None,
                    skip_permissions: auto_approve,
                    session_id: Some(sid.to_string()),
                    structured_prompt_server: structured_prompt_server.clone(),
                })
            });
        }
    }
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
            starter_prompt: Default::default(),
            args: None,
            skip_permissions: auto_approve,
            session_id: None,
            structured_prompt_server,
        })
    })
}

async fn drive_restart(
    svc: ServiceContext,
    snap: TabRestartSnapshot,
    force: bool,
    grace: Duration,
) {
    let session_present = svc.session_file_present(&snap);
    if !session_present && snap.session_id.is_some() {
        log::warn!(
            "[agent_restart] tab={} session_id={:?} not found on disk — will start fresh",
            snap.tab_id,
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
                "[agent_restart] tab={} sending {} (graceful)",
                snap.tab_id,
                exit_cmd
            );
            if let Err(e) = svc.send_text(&snap.tab_id, &exit_cmd, true) {
                log::warn!(
                    "[agent_restart] graceful exit send failed for tab={}: {}",
                    snap.tab_id,
                    e
                );
            }
        } else {
            log::info!(
                "[agent_restart] tab={} provider={} has no exit_command — falling through to grace wait",
                snap.tab_id,
                snap.provider
            );
        }
    }

    let shell_ready = wait_for_shell_after_exit(&svc, &snap, force, grace).await;
    if !shell_ready {
        log::error!(
            "[agent_restart] tab={} agent TUI still on screen after exit + Ctrl-C escalation — \
             aborting relaunch to avoid piping launch command into the TUI's input box. \
             User can re-run restart with force=true or close the Tab manually.",
            snap.tab_id
        );
        crate::logs::record(
            crate::logs::LogLevel::Warn,
            "agent.restart",
            format!(
                "restart aborted on Tab {} — agent did not exit cleanly; re-run with force=true",
                snap.tab_id
            ),
            Some(serde_json::json!({
                "tab_id": snap.tab_id,
                "provider": snap.provider,
                "reason": "tui_still_visible",
            })),
        );
        return;
    }
    svc.clear_tab_agent_identity_for_lifecycle_exit(&snap.tab_id);

    let Some(launch_cmd) = build_relaunch_command(&svc, &snap, session_present)
        .map(|cmd| crate::agent::stub_launch::apply(&snap.provider, cmd))
    else {
        log::error!(
            "[agent_restart] tab={} provider={} returned no launch command — restart aborted",
            snap.tab_id,
            snap.provider
        );
        return;
    };

    log::info!(
        "[agent_restart] tab={} launching: {}",
        snap.tab_id,
        launch_cmd
    );
    if let Err(e) = svc.send_text(&snap.tab_id, &launch_cmd, true) {
        log::error!(
            "[agent_restart] tab={} relaunch send failed: {}",
            snap.tab_id,
            e
        );
    }
}

async fn wait_for_shell_after_exit(
    svc: &ServiceContext,
    snap: &TabRestartSnapshot,
    force: bool,
    grace: Duration,
) -> bool {
    if force {
        log::info!(
            "[agent_restart] tab={} force=true — sending Ctrl-C and waiting briefly",
            snap.tab_id
        );
        let _ = svc.write_raw(&snap.tab_id, b"\x03");
        tokio::time::sleep(Duration::from_millis(300)).await;
        let _ = svc.write_raw(&snap.tab_id, b"\x03");
        return poll_bare_shell_prompt(svc, &snap.pty_id, Duration::from_secs(3)).await;
    }

    if poll_bare_shell_prompt(svc, &snap.pty_id, grace).await {
        return true;
    }

    log::warn!(
        "[agent_restart] tab={} graceful exit timed out — escalating to Ctrl-C",
        snap.tab_id
    );
    let _ = svc.write_raw(&snap.tab_id, b"\x03");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let _ = svc.write_raw(&snap.tab_id, b"\x03");
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
    // Shell prompt must be present, and no agent TUI may be on screen.
    crate::pty::screen_state::screen_has_shell_prompt(&text)
        && !screen_shows_agent_tui(&svc.agent_registry, &text)
}

pub(crate) fn screen_shows_agent_tui(
    registry: &crate::agent::registry::AgentRegistry,
    screen_text: &str,
) -> bool {
    crate::pty::screen_state::screen_has_agent_tui(screen_text)
        || registry.any_provider_screen_has_tui(screen_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_tracker() -> RestartTracker {
        new_restart_tracker()
    }

    fn check(tracker: &RestartTracker, tab_id: &str) -> Result<(), AgentRestartError> {
        let mut guard = tracker.lock();
        let now = Instant::now();
        if let Some(last) = guard.get(tab_id) {
            let elapsed = now.duration_since(*last);
            let limit = Duration::from_secs(RESTART_RATE_LIMIT_SECS);
            if elapsed < limit {
                let retry = (limit - elapsed).as_secs().max(1);
                return Err(AgentRestartError::RateLimited {
                    retry_after_secs: retry,
                });
            }
        }
        guard.insert(tab_id.to_string(), now);
        Ok(())
    }

    #[test]
    fn rate_limit_first_call_succeeds() {
        let t = fresh_tracker();
        assert!(check(&t, "tab-1").is_ok());
    }

    #[test]
    fn rate_limit_second_call_blocked() {
        let t = fresh_tracker();
        check(&t, "tab-1").unwrap();
        match check(&t, "tab-1") {
            Err(AgentRestartError::RateLimited { retry_after_secs }) => {
                assert!(retry_after_secs > 0);
                assert!(retry_after_secs <= RESTART_RATE_LIMIT_SECS);
            }
            other => panic!("expected RateLimited, got {:?}", other),
        }
    }

    #[test]
    fn rate_limit_independent_tabs() {
        let t = fresh_tracker();
        check(&t, "tab-a").unwrap();
        assert!(check(&t, "tab-b").is_ok());
    }

    #[test]
    fn rate_limit_resets_after_window() {
        let t = fresh_tracker();
        let past = Instant::now() - Duration::from_secs(RESTART_RATE_LIMIT_SECS + 1);
        t.lock().insert("tab-1".to_string(), past);
        assert!(check(&t, "tab-1").is_ok());
    }

    fn registry_with_claude() -> crate::agent::registry::AgentRegistry {
        let reg = crate::agent::registry::AgentRegistry::new();
        reg.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
        reg
    }

    #[test]
    fn build_relaunch_falls_back_to_fresh_when_session_stale() {
        let reg = registry_with_claude();
        let cmd = build_relaunch_command_with_registry(
            &reg,
            "claude",
            Some("ghost-id"),
            true,
            false,
            None,
        );
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
        let cmd =
            build_relaunch_command_with_registry(&reg, "claude", Some("abc-123"), true, true, None)
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
        assert!(
            build_relaunch_command_with_registry(&reg, "nonsuch", None, true, false, None)
                .is_none()
        );
    }

    #[test]
    fn screen_shows_agent_tui_catches_provider_text_fingerprint() {
        let reg = registry_with_claude();
        let footer = "some assistant output\n⏵⏵ bypass permissions on (shift+tab to cycle)\n❯ ";
        assert!(
            !crate::pty::screen_state::screen_has_agent_tui(footer),
            "footer-only frame has no universal glyphs — the gap the guard closes"
        );
        assert!(
            crate::pty::screen_state::screen_has_shell_prompt(footer),
            "`❯` still looks shell-ish to the prompt matcher"
        );
        assert!(
            screen_shows_agent_tui(&reg, footer),
            "provider fingerprint must mark this as an active agent TUI"
        );
    }

    #[test]
    fn screen_shows_agent_tui_catches_universal_glyphs() {
        let reg = registry_with_claude();
        let boxed = "╭──────────────────────────╮\n│ ❯                        │\n╰──────────────────────────╯";
        assert!(screen_shows_agent_tui(&reg, boxed));
    }

    #[test]
    fn screen_shows_agent_tui_false_for_bare_shell() {
        let reg = registry_with_claude();
        assert!(!screen_shows_agent_tui(&reg, "user@host:~/codeterm$ "));
        assert!(!screen_shows_agent_tui(&reg, "~/code on  master\n❯ "));
    }

    fn outcome(session_id: Option<&str>, stale: bool, auto_approve: bool) -> RestartOutcome {
        RestartOutcome {
            tab_id: "p1".to_string(),
            provider: "codex".to_string(),
            session_id: session_id.map(String::from),
            session_was_stale: stale,
            force: true,
            auto_approve,
        }
    }

    #[test]
    fn session_action_resume_when_present_and_fresh() {
        assert_eq!(outcome(Some("sid"), false, true).session_action(), "resume");
    }

    #[test]
    fn session_action_fresh_when_stale_or_missing() {
        assert_eq!(outcome(Some("sid"), true, true).session_action(), "fresh");
        assert_eq!(outcome(None, false, true).session_action(), "fresh");
    }

    #[test]
    fn permission_profile_reflects_auto_approve() {
        assert_eq!(outcome(None, false, true).permission_profile(), "yolo");
        assert_eq!(
            outcome(None, false, false).permission_profile(),
            "sandboxed"
        );
    }

    #[test]
    fn build_relaunch_preserves_yolo_profile_on_resume_and_fresh() {
        let reg = registry_with_claude();
        // auto_approve=true must keep the skip flag on BOTH the resume and fresh paths.
        let resume =
            build_relaunch_command_with_registry(&reg, "claude", Some("sid"), true, true, None)
                .expect("resume cmd");
        assert!(resume.contains("--dangerously-skip-permissions"));
        let fresh =
            build_relaunch_command_with_registry(&reg, "claude", Some("sid"), true, false, None)
                .expect("fresh cmd");
        assert!(fresh.contains("--dangerously-skip-permissions"));
        // auto_approve=false must NOT leak the skip flag — sandboxed profile preserved.
        let sandboxed =
            build_relaunch_command_with_registry(&reg, "claude", Some("sid"), false, true, None)
                .expect("sandboxed resume cmd");
        assert!(!sandboxed.contains("--dangerously-skip-permissions"));
    }

    #[test]
    fn build_relaunch_drops_resume_without_session_id() {
        let reg = registry_with_claude();
        let cmd = build_relaunch_command_with_registry(&reg, "claude", None, true, true, None)
            .expect("fresh launch when no session id");
        assert!(
            !cmd.contains("--resume"),
            "no session id must mean no --resume; got: {}",
            cmd
        );
    }
}

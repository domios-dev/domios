use std::time::Duration;

use tauri::Manager;
use tokio::task::JoinSet;

use crate::service::ServiceState;
use crate::session::SessionState;

const PER_PANE_GRACE: Duration = Duration::from_secs(10);
const SIGTERM_GRACE: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Stable fallback exit-command per known provider id.
/// Used only when the registered provider's `exit_command()` returns None
/// (test/stub providers, plugin loading order quirks). Real providers
/// expose this via the trait — see `plugin/agent_claude::provider` and
/// `plugin/agent_codex::provider`.
fn provider_exit_command(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "claude" | "codex" => Some("/exit"),
        _ => None,
    }
}

/// Step 2-9 from `docs/plans/auto-update.md`. Step 1 (user confirmation) and
/// step 6 (binary swap, handled by `tauri-plugin-updater` before this runs)
/// happen outside. Steps 7-9 (relaunch + session restore + per-pane resume)
/// happen in the NEW process via `lib.rs::init_states` (snapshot reload) and
/// `commands::spawn_pty` (auto-resume by `agent_session_id`).
pub async fn graceful_shutdown(app: &tauri::AppHandle) -> Result<(), String> {
    log::info!("[updater] graceful_shutdown: starting");

    let svc = app
        .try_state::<ServiceState>()
        .ok_or_else(|| "ServiceState not registered".to_string())?
        .inner()
        .clone();

    let agent_panes = collect_agent_panes(&svc);
    log::info!(
        "[updater] graceful_shutdown: {} agent pane(s) need /exit",
        agent_panes.len()
    );

    send_exit_to_agents(&svc, &agent_panes);

    wait_or_kill_panes(svc.pty.clone(), agent_panes).await;

    persist_session(app, &svc)?;

    if let Err(e) = svc.mem.checkpoint_and_close() {
        log::warn!("[updater] mem WAL checkpoint failed: {}", e);
    }

    log::info!("[updater] graceful_shutdown: complete");
    Ok(())
}

#[derive(Clone)]
struct AgentPaneInfo {
    pane_id: String,
    pty_id: String,
    provider_id: String,
    exit_command: Option<String>,
}

fn collect_agent_panes(svc: &ServiceState) -> Vec<AgentPaneInfo> {
    let snapshot = svc.mux.read().snapshot();
    let mut out = Vec::new();
    for (_, pane) in snapshot.panes.iter() {
        let (Some(provider_id), Some(pty_id)) = (pane.agent_type.as_ref(), pane.pty_id.as_ref())
        else {
            continue;
        };
        let exit_command = svc
            .agent_registry
            .with_provider(provider_id, |p| p.exit_command().map(|s| s.to_string()))
            .flatten()
            .or_else(|| provider_exit_command(provider_id).map(|s| s.to_string()));
        out.push(AgentPaneInfo {
            pane_id: pane.id.clone(),
            pty_id: pty_id.clone(),
            provider_id: provider_id.clone(),
            exit_command,
        });
    }
    out
}

fn send_exit_to_agents(svc: &ServiceState, agents: &[AgentPaneInfo]) {
    for agent in agents {
        let Some(cmd) = agent.exit_command.as_deref() else {
            log::warn!(
                "[updater] no exit command for provider '{}' on pane {}; will skip /exit and rely on signal",
                agent.provider_id, agent.pane_id
            );
            continue;
        };
        let payload = format!("{}\r", cmd);
        let pty = svc.pty.lock();
        match pty.write(&agent.pty_id, payload.as_bytes()) {
            Ok(()) => log::info!(
                "[updater] sent {:?} to pane {} (provider={})",
                cmd,
                agent.pane_id,
                agent.provider_id
            ),
            Err(e) => log::warn!(
                "[updater] failed to send exit to pane {}: {}",
                agent.pane_id,
                e
            ),
        }
    }
}

async fn wait_or_kill_panes(pty: crate::PtyState, agents: Vec<AgentPaneInfo>) {
    if agents.is_empty() {
        return;
    }
    let mut set: JoinSet<()> = JoinSet::new();
    for agent in agents {
        let pty = pty.clone();
        set.spawn(async move { wait_or_kill_one(pty, agent).await });
    }
    while set.join_next().await.is_some() {}
}

async fn wait_or_kill_one(pty: crate::PtyState, agent: AgentPaneInfo) {
    if poll_until_exit(&pty, &agent.pty_id, PER_PANE_GRACE).await {
        log::info!(
            "[updater] pane {} exited gracefully (provider={})",
            agent.pane_id,
            agent.provider_id
        );
        let _ = pty.lock().kill(&agent.pty_id);
        return;
    }

    log::warn!(
        "[updater] pane {} did not exit within {:?}; sending SIGTERM via Child::kill()",
        agent.pane_id,
        PER_PANE_GRACE
    );
    if let Err(e) = soft_kill(&pty, &agent.pty_id) {
        log::warn!(
            "[updater] soft_kill failed for pane {}: {}",
            agent.pane_id,
            e
        );
    }

    if poll_until_exit(&pty, &agent.pty_id, SIGTERM_GRACE).await {
        log::info!("[updater] pane {} exited after SIGTERM", agent.pane_id);
        let _ = pty.lock().kill(&agent.pty_id);
        return;
    }

    log::warn!(
        "[updater] pane {} still alive after SIGTERM; dropping PTY (forces SIGKILL)",
        agent.pane_id
    );
    let _ = pty.lock().kill(&agent.pty_id);
}

async fn poll_until_exit(pty: &crate::PtyState, pty_id: &str, budget: Duration) -> bool {
    let deadline = std::time::Instant::now() + budget;
    loop {
        if !is_alive(pty, pty_id) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

fn is_alive(pty: &crate::PtyState, pty_id: &str) -> bool {
    let manager = pty.lock();
    let Some(session) = manager.get_session(pty_id) else {
        return false;
    };
    let mut child = match session.child.lock() {
        Ok(g) => g,
        Err(_) => return true,
    };
    matches!(child.try_wait(), Ok(None))
}

fn soft_kill(pty: &crate::PtyState, pty_id: &str) -> Result<(), String> {
    let manager = pty.lock();
    let session = manager
        .get_session(pty_id)
        .ok_or_else(|| format!("PTY {} not found", pty_id))?;
    drop(manager);
    let mut child = session
        .child
        .lock()
        .map_err(|e| format!("child lock poisoned: {}", e))?;
    child.kill().map_err(|e| format!("Child::kill: {}", e))
}

pub(crate) fn persist_session(app: &tauri::AppHandle, svc: &ServiceState) -> Result<(), String> {
    let session = app
        .try_state::<SessionState>()
        .ok_or_else(|| "SessionState not registered".to_string())?
        .inner()
        .clone();
    let snapshot = svc.mux.read().to_session_snapshot();
    session
        .lock()
        .save(&snapshot)
        .map_err(|e| format!("save session: {}", e))?;
    log::info!(
        "[updater] session snapshot saved ({} workspaces)",
        snapshot.workspaces.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_exit_command_known_providers() {
        assert_eq!(provider_exit_command("claude"), Some("/exit"));
        assert_eq!(provider_exit_command("codex"), Some("/exit"));
    }

    #[test]
    fn provider_exit_command_unknown_returns_none() {
        assert_eq!(provider_exit_command("aider"), None);
        assert_eq!(provider_exit_command(""), None);
        assert_eq!(provider_exit_command("Claude"), None);
    }

    #[test]
    fn registered_providers_match_fallback_mapping() {
        // Sanity: the trait-level exit_command for the providers we ship must agree with our compile-time fallback. If a provider rewords /exit to
        // something else, both this test and the fallback must change together.
        use crate::agent::provider::AgentProvider;
        use crate::plugin::agent_claude::ClaudeProvider;

        let claude = ClaudeProvider::new();
        assert_eq!(claude.exit_command(), Some("/exit"));
        assert_eq!(provider_exit_command(claude.id()), claude.exit_command());

        // codex/opencode are JS plugins now; their /exit fallback is asserted in
        // their plugin-side tests (plugins/provider-*/tests).
    }

    #[test]
    fn mem_checkpoint_returns_pragma_row() {
        let mem = crate::mem::MemManager::open_in_memory().unwrap();
        let (busy, log_frames, checkpointed) = mem
            .checkpoint_and_close()
            .expect("checkpoint should succeed on a fresh in-memory db");
        // In-memory dbs don't use WAL, so log_frames/checkpointed are -1 per SQLite docs ("PRAGMA wal_checkpoint returns -1 columns when not in WAL
        // mode"). We only assert the call path is wired.
        assert!(busy >= -1);
        assert!(log_frames >= -1);
        assert!(checkpointed >= -1);
    }
}

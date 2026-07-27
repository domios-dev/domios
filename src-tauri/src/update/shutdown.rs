use std::time::Duration;

use tauri::Manager;
use tokio::task::JoinSet;

use crate::service::ServiceState;

const PER_PANE_GRACE: Duration = Duration::from_secs(10);
const FORCED_EXIT_GRACE: Duration = Duration::from_secs(5);
const SPAWN_QUIESCE_TIMEOUT: Duration = Duration::from_secs(10);
const PTY_REAP_TIMEOUT: Duration = Duration::from_secs(10);

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

    // Stop the liveness watchdog treating the imminent API-down as a crash.
    crate::crash::shutdown::mark_shutting_down_for(crate::crash::lifecycle::ShutdownReason::Update);

    let svc = app
        .try_state::<ServiceState>()
        .ok_or_else(|| "ServiceState not registered".to_string())?
        .inner()
        .clone();

    quiesce_pty_spawns(&svc).await?;

    let agent_panes = collect_agent_panes(&svc);
    log::info!(
        "[updater] graceful_shutdown: {} agent Tab(s) need /exit",
        agent_panes.len()
    );

    send_exit_to_agents(&svc, &agent_panes);

    wait_or_kill_panes(svc.pty.clone(), agent_panes).await?;
    let reaped = reap_owned_ptys(&svc).await?;
    let conpty_hosts = super::conpty_cleanup::cleanup_before_installer().await?;

    persist_session(app, &svc)?;

    if let Err(e) = svc.mem.checkpoint_and_close() {
        log::warn!("[updater] mem WAL checkpoint failed: {}", e);
    }

    log::info!(
        "[updater] graceful_shutdown: complete; {} PTY session(s) reaped, {} bundled ConPTY host(s) reaped",
        reaped,
        conpty_hosts
    );
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
                "[updater] no exit command for provider '{}' on Tab {}; will skip /exit and rely on signal",
                agent.provider_id, agent.pane_id
            );
            continue;
        };
        let payload = format!("{}\r", cmd);
        let pty = svc.pty.lock();
        match pty.write(&agent.pty_id, payload.as_bytes()) {
            Ok(()) => log::info!(
                "[updater] sent {:?} to Tab {} (provider={})",
                cmd,
                agent.pane_id,
                agent.provider_id
            ),
            Err(e) => log::warn!(
                "[updater] failed to send exit to Tab {}: {}",
                agent.pane_id,
                e
            ),
        }
    }
}

async fn quiesce_pty_spawns(svc: &ServiceState) -> Result<(), String> {
    let spawn_gate = svc.pty.lock().spawn_gate();
    spawn_gate.quiesce();
    tokio::task::spawn_blocking(move || {
        spawn_gate.wait_for_idle(std::time::Instant::now() + SPAWN_QUIESCE_TIMEOUT)
    })
    .await
    .map_err(|error| format!("PTY spawn quiesce task failed: {error}"))?
}

async fn wait_or_kill_panes(
    pty: crate::PtyState,
    agents: Vec<AgentPaneInfo>,
) -> Result<(), String> {
    if agents.is_empty() {
        return Ok(());
    }
    let mut set: JoinSet<Result<(), String>> = JoinSet::new();
    for agent in agents {
        let pty = pty.clone();
        set.spawn(async move { wait_or_kill_one(pty, agent).await });
    }
    let mut failures = Vec::new();
    while let Some(result) = set.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => failures.push(error),
            Err(error) => failures.push(format!("agent shutdown task failed: {error}")),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

async fn wait_or_kill_one(pty: crate::PtyState, agent: AgentPaneInfo) -> Result<(), String> {
    let session = {
        let manager = pty.lock();
        manager.get_session(&agent.pty_id)
    };
    let Some(session) = session else {
        return Ok(());
    };
    let mut killer = session
        .child
        .lock()
        .map_err(|error| format!("Tab {} child lock poisoned: {error}", agent.pane_id))?
        .clone_killer();
    let waiting_session = session.clone();
    let mut waiter = tokio::task::spawn_blocking(move || {
        waiting_session
            .child
            .lock()
            .map_err(|error| format!("child lock poisoned: {error}"))?
            .wait()
            .map_err(|error| error.to_string())
    });

    if wait_for_child(&mut waiter, PER_PANE_GRACE).await? {
        log::info!(
            "[updater] Tab {} exited gracefully (provider={})",
            agent.pane_id,
            agent.provider_id
        );
        return Ok(());
    }

    log::warn!(
        "[updater] Tab {} did not exit within {:?}; terminating its owned process tree",
        agent.pane_id,
        PER_PANE_GRACE
    );
    if let Err(error) = killer.kill() {
        log::warn!(
            "[updater] process-tree termination failed for Tab {}: {}",
            agent.pane_id,
            error
        );
    }

    if wait_for_child(&mut waiter, FORCED_EXIT_GRACE).await? {
        log::info!(
            "[updater] Tab {} exited after forced termination",
            agent.pane_id
        );
        return Ok(());
    }

    Err(format!(
        "Tab {} process tree did not exit before shutdown deadline",
        agent.pane_id
    ))
}

async fn wait_for_child(
    waiter: &mut tokio::task::JoinHandle<Result<portable_pty::ExitStatus, String>>,
    timeout: Duration,
) -> Result<bool, String> {
    match tokio::time::timeout(timeout, waiter).await {
        Ok(result) => result
            .map_err(|error| format!("child wait task failed: {error}"))?
            .map(|_| true),
        Err(_) => Ok(false),
    }
}

async fn reap_owned_ptys(svc: &ServiceState) -> Result<usize, String> {
    let reapers = svc.pty.lock().begin_shutdown()?;
    let pty_ids = reapers
        .iter()
        .map(|reaper| reaper.pty_id().to_string())
        .collect::<Vec<_>>();
    svc.pty_ownership.release_ptys(&pty_ids);

    let count = reapers.len();
    let mut set = JoinSet::new();
    for reaper in reapers {
        set.spawn_blocking(move || reaper.wait_bounded(PTY_REAP_TIMEOUT));
    }
    let mut failures = Vec::new();
    while let Some(result) = set.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => failures.push(error),
            Err(error) => failures.push(format!("PTY reap task failed: {error}")),
        }
    }
    if failures.is_empty() {
        Ok(count)
    } else {
        Err(failures.join("; "))
    }
}

pub(crate) fn persist_session(app: &tauri::AppHandle, svc: &ServiceState) -> Result<(), String> {
    let result = crate::service::session_persist::write_session_snapshot(app, &svc.mux);
    if result.is_ok() {
        let snapshot = svc.mux.read().to_session_snapshot();
        log::info!(
            "[updater] session snapshot saved ({} workspaces)",
            snapshot.workspaces.len()
        );
    }
    result
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
        // something else, both this test and the fallback must change together.
        use crate::agent::provider::AgentProvider;
        use crate::plugin::agent_claude::ClaudeProvider;

        let claude = ClaudeProvider::new();
        assert_eq!(claude.exit_command(), Some("/exit"));
        assert_eq!(provider_exit_command(claude.id()), claude.exit_command());

        // their plugin-side tests (plugins/provider-*/tests).
    }

    #[test]
    fn mem_checkpoint_returns_pragma_row() {
        let mem = crate::mem::MemManager::open_in_memory().unwrap();
        let (busy, log_frames, checkpointed) = mem
            .checkpoint_and_close()
            .expect("checkpoint should succeed on a fresh in-memory db");
        // mode"). We only assert the call path is wired.
        assert!(busy >= -1);
        assert!(log_frames >= -1);
        assert!(checkpointed >= -1);
    }
}

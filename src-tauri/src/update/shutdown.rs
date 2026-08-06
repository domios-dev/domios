use std::time::Duration;

use tauri::Manager;
use tokio::task::JoinSet;

use crate::service::ServiceState;

const PER_TAB_GRACE: Duration = Duration::from_secs(10);
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
/// happen outside. Steps 7-9 (relaunch + session restore + per-tab resume)
/// happen in the NEW process via `lib.rs::init_states` (snapshot reload) and
/// `commands::spawn_pty` (auto-resume by `agent_session_id`).
pub async fn graceful_shutdown(
    app: &tauri::AppHandle,
    initiating_tab_id: Option<&str>,
) -> Result<Vec<String>, String> {
    log::info!("[updater] graceful_shutdown: starting");

    // Stop the liveness watchdog treating the imminent API-down as a crash.
    crate::crash::shutdown::mark_shutting_down_for(crate::crash::lifecycle::ShutdownReason::Update);

    let svc = app
        .try_state::<ServiceState>()
        .ok_or_else(|| "ServiceState not registered".to_string())?
        .inner()
        .clone();

    quiesce_pty_spawns(&svc).await?;

    let agent_tabs = collect_agent_tabs(&svc, initiating_tab_id);
    let swept: Vec<String> = agent_tabs.iter().map(|a| a.tab_id.clone()).collect();
    log::info!(
        "[updater] graceful_shutdown: {} agent Tab(s) need /exit (initiating Tab {:?} excluded)",
        agent_tabs.len(),
        initiating_tab_id
    );

    send_exit_to_agents(&svc, &agent_tabs);

    wait_or_kill_tabs(svc.pty.clone(), agent_tabs).await?;
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
    Ok(swept)
}

#[derive(Clone)]
struct AgentTabInfo {
    tab_id: String,
    pty_id: String,
    provider_id: String,
    exit_command: Option<String>,
}

fn collect_agent_tabs(svc: &ServiceState, initiating_tab_id: Option<&str>) -> Vec<AgentTabInfo> {
    let snapshot = svc.mux.read().snapshot();
    let tabs = snapshot.tabs.iter().map(|(_, tab)| {
        (
            tab.id.as_str(),
            tab.agent_type.as_deref(),
            tab.pty_id.as_deref(),
        )
    });
    select_agent_tabs(tabs, initiating_tab_id, |provider_id| {
        svc.agent_registry
            .with_provider(provider_id, |p| p.exit_command().map(|s| s.to_string()))
            .flatten()
            .or_else(|| provider_exit_command(provider_id).map(|s| s.to_string()))
    })
}

/// Pure selection seam. Every tab with an agent + PTY is swept EXCEPT the
/// initiating pane (Defect 3) — the caller that ran `update install` must
/// survive the sweep to hand off to the relaunching process.
fn select_agent_tabs<'a, R>(
    tabs: impl Iterator<Item = (&'a str, Option<&'a str>, Option<&'a str>)>,
    initiating_tab_id: Option<&str>,
    resolve_exit: R,
) -> Vec<AgentTabInfo>
where
    R: Fn(&str) -> Option<String>,
{
    let mut out = Vec::new();
    for (tab_id, agent_type, pty_id) in tabs {
        let (Some(provider_id), Some(pty_id)) = (agent_type, pty_id) else {
            continue;
        };
        if initiating_tab_id == Some(tab_id) {
            continue;
        }
        out.push(AgentTabInfo {
            tab_id: tab_id.to_string(),
            pty_id: pty_id.to_string(),
            provider_id: provider_id.to_string(),
            exit_command: resolve_exit(provider_id),
        });
    }
    out
}

fn send_exit_to_agents(svc: &ServiceState, agents: &[AgentTabInfo]) {
    dispatch_exit_submits(agents, |agent, cmd| {
        // Route /exit through the single canonical post-ready submit path
        // (composer-ready quiesce + paste + bounded submit-key burst) instead of
        // a raw `\r` write that only buffers into a mid-turn composer.
        svc.send_text_detached(&agent.tab_id, cmd);
    });
}

/// Pure dispatch seam. Issues one submit per agent that has an exit command,
/// passing the bare command (no trailing `\r` — the submit contract owns the
/// submit key). Agents without an exit command fall through to the signal sweep.
fn dispatch_exit_submits(agents: &[AgentTabInfo], mut submit: impl FnMut(&AgentTabInfo, &str)) {
    for agent in agents {
        let Some(cmd) = agent.exit_command.as_deref() else {
            log::warn!(
                "[updater] no exit command for provider '{}' on Tab {}; will skip /exit and rely on signal",
                agent.provider_id, agent.tab_id
            );
            continue;
        };
        log::info!(
            "[updater] submitting {:?} to Tab {} via provider submit contract (provider={})",
            cmd, agent.tab_id, agent.provider_id
        );
        submit(agent, cmd);
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

async fn wait_or_kill_tabs(pty: crate::PtyState, agents: Vec<AgentTabInfo>) -> Result<(), String> {
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

async fn wait_or_kill_one(pty: crate::PtyState, agent: AgentTabInfo) -> Result<(), String> {
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
        .map_err(|error| format!("Tab {} child lock poisoned: {error}", agent.tab_id))?
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

    if wait_for_child(&mut waiter, PER_TAB_GRACE).await? {
        log::info!(
            "[updater] Tab {} exited gracefully (provider={})",
            agent.tab_id,
            agent.provider_id
        );
        return Ok(());
    }

    log::warn!(
        "[updater] Tab {} did not exit within {:?}; terminating its owned process tree",
        agent.tab_id,
        PER_TAB_GRACE
    );
    if let Err(error) = killer.kill() {
        log::warn!(
            "[updater] process-tree termination failed for Tab {}: {}",
            agent.tab_id,
            error
        );
    }

    if wait_for_child(&mut waiter, FORCED_EXIT_GRACE).await? {
        log::info!(
            "[updater] Tab {} exited after forced termination",
            agent.tab_id
        );
        return Ok(());
    }

    Err(format!(
        "Tab {} process tree did not exit before shutdown deadline",
        agent.tab_id
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

    fn exit_for(provider: &str) -> Option<String> {
        provider_exit_command(provider).map(|s| s.to_string())
    }

    #[test]
    fn sweep_excludes_initiating_pane_keeps_others() {
        // The initiating GA pane ran `update install`; it must NOT be in the
        // kill set, while every other agent pane is.
        let tabs = vec![
            ("ga-tab", Some("claude"), Some("pty-ga")),
            ("worker-1", Some("codex"), Some("pty-1")),
            ("worker-2", Some("claude"), Some("pty-2")),
        ];
        let selected = select_agent_tabs(tabs.into_iter(), Some("ga-tab"), exit_for);
        let ids: Vec<&str> = selected.iter().map(|a| a.tab_id.as_str()).collect();
        assert!(!ids.contains(&"ga-tab"), "initiating pane must survive sweep");
        assert!(ids.contains(&"worker-1"));
        assert!(ids.contains(&"worker-2"));
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn sweep_without_initiating_id_includes_all_agent_panes() {
        let tabs = vec![
            ("ga-tab", Some("claude"), Some("pty-ga")),
            ("worker-1", Some("codex"), Some("pty-1")),
        ];
        let selected = select_agent_tabs(tabs.into_iter(), None, exit_for);
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn sweep_skips_tabs_without_agent_or_pty() {
        let tabs = vec![
            ("plain-shell", None, Some("pty-shell")),
            ("agent-no-pty", Some("claude"), None),
            ("real-agent", Some("claude"), Some("pty-a")),
        ];
        let selected = select_agent_tabs(tabs.into_iter(), Some("ga-tab"), exit_for);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].tab_id, "real-agent");
        assert_eq!(selected[0].exit_command.as_deref(), Some("/exit"));
    }

    fn agent(tab: &str, provider: &str, exit: Option<&str>) -> AgentTabInfo {
        AgentTabInfo {
            tab_id: tab.to_string(),
            pty_id: format!("pty-{tab}"),
            provider_id: provider.to_string(),
            exit_command: exit.map(|s| s.to_string()),
        }
    }

    #[test]
    fn exit_dispatch_submits_bare_command_not_a_raw_carriage_return_write() {
        let agents = vec![
            agent("t1", "claude", Some("/exit")),
            agent("t2", "codex", Some("/exit")),
        ];
        let mut calls: Vec<(String, String)> = Vec::new();
        dispatch_exit_submits(&agents, |a, cmd| calls.push((a.tab_id.clone(), cmd.to_string())));

        // One submit per agent, routed through the seam (the production wiring is
        // `send_text_detached` → the single canonical submit path), never a raw write.
        assert_eq!(
            calls,
            vec![
                ("t1".to_string(), "/exit".to_string()),
                ("t2".to_string(), "/exit".to_string()),
            ]
        );
        // The old raw-write signature was `"{cmd}\r"`; the submit contract owns the
        // submit key, so the dispatched payload must carry no carriage return.
        assert!(calls.iter().all(|(_, cmd)| !cmd.contains('\r')));
    }

    #[test]
    fn exit_dispatch_skips_agents_without_an_exit_command() {
        let agents = vec![
            agent("t1", "claude", Some("/exit")),
            agent("t2", "aider", None),
        ];
        let mut submitted: Vec<String> = Vec::new();
        dispatch_exit_submits(&agents, |a, _| submitted.push(a.tab_id.clone()));
        assert_eq!(submitted, vec!["t1".to_string()]);
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

    /// The realistic precondition for the ESRCH path: the agent honoured `/exit`,
    /// `wait_or_kill_one` already reaped it, and `reap_owned_ptys` kills it again.
    #[cfg(unix)]
    fn spawn_and_reap_child() -> (crate::PtyState, String) {
        use crate::pty::manager::PtyManager;
        use codeterm_util::lock_hold::TimedMutex;
        use std::collections::HashMap;
        use std::sync::Arc;

        let pty: crate::PtyState = Arc::new(TimedMutex::new("pty", PtyManager::new()));
        let (pty_id, session) = pty
            .lock()
            .spawn(
                Some("/usr/bin/true".to_string()),
                None,
                80,
                24,
                HashMap::new(),
            )
            .expect("spawn test PTY");
        session
            .child
            .lock()
            .expect("child lock")
            .wait()
            .expect("child exits on its own");
        (pty, pty_id)
    }

    #[cfg(unix)]
    #[test]
    fn reaped_child_kill_fails_with_esrch_and_not_eperm() {
        let (pty, pty_id) = spawn_and_reap_child();
        let session = pty.lock().get_session(&pty_id).expect("session registered");
        let error = session
            .child
            .lock()
            .expect("child lock")
            .kill()
            .expect_err("killing an already-reaped child fails at the OS layer");
        assert_eq!(
            error.raw_os_error(),
            Some(libc::ESRCH),
            "teardown abort mechanism is not ESRCH: {error:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_reap_succeeds_when_child_already_exited() {
        let (pty, pty_id) = spawn_and_reap_child();
        let reapers = pty.lock().begin_shutdown().expect("begin_shutdown");
        assert_eq!(reapers.len(), 1, "expected exactly the one test PTY");
        for reaper in reapers {
            if let Err(error) = reaper.wait_bounded(PTY_REAP_TIMEOUT) {
                assert!(
                    !error.contains("termination failed"),
                    "already-gone PTY {pty_id} aborted update teardown: {error}"
                );
                panic!("reap of an already-exited child failed for another reason: {error}");
            }
        }
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

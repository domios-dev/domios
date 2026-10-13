//! Start / restart / readiness-poll for the General Agent PTY+launch flow.

mod launch;
mod readiness;

use super::launch_files::general_agent_env_vars;
use super::{binding_wait, general_dir, instructions, launch_lock, state, GENERAL_ALIAS};
use codeterm_types::pty::BootStage;
use readiness::{await_agent_launch_evidence, await_agent_output};
#[cfg(test)]
use readiness::{await_agent_output_with_signal, ScreenChangeSubscription};

struct AutomaticLaunchClaim<'a> {
    mux: &'a crate::MuxState,
    tab_id: String,
}

impl<'a> AutomaticLaunchClaim<'a> {
    fn acquire(mux: &'a crate::MuxState, tab_id: &str) -> Result<Self, String> {
        let mut m = mux.write();
        let tab = m
            .get_tab_mut(tab_id)
            .ok_or_else(|| format!("General Agent Tab {tab_id} is missing"))?;
        apply_general_agent_launch_claim(tab);
        Ok(Self {
            mux,
            tab_id: tab_id.to_string(),
        })
    }
}

impl Drop for AutomaticLaunchClaim<'_> {
    fn drop(&mut self) {
        if let Some(tab) = self.mux.write().get_tab_mut(&self.tab_id) {
            tab.agent_booting = false;
        }
    }
}

fn apply_general_agent_launch_claim(tab: &mut crate::multiplexer::types::Tab) {
    tab.agent_booting = true;
    crate::service::boot_stage::apply_boot_stage(tab, BootStage::PtyStarting, None);
}

fn deliver_before_binding<T>(
    deliver: impl FnOnce() -> Result<(), String>,
    wait_for_binding: impl FnOnce() -> T,
) -> Result<T, String> {
    deliver()?;
    Ok(wait_for_binding())
}

/// The ledger reads the transaction's `InjectionResult` and nothing else — never a
/// return code, never the absence of an error.
fn launch_delivery_evidence(
    tab_id: &str,
    outcome: Result<
        crate::service::inject_verify::InjectionResult,
        crate::service::inject_verify::InjectionError,
    >,
) -> Result<bool, crate::service::delivery_error::DeliveryError> {
    outcome
        .map(|result| result.submit_observed)
        .map_err(|error| {
            crate::service::delivery_error::DeliveryError::from_injection(tab_id, error)
        })
}

/// Bytes that reached the PTY make the launch delivery uncertain, never failed:
/// the ledger keeps the pending evidence and the session binding resolves it.
/// Only a refusal that wrote nothing is a spawn failure.
fn launch_delivery_failure(
    tab_id: &str,
    outcome: Result<(), crate::service::delivery_error::DeliveryError>,
) -> Option<String> {
    let error = outcome.err()?;
    if error.surfaces_as_failure(crate::service::delivery_error::SendOrigin::Programmatic) {
        return Some(format!("{}: {error}", error.code()));
    }
    log::warn!(
        "[general-agent] launch delivery for Tab {tab_id} state={}; pending evidence retained",
        error.code()
    );
    None
}

fn deliver_confirmed_general_message(
    svc: &crate::ServiceState,
    tab_id: &str,
    pty_id: &str,
    provider_id: &str,
    text: &str,
) -> Result<(), String> {
    let evidence = launch_delivery_evidence(
        tab_id,
        svc.send_launch_text_optimistic(pty_id, provider_id, text, true),
    );
    let outcome = svc.reconcile_pending_delivery(tab_id, evidence);
    match launch_delivery_failure(tab_id, outcome) {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

fn general_agent_first_run_complete(
    mem: &crate::mem::MemManager,
    general_dir: &std::path::Path,
) -> bool {
    if mem.get("preference_identity").is_err() {
        return false;
    }
    std::fs::read_to_string(general_dir.join("identity.md"))
        .is_ok_and(|identity| !identity.trim().is_empty())
}

fn compose_startup_message(
    mem: &crate::mem::MemManager,
    bootstrap_path: &std::path::Path,
) -> Option<String> {
    if bootstrap_path
        .parent()
        .is_some_and(|general_dir| general_agent_first_run_complete(mem, general_dir))
    {
        return None;
    }
    std::fs::read_to_string(bootstrap_path).ok().map(|body| {
        crate::service::messenger::Messenger::from(
            crate::service::messenger::MessageEnvelope::codeterm_reminder("general-bootstrap"),
        )
        .wrap(&body)
    })
}

struct GeneralAgentLaunchInstructions {
    task: Option<String>,
    startup_message: Option<String>,
    starter_prompt: crate::agent::provider::StarterPrompt,
    prompt_delivery: &'static str,
}

fn compose_launch_instructions(
    system_prompt: String,
    startup_message: Option<String>,
    delivery: &crate::agent::provider::SystemPromptDelivery,
) -> GeneralAgentLaunchInstructions {
    use crate::agent::provider::{StarterPrompt, SystemPromptDelivery};

    let prompt_delivery = match delivery {
        SystemPromptDelivery::CliFlag(_) => "cli_flag",
        SystemPromptDelivery::SessionConfig => "session_config",
        SystemPromptDelivery::External => "confirmed_fallback",
    };
    match crate::service::launch_delivery::mode_for_descriptor(delivery) {
        crate::service::launch_delivery::LaunchDeliveryMode::SystemPrompt => {
            let first_turn = startup_message.is_some();
            let task = startup_message
                .map(|bootstrap| format!("{system_prompt}\n\n{bootstrap}"))
                .unwrap_or(system_prompt);
            GeneralAgentLaunchInstructions {
                task: Some(task),
                startup_message: None,
                starter_prompt: if first_turn {
                    StarterPrompt::AssignedTask
                } else {
                    StarterPrompt::NoStarter
                },
                prompt_delivery,
            }
        }
        crate::service::launch_delivery::LaunchDeliveryMode::ConfirmedFallback => {
            GeneralAgentLaunchInstructions {
                task: None,
                startup_message,
                starter_prompt: StarterPrompt::NoStarter,
                prompt_delivery,
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum GeneralAgentSessionPlan {
    FirstRunFresh,
    ConfiguredFresh,
    Restore(String),
    Discover,
}

fn general_agent_session_plan(
    resume_sessions_enabled: bool,
    force_fresh_session: bool,
    snapshot_session_id: Option<String>,
) -> GeneralAgentSessionPlan {
    if force_fresh_session {
        GeneralAgentSessionPlan::FirstRunFresh
    } else if !resume_sessions_enabled {
        GeneralAgentSessionPlan::ConfiguredFresh
    } else if let Some(session_id) = snapshot_session_id {
        GeneralAgentSessionPlan::Restore(session_id)
    } else {
        GeneralAgentSessionPlan::Discover
    }
}

/// Start the General Agent — called from start_servers on a background thread.
///
/// 1. Ensure directories + instructions file
/// 2. Find or create General workspace (protected)
/// 3. Find or create General tab (protected, aliased)
/// 4. Spawn PTY + launch or resume agent
/// 5. Start backend reader
pub fn start_general_agent(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    _app: &crate::app_runtime::AppHandle,
) {
    let Some(_launch_guard) = launch_lock::LaunchGuard::acquire() else {
        log::info!("[general-agent] Start skipped; another launch owns the agent");
        return;
    };
    let general_config = config.lock().general_agent.clone();
    let provider_name = &general_config.agent_provider;
    let resume_sessions_enabled = crate::config::resume_sessions_enabled(provider_name);
    let permission_bypass =
        match general_agent_permission_bypass(&svc.agent_registry, provider_name) {
            Ok(value) => value,
            Err(error) => {
                log::warn!("[general-agent] Start blocked: {error}");
                return;
            }
        };

    log::info!("[general-agent] Starting (provider={})", provider_name);
    // Regenerate AGENT.md in a background thread.
    // If mem.lock is busy, delay the snapshot pass rather than blocking PTY resume.
    {
        let mem = svc.mem.clone();
        let provider_owned = provider_name.clone();
        std::thread::spawn(move || {
            instructions::generate_instructions(&provider_owned, Some(&mem));
        });
    }

    let general_cwd = general_dir().to_string_lossy().to_string();

    // cosmetically restored — verify with screen markers and tear down the bare shell if no agent is running.
    {
        let (tab_id_opt, current_agent, current_pty_id, current_auto_approve) = {
            let m = mux.write();
            match m.resolve_tab_id(GENERAL_ALIAS) {
                Some(pid) => {
                    let tab = m.get_tab(&pid);
                    let at = tab.and_then(|p| p.agent_type.clone());
                    let pt = tab.and_then(|p| p.pty_id.clone());
                    let auto_approve = tab.is_some_and(|p| p.auto_approve);
                    (Some(pid), at, pt, auto_approve)
                }
                None => (None, None, None, false),
            }
        };
        if let (Some(pid), Some(current_agent)) = (tab_id_opt.clone(), current_agent.clone()) {
            if &current_agent == provider_name {
                if let Some(pty_id) = current_pty_id.clone() {
                    let alive = pty.lock().get_session(&pty_id).is_some();
                    if alive {
                        let agent_visible = await_agent_output(
                            screen_state,
                            &svc.agent_registry,
                            &pty_id,
                            std::time::Duration::from_secs(3),
                        );
                        if agent_visible && current_auto_approve == permission_bypass {
                            let unbound = mux
                                .write()
                                .get_tab(&pid)
                                .and_then(|p| p.agent_session_id.clone())
                                .is_none();
                            if unbound {
                                if let Some(sid) = state::load_session_id(
                                    &svc.agent_registry,
                                    provider_name,
                                    &general_cwd,
                                ) {
                                    svc.set_agent_session_id(&pid, Some(sid.clone()));
                                    log::info!("[general-agent] Restored durable session {} onto reused PTY {}", sid, pty_id);
                                }
                            }
                            svc.reconcile_boot_stage_with_session_binding(&pid);
                            log::info!("[general-agent] Already running (tab={}, pty={}, provider={}) — skipping", pid, pty_id, current_agent);
                            return;
                        }
                        if agent_visible {
                            log::info!("[general-agent] Permission policy changed for PTY {} — terminating before relaunch", pty_id);
                        } else {
                            log::info!("[general-agent] Tab has live PTY {} but no agent output before deadline — terminating before relaunch", pty_id);
                        }
                        if let Err(reason) =
                            terminate_pty_before_relaunch(pty, screen_state, &pty_id)
                        {
                            fail_general_agent_start(svc, mux, &pid, &reason);
                            return;
                        }
                        clear_stale_general_binding(svc, mux, &pid);
                    } else {
                        // Stale pty_id pointing at a dead session — clear it so spawn proceeds cleanly.
                        log::info!(
                            "[general-agent] Clearing stale pty_id {} (session not in PtyManager)",
                            pty_id
                        );
                        screen_state.lock().remove(&pty_id);
                        crate::service::stream_preview::remove(&pty_id);
                        clear_stale_general_binding(svc, mux, &pid);
                    }
                }
            } else {
                log::info!(
                    "[general-agent] Provider mismatch: running={}, wanted={} — restarting",
                    current_agent,
                    provider_name
                );
                if let Some(pty_id) = current_pty_id {
                    if let Err(reason) = terminate_pty_before_relaunch(pty, screen_state, &pty_id) {
                        fail_general_agent_start(svc, mux, &pid, &reason);
                        return;
                    }
                }
                clear_stale_general_binding(svc, mux, &pid);
            }
        }
    }

    // Find General tab (already created by ensure_general_workspace in init_states)
    let (tab_id, snapshot_session_id) = {
        let m = mux.write();
        let tab = m.resolve_tab_id(GENERAL_ALIAS).or_else(|| {
            m.list_workspaces()
                .iter()
                .find(|ws| ws.protected)
                .and_then(|ws| ws.tab_ids.first().cloned())
        });
        match tab {
            Some(pid) => {
                let sid = m.get_tab(&pid).and_then(|p| p.agent_session_id.clone());
                (pid, sid)
            }
            None => {
                log::error!(
                    "[general-agent] No General Tab found — ensure_general_workspace failed?"
                );
                return;
            }
        }
    };
    let restored = mux
        .read()
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.restored_from_snapshot);
    crate::service::supporter_notify::record_launch(&tab_id, restored);
    let _restore_observation =
        crate::service::supporter_notify::begin_restore_observation(svc, &tab_id);
    svc.clear_tab_restored_flag(&tab_id);
    let _automatic_launch_claim = match AutomaticLaunchClaim::acquire(mux, &tab_id) {
        Ok(claim) => claim,
        Err(error) => {
            log::error!("[general-agent] Start blocked: {error}");
            return;
        }
    };
    svc.emit_boot_stage(&tab_id, BootStage::PtyStarting, None);
    let force_fresh_session = state::take_fresh_session_requirement();
    let session_plan = general_agent_session_plan(
        resume_sessions_enabled,
        force_fresh_session,
        snapshot_session_id.clone(),
    );
    let session_id = match session_plan {
        GeneralAgentSessionPlan::FirstRunFresh => {
            svc.set_agent_session_id(&tab_id, None);
            state::clear_session_id();
            log::info!("[general-agent] First-run launch requires a fresh provider session");
            None
        }
        GeneralAgentSessionPlan::ConfiguredFresh => {
            if snapshot_session_id.is_some() {
                svc.set_agent_session_id(&tab_id, None);
            }
            state::clear_session_id();
            None
        }
        GeneralAgentSessionPlan::Restore(session_id) => Some(session_id),
        GeneralAgentSessionPlan::Discover => {
            if let Some(stored) =
                state::load_session_id(&svc.agent_registry, provider_name, &general_cwd)
            {
                svc.set_agent_session_id(&tab_id, Some(stored.clone()));
                log::info!("[general-agent] Loaded durable session id: {}", stored);
                Some(stored)
            } else {
                let exclude = mux.write().other_session_ids(&tab_id);
                let detected = state::detect_owned_session_id(
                    &svc.agent_registry,
                    provider_name,
                    &general_cwd,
                    &exclude,
                    Some(std::time::Duration::from_secs(86400 * 7)), // 7 days max age
                );
                if let Some(ref sid) = detected {
                    svc.set_agent_session_id(&tab_id, Some(sid.clone()));
                    state::persist_session_id(provider_name, sid);
                    log::info!(
                        "[general-agent] Detected GA-owned session from filesystem: {}",
                        sid
                    );
                }
                detected
            }
        }
    };

    let session_id = if let Some(ref id) = session_id {
        let valid = svc
            .agent_registry
            .with_provider(provider_name, |p| {
                p.session_exists(&general_cwd, id) == Some(true)
            })
            .unwrap_or(false);
        let owned = valid
            && state::session_id_is_owned(&svc.agent_registry, provider_name, &general_cwd, id);
        if !owned {
            log::info!("[general-agent] Session '{}' not valid or not GA-owned for provider '{}' — clearing, will detect fresh", id, provider_name);
            svc.set_agent_session_id(&tab_id, None);
            state::clear_session_id();
            let exclude = mux.write().other_session_ids(&tab_id);
            let fresh = state::detect_owned_session_id(
                &svc.agent_registry,
                provider_name,
                &general_cwd,
                &exclude,
                Some(std::time::Duration::from_secs(86400 * 7)),
            );
            if let Some(ref sid) = fresh {
                svc.set_agent_session_id(&tab_id, Some(sid.clone()));
                state::persist_session_id(provider_name, sid);
                log::info!("[general-agent] Detected fresh GA-owned session: {}", sid);
            }
            fresh
        } else {
            state::persist_session_id(provider_name, id);
            session_id
        }
    } else {
        session_id
    };

    launch::start(
        svc,
        mux,
        pty,
        screen_state,
        config,
        super::session_contract::TabLaunch {
            tab_id,
            session_id,
            provider: provider_name,
            permission_bypass,
            ownership: super::session_contract::LaunchOwnership::Main,
            selection: Default::default(),
        },
    );
}

pub(super) fn start_fresh_general_session(
    svc: &crate::ServiceState,
    config: &crate::config::ConfigState,
    tab_id: String,
    provider: &str,
    permission_bypass: bool,
    selection: super::session_contract::FreshLaunchSelection,
) {
    let _claim = match AutomaticLaunchClaim::acquire(&svc.mux, &tab_id) {
        Ok(claim) => claim,
        Err(error) => {
            log::error!("[general-agent] Fresh session blocked: {error}");
            return;
        }
    };
    launch::start(
        svc,
        &svc.mux,
        &svc.pty,
        &svc.screen_state,
        config,
        super::session_contract::TabLaunch {
            tab_id,
            session_id: None,
            provider,
            permission_bypass,
            ownership: super::session_contract::LaunchOwnership::FreshSession,
            selection,
        },
    );
}

/// Restart the General Agent with the current configuration.
pub fn restart_general_agent(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    app: &crate::app_runtime::AppHandle,
) {
    restart_general_agent_with_force(svc, mux, pty, screen_state, config, app, false);
}

pub(crate) fn restart_general_agent_with_force(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    app: &crate::app_runtime::AppHandle,
    force: bool,
) {
    log::info!("[general-agent] Manual restart requested");
    if let Err(error) = super::recovery_episode::store().reset_by_owner() {
        log::error!("[general-agent] Cannot reset recovery episode: {error}");
        return;
    }

    restart_general_agent_inner(svc, mux, pty, screen_state, config, app, force);
}

pub(crate) fn restart_general_agent_fresh_after_exit(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    app: &crate::app_runtime::AppHandle,
) -> Result<(), String> {
    state::require_fresh_session()?;
    restart_general_agent_inner(svc, mux, pty, screen_state, config, app, false);
    Ok(())
}

fn restart_general_agent_inner(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    app: &crate::app_runtime::AppHandle,
    force: bool,
) {
    if !super::restart_guard::permit(svc, mux, force) {
        return;
    }
    let mut stale_pty_id = None;
    let mut general_tab_id = None;
    {
        let m = mux.write();
        if let Some(pid) = m.resolve_tab_id(GENERAL_ALIAS) {
            general_tab_id = Some(pid.clone());
            if let Some(tab) = m.get_tab(&pid) {
                if let Some(ref pty_id) = tab.pty_id {
                    stale_pty_id = Some(pty_id.clone());
                    log::info!(
                        "[general-agent] Terminating PTY {} process tree for restart",
                        pty_id
                    );
                }
            }
            drop(m);
            clear_stale_general_binding(svc, mux, &pid);
        }
    }

    if let Some(pty_id) = stale_pty_id {
        if let Err(reason) = terminate_pty_before_relaunch(pty, screen_state, &pty_id) {
            log::error!("[general-agent] Restart blocked by PTY teardown: {reason}");
            if let Some(tab_id) = general_tab_id {
                svc.emit_boot_stage(&tab_id, BootStage::SpawnFailed, Some(&reason));
                if let Some(tab) = mux.write().get_tab_mut(&tab_id) {
                    tab.agent_booting = false;
                }
                svc.emit_state_changed();
            }
            return;
        }
    }

    // start_general_agent). Same rationale as the lifecycle::start_general_agent path: do not block restart on mem.wake_up().
    {
        let mem = svc.mem.clone();
        let provider_name = config.lock().general_agent.agent_provider.clone();
        std::thread::spawn(move || {
            instructions::generate_instructions(&provider_name, Some(&mem));
        });
    }

    start_general_agent(svc, mux, pty, screen_state, config, app);
}

pub fn restart_general_agent_tab(
    tab_id: &str,
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    config: &crate::config::ConfigState,
    app: &crate::app_runtime::AppHandle,
) -> Result<(), String> {
    if validate_general_agent_tab_retry(tab_id, mux)? {
        restart_general_agent(svc, mux, pty, screen_state, config, app);
    }
    Ok(())
}

pub fn validate_general_agent_tab_retry(
    tab_id: &str,
    mux: &crate::MuxState,
) -> Result<bool, String> {
    validate_general_agent_tab_retry_inner(tab_id, mux, false)
}

pub fn claim_general_agent_tab_retry(tab_id: &str, mux: &crate::MuxState) -> Result<bool, String> {
    validate_general_agent_tab_retry_inner(tab_id, mux, true)
}

fn validate_general_agent_tab_retry_inner(
    tab_id: &str,
    mux: &crate::MuxState,
    claim: bool,
) -> Result<bool, String> {
    let mut m = mux.write();
    let general_tab_id = m
        .resolve_tab_id(GENERAL_ALIAS)
        .ok_or_else(|| "General Agent Tab is not registered".to_string())?;
    let requested_tab_id = m
        .resolve_tab_id(tab_id)
        .unwrap_or_else(|| tab_id.to_string());
    if requested_tab_id != general_tab_id {
        return Err(format!(
            "Retry rejected: Tab {} is not the General Agent Tab {}",
            requested_tab_id, general_tab_id
        ));
    }
    let tab = m
        .get_tab(&general_tab_id)
        .ok_or_else(|| format!("General Agent Tab {} is missing", general_tab_id))?;
    if !tab.protected {
        return Err(format!(
            "Retry rejected: General Agent Tab {} is not protected",
            general_tab_id
        ));
    }
    // `agent_booting` is the claim marker — one owner, always.
    if tab.agent_booting {
        log::info!(
            "[general-agent] Retry for Tab {} ignored; launch already claimed",
            general_tab_id
        );
        return Ok(false);
    }
    // Refusing recovery on a launch stage without an owner left `shell_ready` tabs unrecoverable.
    if tab.boot_stage.is_some_and(is_launch_stage) {
        if launch_lock::launch_in_progress() {
            log::info!(
                "[general-agent] Retry for Tab {} ignored; launch already in progress",
                general_tab_id
            );
            return Ok(false);
        }
        log::info!(
            "[general-agent] Tab {} latched at {:?} with no live launch; treating as stale",
            general_tab_id,
            tab.boot_stage
        );
    }
    if claim {
        let _ = tab;
        if let Some(tab) = m.get_tab_mut(&general_tab_id) {
            apply_general_agent_launch_claim(tab);
        }
    }
    Ok(true)
}

fn is_launch_stage(stage: BootStage) -> bool {
    matches!(
        stage,
        BootStage::PtyStarting
            | BootStage::PtySpawned
            | BootStage::ReaderReady
            | BootStage::ShellReady
            | BootStage::AgentLaunching
            | BootStage::LaunchWritten
            | BootStage::ProviderReady
            | BootStage::PreparingRoleInstructions
            | BootStage::DeliveringInstructions
    )
}

fn tab_still_owns_pty(mux: &crate::MuxState, tab_id: &str, pty_id: &str) -> bool {
    mux.write()
        .get_tab(tab_id)
        .and_then(|p| p.pty_id.as_deref())
        == Some(pty_id)
}

fn terminate_pty_before_relaunch(
    pty: &crate::PtyState,
    screen_state: &crate::ScreenStateHandle,
    pty_id: &str,
) -> Result<(), String> {
    let reaper = {
        let mut manager = pty.lock();
        match manager.begin_kill(pty_id) {
            Ok(reaper) => Some(reaper),
            Err(_error) if manager.get_session(pty_id).is_none() => {
                log::info!("[general-agent] PTY {} was already torn down", pty_id);
                None
            }
            Err(error) => return Err(error),
        }
    };
    if let Some(reaper) = reaper {
        reaper.wait_bounded(std::time::Duration::from_secs(5))?;
    }
    screen_state.lock().remove(pty_id);
    crate::service::stream_preview::remove(pty_id);
    Ok(())
}

fn general_agent_permission_bypass(
    registry: &crate::agent::registry::AgentRegistry,
    provider: &str,
) -> Result<bool, String> {
    crate::service::permission_policy::configured_permission_bypass(registry, provider)
        .map(|authorization| authorization.is_some())
}

fn clear_stale_general_binding(svc: &crate::ServiceState, mux: &crate::MuxState, tab_id: &str) {
    svc.set_tab_agent_type(tab_id, None);
    svc.set_agent_session_id(tab_id, None);
    state::clear_session_id();
    if let Some(tab) = mux.write().get_tab_mut(tab_id) {
        tab.pty_id = None;
    }
}

fn fail_general_agent_start(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    tab_id: &str,
    reason: &str,
) {
    if let Some(tab) = mux.write().get_tab_mut(tab_id) {
        tab.agent_booting = false;
    }
    svc.emit_boot_stage(tab_id, BootStage::SpawnFailed, Some(reason));
    svc.emit_state_changed();
}

const BINDING_UNREPORTED: &str = "authoritative provider session binding was not reported";
const UNCONFIRMED_DELIVERY: &str =
    "submit_unconfirmed: launch payload written, pending evidence retained";
const UNACKNOWLEDGED_DELIVERY: &str = "pending: launch delivery is unacknowledged";

/// What the launch records when the binding never arrived, read from the
/// delivery the ledger still holds rather than inferred from a return code.
/// Bytes on the PTY make this uncertainty about one attempt, never a verdict.
fn missing_binding_provisional(
    delivery: Option<crate::multiplexer::types::PendingDeliveryState>,
) -> (BootStage, String) {
    use crate::multiplexer::types::PendingDeliveryState;
    let evidence = match delivery {
        Some(PendingDeliveryState::Unconfirmed) => Some(UNCONFIRMED_DELIVERY),
        Some(state) if state.needs_delivery() => Some(UNACKNOWLEDGED_DELIVERY),
        _ => None,
    };
    let reason = match evidence {
        Some(evidence) => format!("{evidence}; {BINDING_UNREPORTED}"),
        None => BINDING_UNREPORTED.to_string(),
    };
    (BootStage::LaunchBindingUnconfirmed, reason)
}

/// A missing session binding is this observer giving up, not the launch
/// failing. The tab keeps a provisional stage bound to this PTY and this launch
/// attempt, carrying the delivery evidence, so the agent's own late binding can
/// still retire it.
fn defer_general_agent_start_to_late_binding(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    tab_id: &str,
    pty_id: &str,
) {
    let delivery = mux
        .read()
        .get_tab(tab_id)
        .and_then(|tab| tab.pending_delivery.as_ref().map(|record| record.state()));
    let (stage, reason) = missing_binding_provisional(delivery);
    defer_general_agent_start(svc, mux, tab_id, pty_id, stage, reason);
}

/// An output deadline is this observer giving up, not the launch failing: the
/// tab keeps a provisional stage bound to this PTY and this launch attempt, so
/// the agent's own late session binding can still retire it.
fn defer_general_agent_start_to_late_readiness(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    tab_id: &str,
    pty_id: &str,
) {
    defer_general_agent_start(
        svc,
        mux,
        tab_id,
        pty_id,
        BootStage::LaunchDeadlineElapsed,
        crate::service::launch_readiness::GENERAL_AGENT_OUTPUT_DEADLINE.to_string(),
    );
}

/// Hand this observer's provisional evidence to the shared launch-readiness
/// owner, which is what binds it to the tab's own PTY and launch attempt.
fn defer_general_agent_start(
    svc: &crate::ServiceState,
    mux: &crate::MuxState,
    tab_id: &str,
    pty_id: &str,
    stage: BootStage,
    reason: String,
) {
    if let Some(tab) = mux.write().get_tab_mut(tab_id) {
        tab.agent_booting = false;
    }
    match crate::service::launch_readiness::LaunchAttempt::current(mux, tab_id, pty_id) {
        Some(attempt) => {
            svc.settle_launch_readiness(
                &attempt,
                crate::service::launch_readiness::LaunchEvidence::Unobserved { stage, reason },
            );
        }
        None => log::info!(
            "[general-agent] {} on PTY {pty_id} ignored; Tab {tab_id} moved on",
            stage.as_str()
        ),
    }
    svc.emit_state_changed();
}

#[cfg(test)]
mod tests;

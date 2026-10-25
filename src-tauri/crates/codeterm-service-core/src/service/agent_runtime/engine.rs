use super::{
    model::{self, ItemCache, RuntimeFailure, RuntimePhase, RuntimeState},
    process::Process,
    process_slot::ProcessSlot,
};
use crate::service::MuxEmitCause;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::*;
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
    time::Duration,
};

pub struct OutstandingRequest {
    pub request: PendingRuntimeRequest,
    pub prompt: Option<codeterm_types::pty::PendingPrompt>,
}

pub struct Session {
    pub(super) capacity_retry: Mutex<super::capacity_retry::Policy>,
    pub(super) update_generation: std::sync::atomic::AtomicU64,
    pub(super) update_recovery_available: std::sync::atomic::AtomicBool,
    pub provider: String,
    pub adapter: Mutex<Option<Arc<dyn crate::agent::provider::AgentProvider>>>,
    pub context: Mutex<RuntimeContext>,
    pub(super) controls_confirmation: Mutex<Option<RuntimeSettings>>,
    pub(crate) control_changes: tokio::sync::watch::Sender<()>,
    pub(super) reverted_thread: Mutex<Option<String>>,
    pub(super) completions: Mutex<super::completions::Completions>,
    pub state: Mutex<RuntimeState>,
    pub cache: Mutex<ItemCache>,
    pub auth: Mutex<RuntimeAuthState>,
    pub(super) login: Mutex<super::login::LoginState>,
    pub thread_loaded: std::sync::atomic::AtomicBool,
    pub hook_state: Mutex<Value>,
    pub maps: Mutex<super::events::RuntimeMaps>,
    pub(super) provider_operation_failures: std::sync::atomic::AtomicU32,
    pub(super) bookkeeping_warning_emitted: std::sync::atomic::AtomicBool,
    pub prompts: Mutex<HashMap<RpcId, OutstandingRequest>>,
    pub process: ProcessSlot<Arc<Process>>,
    pub gate: tokio::sync::Semaphore,
    pub(super) initialization: super::initialize::Initialization,
    pub(super) compaction: Mutex<super::subscriber::CompactionState>,
    pub(super) abort_note: Mutex<Option<String>>,
    pub(super) history_retry: Mutex<Option<backfill::FailedReplay>>,
}
static SESSIONS: LazyLock<Mutex<HashMap<String, Arc<Session>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn requested(svc: &ServiceContext, provider: &str) -> bool {
    svc.agent_registry
        .with_provider(provider, |provider| provider.agent_runtime().is_some())
        .unwrap_or(false)
}
pub fn for_tab(tab: &str) -> Option<Arc<Session>> {
    SESSIONS.lock().get(tab).cloned()
}

pub(super) fn login_pending_session(
    svc: &ServiceContext,
    tab: &str,
    error: RuntimeFailure,
) -> Result<Arc<Session>, RuntimeFailure> {
    if matches!(error, RuntimeFailure::SignInRequired) {
        if let Some(session) = for_tab(tab) {
            validate_binding(svc, &session)?;
            return Ok(session);
        }
    }
    Err(error)
}
pub(crate) fn has_live_bound_session(tab: &crate::multiplexer::types::Tab) -> bool {
    let Some(session) = for_tab(&tab.id) else {
        return false;
    };
    let context = session.context.lock().clone();
    let ready = matches!(session.state.lock().phase, RuntimePhase::Ready);
    let process_live = session
        .process
        .lock()
        .as_ref()
        .is_some_and(|process| process.ended.borrow().is_none());
    session.provider == tab.agent_type.as_deref().unwrap_or_default()
        && context.tab_id == tab.id
        && Some(context.binding_nonce.as_str()) == tab.session_binding_nonce.as_deref()
        && context.thread_id == tab.agent_session_id
        && session
            .thread_loaded
            .load(std::sync::atomic::Ordering::Acquire)
        && ready
        && process_live
}

#[path = "engine/backfill.rs"]
pub(super) mod backfill;

#[path = "engine/launch.rs"]
mod launch;
mod resume;
use launch::{require_launch_authority, validate_loaded_settings};

#[cfg(test)]
#[path = "engine/test_support.rs"]
pub(crate) mod test_support;

pub fn stop_blocking(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    super::login::close_cli(tab);
    if for_tab(tab).is_none() {
        return Ok(());
    }
    let finish = || crate::async_runtime::block_on(stop(svc, tab));
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(finish)
        }
        Ok(_) => Err("Runtime stop requires async stop on a current-thread executor".into()),
        Err(_) => finish(),
    }
}

pub(crate) fn stop_removed_provider(svc: &ServiceContext, provider: &str) {
    if svc.agent_registry.provider_arc(provider).is_some() {
        return;
    }
    let tabs: Vec<_> = SESSIONS
        .lock()
        .iter()
        .filter(|(_, session)| session.provider == provider)
        .map(|(tab, _)| tab.clone())
        .collect();
    for tab in tabs {
        let svc = svc.clone();
        let provider = provider.to_string();
        crate::async_runtime::spawn(async move {
            if svc.agent_registry.provider_arc(&provider).is_none() {
                if let Err(error) = stop(&svc, &tab).await {
                    log::warn!("[runtime] removed provider shutdown failed for {tab}: {error}");
                }
            }
        });
    }
}

pub async fn stop(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    if let Err(error) = super::capacity_retry::cancel(svc, tab) {
        log::warn!("[runtime-retry] queued retry cleanup failed for {tab}: {error}");
    }
    let Some(session) = for_tab(tab) else {
        return Ok(());
    };
    let permit = session.gate.try_acquire().ok();
    session.gate.close();
    let process = session.process.lock().clone();
    if let Some(process) = process {
        if let Err(reason) = super::background::cleanup(
            svc,
            &session,
            &process,
            super::background::Mode::Shutdown,
            Vec::new(),
            permit.is_some(),
        )
        .await
        {
            let epoch = session.context.lock().epoch;
            session.state.lock().degrade(epoch, &reason);
            publish(svc, tab, &session);
            return Err(reason.to_string());
        }
    }
    {
        let mut sessions = SESSIONS.lock();
        if !sessions
            .get(tab)
            .is_some_and(|current| Arc::ptr_eq(current, &session))
        {
            return Err(RuntimeFailure::Superseded.to_string());
        }
        sessions.remove(tab);
    }
    super::authority::revoke(tab);
    super::environment::revoke(tab);
    super::capture::revoke(tab);
    super::login::forget(&session);
    session.state.lock().phase = RuntimePhase::Unloaded {
        reason: Some("Runtime stopped".into()),
    };
    session.control_changes.send_replace(());
    session.process.close();
    Ok(())
}

pub async fn ensure(svc: &ServiceContext, tab: &str) -> Result<Arc<Session>, RuntimeFailure> {
    ensure_with_fork(svc, tab, None).await
}

pub(super) async fn ensure_fork(
    svc: &ServiceContext,
    tab: &str,
    source_thread_id: &str,
    turn_id: &str,
) -> Result<Arc<Session>, RuntimeFailure> {
    ensure_with_fork(
        svc,
        tab,
        Some((source_thread_id.to_string(), turn_id.to_string())),
    )
    .await
}

async fn ensure_with_fork(
    svc: &ServiceContext,
    tab: &str,
    fork: Option<(String, String)>,
) -> Result<Arc<Session>, RuntimeFailure> {
    if svc
        .mux
        .read()
        .get_tab(tab)
        .is_some_and(|target| target.frozen)
    {
        return Err(RuntimeFailure::Refused(
            "Tab is sleeping; open or address it to wake".into(),
        ));
    }
    super::restore::tab(svc, tab).map_err(RuntimeFailure::Refused)?;
    require_launch_authority(svc, tab)?;
    let prompt_service = svc.clone();
    let prompt_tab = tab.to_string();
    tokio::task::spawn_blocking(move || {
        crate::service::orchestrator::ensure_tab_runtime_prompt(&prompt_service, &prompt_tab)
    })
    .await
    .map_err(|error| RuntimeFailure::HookFailed(error.to_string()))?
    .map_err(RuntimeFailure::Refused)?;
    let (provider, thread, nonce, config, cwd, saved_state) = {
        let mux = svc.mux.read();
        let target = mux.get_tab(tab).ok_or(RuntimeFailure::Superseded)?;
        (
            target
                .agent_type
                .clone()
                .ok_or(RuntimeFailure::Superseded)?,
            target.agent_session_id.clone(),
            target
                .session_binding_nonce
                .clone()
                .ok_or(RuntimeFailure::Superseded)?,
            target.agent_runtime_config.clone().unwrap_or(Value::Null),
            target.cwd.clone().filter(|cwd| !cwd.is_empty()).ok_or_else(|| {
                RuntimeFailure::Refused(
                    "The agent needs a working folder: start it in a workspace that has a folder, or pass --cwd".into(),
                )
            })?,
            target.agent_runtime_state.clone(),
        )
    };
    if !super::requested_for_tab(svc, tab) {
        return Err(RuntimeFailure::Refused("Runtime capability absent".into()));
    }
    let session = {
        let mut sessions = SESSIONS.lock();
        if sessions.get(tab).is_some_and(|session| {
            session.context.lock().binding_nonce != nonce || session.provider != provider
        }) {
            if let Some(old) = sessions.remove(tab) {
                if let Some(process) = old.process.close() {
                    process.stop();
                }
            }
        }
        sessions
            .entry(tab.into())
            .or_insert_with(|| {
                let mut state = RuntimeState::unloaded(thread.clone());
                state.restore_timing(saved_state.as_ref());
                Arc::new(Session {
                    capacity_retry: Mutex::new(Default::default()),
                    update_generation: std::sync::atomic::AtomicU64::new(0),
                    update_recovery_available: std::sync::atomic::AtomicBool::new(false),
                    provider: provider.clone(),
                    adapter: Mutex::new(None),
                    controls_confirmation: Mutex::new(None),
                    control_changes: tokio::sync::watch::channel(()).0,
                    reverted_thread: Mutex::new(None),
                    completions: Mutex::new(Default::default()),
                    context: Mutex::new(RuntimeContext {
                        epoch: 0,
                        tab_id: tab.into(),
                        binding_nonce: nonce.clone(),
                        host_owned: crate::agent::DOMIOS_HOST_OWNED_CAPABILITIES,
                        thread_id: thread.clone(),
                        active_turn_id: None,
                        settings: super::model::settings_from_config(&config),
                        provider_settings: super::model::public_provider_settings(&config),
                        cwd: cwd.clone(),
                        host_version: codeterm_build_info::version().into(),
                        claimed_sends: Vec::new(),
                        active_turns: Vec::new(),
                        thread_parents: Vec::new(),
                        pending_request: None,
                    }),
                    state: Mutex::new(state),
                    cache: Mutex::new(ItemCache::default()),
                    auth: Mutex::new(RuntimeAuthState::Unknown),
                    login: Mutex::new(Default::default()),
                    thread_loaded: std::sync::atomic::AtomicBool::new(false),
                    hook_state: Mutex::new(Value::Null),
                    maps: Mutex::new(super::events::RuntimeMaps::default()),
                    provider_operation_failures: std::sync::atomic::AtomicU32::new(0),
                    bookkeeping_warning_emitted: std::sync::atomic::AtomicBool::new(false),
                    prompts: Mutex::new(HashMap::new()),
                    process: ProcessSlot::new(None),
                    gate: tokio::sync::Semaphore::new(1),
                    initialization: super::initialize::Initialization::default(),
                    compaction: Mutex::new(Default::default()),
                    abort_note: Mutex::new(None),
                    history_retry: Mutex::new(None),
                })
            })
            .clone()
    };
    let attempted = std::sync::atomic::AtomicBool::new(false);
    let result = tokio::time::timeout(Duration::from_secs(60), async {
        let _permit = session
            .gate
            .acquire()
            .await
            .map_err(|_| RuntimeFailure::Superseded)?;
        attempted.store(true, std::sync::atomic::Ordering::Release);
        ensure_locked(svc, tab, &provider, &config, &session, fork.as_ref()).await
    })
    .await
    .map_err(|_| RuntimeFailure::Deadline)
    .and_then(|result| result);
    if let Err(reason) = result {
        if attempted.load(std::sync::atomic::Ordering::Acquire) {
            let epoch = session.state.lock().epoch;
            session.state.lock().degrade(epoch, &reason);
            publish(svc, tab, &session);
        }
        return Err(reason);
    }
    Ok(session)
}

async fn ensure_locked(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    config: &Value,
    session: &Arc<Session>,
    fork: Option<&(String, String)>,
) -> Result<(), RuntimeFailure> {
    if fork.is_some() && session.context.lock().thread_id.is_some() {
        return Err(RuntimeFailure::Refused(
            "Fork startup requires an unbound child runtime".into(),
        ));
    }
    let nonce = session.context.lock().binding_nonce.clone();
    {
        let mux = svc.mux.read();
        let target = mux.get_tab(tab).ok_or(RuntimeFailure::Superseded)?;
        if target.agent_type.as_deref() != Some(provider)
            || target.session_binding_nonce.as_deref() != Some(nonce.as_str())
        {
            return Err(RuntimeFailure::Superseded);
        }
        validate_restored_arguments(provider, Some(&target.restorable_launch))?;
    }
    let epoch = session.context.lock().epoch;
    if session.compaction.lock().held(epoch) {
        return Err(RuntimeFailure::Refused(
            "Context restoration pending or failed; sends held".into(),
        ));
    }
    let old = session.process.lock().clone();
    let live = session.adapter.lock().is_some()
        && old.as_ref().is_some_and(|process| {
            process.ended.borrow().is_none() && process.client.ended().borrow().is_none()
        });
    if live && matches!(session.state.lock().phase, RuntimePhase::Ready) {
        return validate_binding(svc, session);
    }
    let started = std::time::Instant::now();
    let mut spawned_ms = None;
    let owner = svc.clone();
    let target = tab.to_string();
    let settings =
        tokio::task::spawn_blocking(move || super::launch_options::restore(&owner, &target))
            .await
            .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?
            .map_err(RuntimeFailure::Refused)?;
    let resolved_cwd = svc.mux.read().get_tab(tab).and_then(|tab| tab.cwd.clone());
    {
        let mut context = session.context.lock();
        context.settings = settings;
        if let Some(cwd) = resolved_cwd {
            context.cwd = cwd;
        }
    }
    crate::startup::boot_timing::mark(tab, None, "runtime_options_ready");
    let process = if live {
        old.ok_or(RuntimeFailure::Superseded)?
    } else {
        if let Some(old) = old {
            old.finish().await?;
        }
        let epoch = {
            let mut state = session.state.lock();
            state.binary_version = None;
            state.begin()
        };
        session
            .provider_operation_failures
            .store(0, std::sync::atomic::Ordering::Relaxed);
        session
            .bookkeeping_warning_emitted
            .store(false, std::sync::atomic::Ordering::Relaxed);
        {
            let mut context = session.context.lock();
            context.epoch = epoch;
            context.claimed_sends.clear();
            context.pending_request = None;
            context.active_turn_id = None;
            context.active_turns.clear();
            context.thread_parents.clear();
        }
        session
            .thread_loaded
            .store(false, std::sync::atomic::Ordering::Release);
        *session.completions.lock() = Default::default();
        session.initialization.reset();
        super::login::forget(session);
        *session.login.lock() = Default::default();
        *session.auth.lock() = RuntimeAuthState::Unknown;
        publish(svc, tab, session);
        let adapter = svc
            .agent_registry
            .provider_arc(provider)
            .ok_or(RuntimeFailure::Superseded)?;
        let capability = adapter.agent_runtime().ok_or(RuntimeFailure::Superseded)?;
        *session.adapter.lock() = Some(adapter.clone());
        let context = session.context.lock().clone();
        plan(session, &context, &RuntimeOperation::Initialize).await?;
        let owner = svc.clone();
        let target = tab.to_string();
        let provider_id = provider.to_string();
        let process = tokio::task::spawn_blocking(move || {
            let nonce = owner
                .mux
                .read()
                .get_tab(&target)
                .and_then(|tab| tab.session_binding_nonce.clone())
                .ok_or(RuntimeFailure::Superseded)?;
            let authority = LaunchAuthority::verify(
                &owner,
                RuntimeLaunchBinding {
                    tab_id: target.clone(),
                    provider_id: provider_id.clone(),
                    binding_nonce: nonce.clone(),
                },
            )
            .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
            let binary = adapter
                .authorize_agent_runtime(&authority)
                .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
            let cwd = owner
                .mux
                .read()
                .get_tab(&target)
                .and_then(|tab| tab.cwd.clone())
                .ok_or_else(|| RuntimeFailure::SpawnFailed {
                    reason: "Runtime cwd missing".into(),
                })?;
            let mut env = super::environment::compose(&owner, &target, &provider_id, &nonce);
            env.extend(
                owner
                    .model_provider_launch_env(&provider_id)
                    .map_err(|error| RuntimeFailure::SpawnFailed {
                        reason: error.to_string(),
                    })?,
            );
            env.extend(crate::service::execution_sandbox::environment(
                &owner, &target,
            ));
            if !super::authority::verifies(&owner, &target, &provider_id, &nonce) {
                return Err(RuntimeFailure::Superseded);
            }
            super::process::spawn(&binary, &capability, &cwd, env, &target).map(Arc::new)
        })
        .await
        .map_err(|error| RuntimeFailure::SpawnFailed {
            reason: error.to_string(),
        })??;
        if !session
            .process
            .install(process.clone(), |process| process.stop())
        {
            process.finish().await?;
            return Err(RuntimeFailure::Superseded);
        }
        crate::startup::boot_timing::mark(tab, None, "runtime_spawned");
        spawned_ms = Some(started.elapsed().as_millis());
        *session.hook_state.lock() = Value::Null;
        session.prompts.lock().clear();
        super::subscriber::start(
            svc.clone(),
            provider.into(),
            session.clone(),
            process.clone(),
        );
        process
    };
    session
        .initialization
        .ensure(|| async {
            let result = run(
                svc,
                provider,
                session,
                &process,
                RuntimeOperation::Initialize,
            )
            .await?;
            let RuntimeResult::Initialized { binary_version } = result else {
                return Err(RuntimeFailure::HookFailed(
                    "Initialize result is invalid".into(),
                ));
            };
            session.state.lock().binary_version = binary_version;
            Ok(())
        })
        .await?;
    crate::startup::boot_timing::mark(tab, None, "runtime_initialized");
    let initialized_ms = started.elapsed().as_millis();
    if matches!(*session.auth.lock(), RuntimeAuthState::SignedOut) {
        let thread_id = session.context.lock().thread_id.clone();
        run(
            svc,
            provider,
            session,
            &process,
            RuntimeOperation::Control {
                thread_id,
                action: "auth".into(),
                value: Value::Null,
            },
        )
        .await?;
        if matches!(*session.auth.lock(), RuntimeAuthState::SignedOut) {
            return Err(RuntimeFailure::SignInRequired);
        }
    }
    let context = session.context.lock().clone();
    if context.thread_id.is_none()
        && svc
            .mux
            .read()
            .get_tab(tab)
            .is_some_and(|tab| tab.restored_from_snapshot)
    {
        return Err(RuntimeFailure::Refused(
            "Restored runtime thread identity missing".into(),
        ));
    }
    let operation = match fork {
        Some((source_thread_id, turn_id)) => RuntimeOperation::ForkThread {
            source_thread_id: source_thread_id.clone(),
            turn_id: turn_id.clone(),
        },
        None => match context.thread_id.clone() {
            Some(thread_id) => RuntimeOperation::ResumeThread {
                thread_id,
                instructions: config["systemPrompt"].as_str().unwrap_or_default().into(),
                settings: context.settings.clone(),
            },
            None => RuntimeOperation::OpenThread {
                instructions: config["systemPrompt"].as_str().unwrap_or_default().into(),
                settings: context.settings.clone(),
            },
        },
    };
    let result = resume::load(svc, provider, session, &process, operation).await?;
    let context = session.context.lock().clone();
    let (thread_id, settings) = match (fork, result) {
        (
            Some((expected_source, expected_turn)),
            RuntimeResult::ThreadForked {
                source_thread_id,
                thread_id,
                turn_id,
            },
        ) if source_thread_id == *expected_source
            && turn_id == *expected_turn
            && thread_id != *expected_source =>
        {
            (thread_id, context.settings.clone())
        }
        (
            None,
            RuntimeResult::ThreadLoaded {
                thread_id,
                settings,
            },
        ) => {
            validate_loaded_settings(&context.settings, &settings, |requested, reported| {
                crate::service::session_model::session_model_matches(
                    &svc.agent_registry,
                    provider,
                    requested,
                    reported,
                )
            })?;
            (thread_id, settings)
        }
        _ => {
            return Err(RuntimeFailure::HookFailed(
                "Thread load result is invalid".into(),
            ));
        }
    };
    crate::startup::boot_timing::mark(tab, None, "thread_resumed");
    let resumed_ms = started.elapsed().as_millis();
    if context
        .thread_id
        .as_deref()
        .is_some_and(|bound| bound != thread_id)
    {
        return Err(RuntimeFailure::Superseded);
    }
    {
        let mut context = session.context.lock();
        context.thread_id = Some(thread_id.clone());
        context.settings = settings;
    }
    bind(svc, session, &thread_id)?;
    if context.thread_id.is_none() && fork.is_none() {
        resume::mark_unused(svc, session, &thread_id)?;
    }
    let settings = session.context.lock().settings.clone();
    super::projections::controls(svc, session, settings);
    if let Err(reason) = super::controls::refresh_service_tiers(svc, session).await {
        log::debug!("Runtime service tier catalogue unavailable: {reason}");
    }
    let owner = svc.clone();
    let target = tab.to_string();
    let thread = thread_id.clone();
    let nonce = session.context.lock().binding_nonce.clone();
    tokio::task::spawn_blocking(move || {
        owner.finalize_runtime_worker_session(&target, &thread, &nonce)?;
        let main = {
            let mux = owner.mux.read();
            mux.get_tab(&target).is_some_and(|tab| {
                crate::service::general_agent::session_contract::owns_main(&mux, tab)
            })
        };
        if main {
            let provider = owner
                .mux
                .read()
                .get_tab(&target)
                .and_then(|tab| tab.agent_type.clone());
            if let Some(provider) = provider {
                crate::service::general_agent::persist_switched_session(&provider, &thread);
            }
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?
    .map_err(RuntimeFailure::HookFailed)?;
    let owner = svc.clone();
    let target = tab.to_string();
    let thread = thread_id.clone();
    let questions = tokio::task::spawn_blocking(move || {
        owner
            .pending_delivery_payloads
            .user_sends
            .runtime_questions(&target, &thread)
    })
    .await
    .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?
    .map_err(RuntimeFailure::HookFailed)?;
    super::question_delivery::publish(svc, tab, session, questions)
        .map_err(RuntimeFailure::HookFailed)?;
    let generation = {
        let mut checkpoint = session.history_retry.lock();
        *checkpoint = None;
        session.cache.lock().begin_backfill()
    };
    let mut port = backfill::RpcPort::new(svc, session, &process, tab, &thread_id);
    let mut replay = backfill::Replay::new(context.epoch, generation, &thread_id);
    let history_adapter = session.adapter.lock().clone();
    let history_cwd = context.cwd.clone();
    let history_thread = thread_id.clone();
    let stored_history = tokio::task::spawn_blocking(move || {
        history_adapter.is_some_and(|adapter| {
            adapter.runtime_stored_history_available(&history_cwd, &history_thread)
        })
    })
    .await
    .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?;
    log::debug!("[chat_backfill] tab={tab} thread={thread_id} stored_history={stored_history}");
    if stored_history {
        session.cache.lock().bound_rereadable();
        replay.bound_startup();
    }
    if let Err(reason) = replay.first(session, &mut port).await {
        session
            .cache
            .lock()
            .finish_backfill(generation, model::HistoryBackfill::Failed);
        return Err(reason);
    }
    let first_page_ms = started.elapsed().as_millis();
    super::bootstrap::submit(svc, session, &process).await?;
    inject_abort_note(svc, provider, session, &process, &thread_id).await;
    if !session.state.lock().load(context.epoch, &thread_id) {
        return Err(RuntimeFailure::Superseded);
    }
    session
        .thread_loaded
        .store(true, std::sync::atomic::Ordering::Release);
    crate::startup::boot_timing::mark(tab, None, "runtime_ready");
    let reopen = backfill::Reopen {
        started,
        spawned_ms,
        initialized_ms,
        resumed_ms,
        first_page_ms,
        ready_ms: started.elapsed().as_millis(),
    };
    if replay.done {
        backfill::finish(&port, &replay, Ok(()), &reopen);
    } else {
        publish(svc, tab, session);
        backfill::continue_in_background(port, replay, reopen);
    }
    svc.emit_state_changed(MuxEmitCause::TabLifecycle);
    svc.on_turn_resume_binding_changed(tab);
    Ok(())
}

async fn inject_abort_note(
    svc: &ServiceContext,
    provider: &str,
    session: &Arc<Session>,
    process: &Arc<Process>,
    thread: &str,
) {
    let Some(cause) = session.abort_note.lock().take() else {
        return;
    };
    let operation = RuntimeOperation::Control {
        thread_id: Some(thread.to_string()),
        action: "restoreContext".into(),
        value: super::abort_provenance::abort_note_context(&cause),
    };
    if let Err(reason) = run(svc, provider, session, process, operation).await {
        let tab = session.context.lock().tab_id.clone();
        log::warn!("[agent-runtime] tab={tab} abort notice not delivered to the agent: {reason}");
    }
}

#[path = "engine/operation.rs"]
mod operation;
pub(super) use operation::plan;
pub use operation::run;
pub(crate) use operation::run_submit;

pub(crate) fn record_completed_turn(session: &Session, thread: &str, turn: Option<&str>) {
    if let Some(turn) = turn {
        session.completions.lock().record(thread, turn);
        session.control_changes.send_replace(());
    }
}

pub fn validate_binding(svc: &ServiceContext, session: &Session) -> Result<(), RuntimeFailure> {
    validate_binding_inner(svc, session, true)
}

pub(super) fn validate_restart_binding(
    svc: &ServiceContext,
    session: &Session,
) -> Result<(), RuntimeFailure> {
    validate_binding_inner(svc, session, false)
}

fn validate_binding_inner(
    svc: &ServiceContext,
    session: &Session,
    require_thread: bool,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    if session.adapter.lock().is_none()
        || svc.agent_registry.provider_arc(&session.provider).is_none()
    {
        return Err(RuntimeFailure::Superseded);
    }
    if svc.mux.read().get_tab(&context.tab_id).is_some_and(|tab| {
        tab.agent_type.as_deref() == Some(session.provider.as_str())
            && tab.session_binding_nonce.as_deref() == Some(context.binding_nonce.as_str())
            && (!require_thread
                || context
                    .thread_id
                    .as_deref()
                    .is_none_or(|thread| tab.agent_session_id.as_deref() == Some(thread)))
    }) {
        Ok(())
    } else {
        Err(RuntimeFailure::Superseded)
    }
}
fn bind(svc: &ServiceContext, session: &Session, thread: &str) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    let mut mux = svc.mux.write();
    let tab = mux
        .get_tab_mut(&context.tab_id)
        .ok_or(RuntimeFailure::Superseded)?;
    commit_runtime_binding(tab, &context.binding_nonce, thread, |tab| {
        svc.publish_boot_stage_transition(
            &context.tab_id,
            tab,
            codeterm_types::pty::BootStage::AgentReady,
            None,
        );
    })
}

fn commit_runtime_binding(
    tab: &mut crate::multiplexer::types::Tab,
    nonce: &str,
    thread: &str,
    publish: impl FnOnce(&crate::multiplexer::types::Tab),
) -> Result<(), RuntimeFailure> {
    if tab.session_binding_nonce.as_deref() != Some(nonce)
        || tab
            .agent_session_id
            .as_deref()
            .is_some_and(|bound| bound != thread)
    {
        return Err(RuntimeFailure::Superseded);
    }
    tab.agent_session_id = Some(thread.into());
    tab.reported_session_binding_nonce = Some(nonce.into());
    tab.agent_booting = false;
    crate::service::boot_stage::apply_boot_stage(
        tab,
        codeterm_types::pty::BootStage::AgentReady,
        None,
    );
    if let crate::multiplexer::types::TabBacking::ChatBackend { session_id, .. } = &mut tab.backing
    {
        *session_id = thread.into();
    }
    publish(tab);
    Ok(())
}

pub(super) fn snapshot(session: &Session) -> model::RuntimeSnapshot {
    let mut snapshot = session.state.lock().snapshot();
    super::update::mask_recoverable_failure(
        &mut snapshot,
        session
            .update_recovery_available
            .load(std::sync::atomic::Ordering::Acquire),
    );
    {
        let cache = session.cache.lock();
        snapshot.history = Some(cache.backfill());
        snapshot.history_pages = Some(cache.history_pages());
    }
    snapshot.history_error = session
        .history_retry
        .lock()
        .as_ref()
        .map(|failure| failure.error.clone());
    snapshot
}

pub fn publish(svc: &ServiceContext, tab: &str, session: &Session) {
    let context = session.context.lock().clone();
    let mut value = serde_json::to_value(snapshot(session)).unwrap_or(Value::Null);
    if let Some(snapshot) = value.as_object_mut() {
        snapshot.insert(
            "auth".into(),
            serde_json::to_value(session.auth.lock().clone()).unwrap_or(Value::Null),
        );
        snapshot.insert(
            "login".into(),
            serde_json::to_value(super::login::project(
                session.login.lock().login.as_ref(),
                super::login::now_ms(),
            ))
            .unwrap_or(Value::Null),
        );
        let (history, pages) = {
            let cache = session.cache.lock();
            (cache.backfill(), cache.history_pages())
        };
        snapshot.insert(
            "history".into(),
            serde_json::to_value(history).unwrap_or(Value::Null),
        );
        snapshot.insert("historyPages".into(), Value::from(pages));
        if let Some(adapter) = session.adapter.lock().as_ref() {
            if let Some(login) = adapter.agent_runtime().and_then(|runtime| runtime.login) {
                snapshot.insert(
                    "loginMethods".into(),
                    serde_json::to_value(login.methods).unwrap_or(Value::Null),
                );
                snapshot.insert(
                    "loginProviderName".into(),
                    Value::String(adapter.display_name().into()),
                );
                snapshot.insert(
                    "loginTerminal".into(),
                    Value::Bool(login.terminal_command.is_some()),
                );
            }
        }
    }
    {
        let mut mux = svc.mux.write();
        let Some(target) = mux.get_tab_mut(tab) else {
            return;
        };
        if target.agent_type.as_deref() != Some(session.provider.as_str())
            || target.session_binding_nonce.as_deref() != Some(context.binding_nonce.as_str())
        {
            return;
        }
        if ["active_turn_started_at_ms", "last_turn_duration_ms"]
            .iter()
            .any(|field| {
                target
                    .agent_runtime_state
                    .as_ref()
                    .and_then(|saved| saved.get(*field))
                    != value.get(*field)
            })
        {
            target.record_activity();
        }
        if value["state"] == "loaded"
            && context.thread_id.is_some()
            && target.agent_session_id == context.thread_id
            && target.reported_session_binding_nonce.as_deref()
                == Some(context.binding_nonce.as_str())
            && session
                .thread_loaded
                .load(std::sync::atomic::Ordering::Acquire)
            && session
                .process
                .lock()
                .as_ref()
                .is_some_and(|process| process.ended.borrow().is_none())
        {
            crate::service::boot_stage::apply_boot_stage(
                target,
                codeterm_types::pty::BootStage::AgentReady,
                None,
            );
        }
        target.agent_runtime_state = Some(value.clone());
        target.agent_booting = value["state"] == "loading";
    }
    session.control_changes.send_replace(());
    svc.notify_event(
        "agent-runtime-state",
        serde_json::json!({"tab_id":tab,"session_binding_nonce":context.binding_nonce,"state":value}),
    );
}

fn validate_restored_arguments(
    provider: &str,
    launch: Option<&crate::multiplexer::types::RestorableLaunchParameters>,
) -> Result<(), RuntimeFailure> {
    let args = launch
        .and_then(|launch| launch.args.as_deref())
        .unwrap_or_default();
    crate::service::spawn_capability::validate_runtime_launch_arguments(provider, args)
        .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))
}

#[cfg(test)]
#[path = "engine/tests.rs"]
mod tests;

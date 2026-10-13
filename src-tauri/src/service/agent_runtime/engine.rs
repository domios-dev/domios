use super::{
    model::{ItemCache, RuntimeFailure, RuntimePhase, RuntimeState},
    process::Process,
};
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
    pub provider: String,
    pub adapter: Mutex<Option<Arc<dyn crate::agent::provider::AgentProvider>>>,
    pub context: Mutex<RuntimeContext>,
    pub(super) completions: Mutex<super::completions::Completions>,
    pub state: Mutex<RuntimeState>,
    pub cache: Mutex<ItemCache>,
    pub auth: Mutex<RuntimeAuthState>,
    pub thread_loaded: std::sync::atomic::AtomicBool,
    pub hook_state: Mutex<Value>,
    pub prompts: Mutex<HashMap<RpcId, OutstandingRequest>>,
    pub process: Mutex<Option<Arc<Process>>>,
    pub gate: tokio::sync::Semaphore,
    initialization: super::initialize::Initialization,
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
pub fn stop(tab: &str) {
    super::authority::revoke(tab);
    super::environment::revoke(tab);
    super::capture::revoke(tab);
    if let Some(session) = SESSIONS.lock().remove(tab) {
        if let Some(process) = session.process.lock().take() {
            process.stop();
        }
    }
}

pub async fn ensure(svc: &ServiceContext, tab: &str) -> Result<Arc<Session>, RuntimeFailure> {
    super::restore::tab(svc, tab).map_err(RuntimeFailure::Refused)?;
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
            target.cwd.clone().unwrap_or_default(),
            target.agent_runtime_state.clone(),
        )
    };
    if !requested(svc, &provider) {
        return Err(RuntimeFailure::Refused("Runtime capability absent".into()));
    }
    let session = {
        let mut sessions = SESSIONS.lock();
        if sessions.get(tab).is_some_and(|session| {
            session.context.lock().binding_nonce != nonce || session.provider != provider
        }) {
            if let Some(old) = sessions.remove(tab) {
                if let Some(process) = old.process.lock().take() {
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
                    provider: provider.clone(),
                    adapter: Mutex::new(None),
                    completions: Mutex::new(Default::default()),
                    context: Mutex::new(RuntimeContext {
                        epoch: 0,
                        tab_id: tab.into(),
                        binding_nonce: nonce.clone(),
                        thread_id: thread.clone(),
                        active_turn_id: None,
                        settings: super::model::settings_from_config(&config),
                        provider_settings: super::model::public_provider_settings(&config),
                        cwd: cwd.clone(),
                        host_version: env!("CARGO_PKG_VERSION").into(),
                        claimed_sends: Vec::new(),
                        active_turns: Vec::new(),
                        thread_parents: Vec::new(),
                        pending_request: None,
                    }),
                    state: Mutex::new(state),
                    cache: Mutex::new(ItemCache::default()),
                    auth: Mutex::new(RuntimeAuthState::Unknown),
                    thread_loaded: std::sync::atomic::AtomicBool::new(false),
                    hook_state: Mutex::new(Value::Null),
                    prompts: Mutex::new(HashMap::new()),
                    process: Mutex::new(None),
                    gate: tokio::sync::Semaphore::new(1),
                    initialization: super::initialize::Initialization::default(),
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
        ensure_locked(svc, tab, &provider, &config, &session).await
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
) -> Result<(), RuntimeFailure> {
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
    let old = session.process.lock().clone();
    let registered = svc.agent_registry.provider_arc(provider);
    let same_adapter = session
        .adapter
        .lock()
        .as_ref()
        .zip(registered.as_ref())
        .is_some_and(|(active, current)| Arc::ptr_eq(active, current));
    let live = same_adapter
        && old.as_ref().is_some_and(|process| {
            process.ended.borrow().is_none() && process.client.ended().borrow().is_none()
        });
    if live && matches!(session.state.lock().phase, RuntimePhase::Ready) {
        return validate_binding(svc, session);
    }
    let owner = svc.clone();
    let target = tab.to_string();
    let settings =
        tokio::task::spawn_blocking(move || super::launch_options::restore(&owner, &target))
            .await
            .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?
            .map_err(RuntimeFailure::Refused)?;
    session.context.lock().settings = settings;
    let process = if live {
        old.ok_or(RuntimeFailure::Superseded)?
    } else {
        if let Some(old) = old {
            old.finish().await?;
        }
        let epoch = session.state.lock().begin();
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
            if !super::authority::verifies(&owner, &target, &provider_id, &nonce) {
                return Err(RuntimeFailure::Superseded);
            }
            super::process::spawn(&binary, &capability, &cwd, env, &target).map(Arc::new)
        })
        .await
        .map_err(|error| RuntimeFailure::SpawnFailed {
            reason: error.to_string(),
        })??;
        *session.process.lock() = Some(process.clone());
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
            if !matches!(
                run(
                    svc,
                    provider,
                    session,
                    &process,
                    RuntimeOperation::Initialize
                )
                .await?,
                RuntimeResult::Initialized
            ) {
                return Err(RuntimeFailure::HookFailed(
                    "Initialize result is invalid".into(),
                ));
            }
            Ok(())
        })
        .await?;
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
            return Err(RuntimeFailure::Refused("Sign-in required".into()));
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
    let operation = match context.thread_id.clone() {
        Some(thread_id) => RuntimeOperation::ResumeThread {
            thread_id,
            settings: context.settings.clone(),
        },
        None => RuntimeOperation::OpenThread {
            instructions: config["systemPrompt"].as_str().unwrap_or_default().into(),
            settings: context.settings.clone(),
        },
    };
    let RuntimeResult::ThreadLoaded {
        thread_id,
        settings,
    } = run(svc, provider, session, &process, operation).await?
    else {
        return Err(RuntimeFailure::HookFailed(
            "Thread load result is invalid".into(),
        ));
    };
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
    let settings = session.context.lock().settings.clone();
    super::projections::controls(svc, session, settings);
    let owner = svc.clone();
    let target = tab.to_string();
    let thread = thread_id.clone();
    let nonce = session.context.lock().binding_nonce.clone();
    tokio::task::spawn_blocking(move || {
        owner.finalize_runtime_worker_session(&target, &thread, &nonce)
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
    let mut cursor = None;
    let mut cursors = std::collections::HashSet::new();
    loop {
        let RuntimeResult::HistoryPage {
            thread_id: read_thread,
            items,
            next_cursor,
            open_items,
            questions,
        } = run(
            svc,
            provider,
            session,
            &process,
            RuntimeOperation::ReadHistory {
                thread_id: thread_id.clone(),
                cursor,
            },
        )
        .await?
        else {
            return Err(RuntimeFailure::HookFailed(
                "History result is invalid".into(),
            ));
        };
        if read_thread != thread_id {
            return Err(RuntimeFailure::Superseded);
        }
        if let Some(questions) = questions {
            let owner = svc.clone();
            let target = tab.to_string();
            let thread = thread_id.clone();
            let questions = tokio::task::spawn_blocking(move || {
                owner
                    .pending_delivery_payloads
                    .user_sends
                    .persist_runtime_history_questions(&target, &thread, questions)
            })
            .await
            .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?
            .map_err(RuntimeFailure::HookFailed)?;
            super::question_delivery::publish(svc, tab, session, questions)
                .map_err(RuntimeFailure::HookFailed)?;
        }
        for item in items {
            session.cache.lock().reconcile(item);
        }
        for open in open_items {
            session.cache.lock().reconcile(open.item);
            session.context.lock().active_turn_id = Some(open.turn_id);
        }
        let Some(next) = next_cursor else { break };
        if !cursors.insert(next.clone()) || cursors.len() > 256 {
            return Err(RuntimeFailure::HookFailed("History cursor limit".into()));
        }
        cursor = Some(next);
    }
    super::bootstrap::submit(svc, session, &process).await?;
    if !session.state.lock().load(context.epoch, &thread_id) {
        return Err(RuntimeFailure::Superseded);
    }
    session
        .thread_loaded
        .store(true, std::sync::atomic::Ordering::Release);
    publish(svc, tab, session);
    svc.emit_state_changed();
    Ok(())
}

pub(super) async fn plan(
    session: &Session,
    context: &RuntimeContext,
    operation: &RuntimeOperation,
) -> Result<RuntimePlan, RuntimeFailure> {
    let adapter = session
        .adapter
        .lock()
        .clone()
        .ok_or(RuntimeFailure::Superseded)?;
    tokio::time::timeout(
        RUNTIME_HOOK_DEADLINE,
        adapter.runtime_plan(context, operation),
    )
    .await
    .map_err(|_| RuntimeFailure::Deadline)?
    .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))
}

pub async fn run(
    svc: &ServiceContext,
    _provider: &str,
    session: &Arc<Session>,
    process: &Arc<Process>,
    operation: RuntimeOperation,
) -> Result<RuntimeResult, RuntimeFailure> {
    tokio::time::timeout(
        std::time::Duration::from_secs(60),
        run_inner(svc, session, process, operation),
    )
    .await
    .map_err(|_| RuntimeFailure::Deadline)?
}

async fn run_inner(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    operation: RuntimeOperation,
) -> Result<RuntimeResult, RuntimeFailure> {
    let mut context = session.context.lock().clone();
    super::model::validate_operation_thread(&context, &operation)?;
    if let RuntimeOperation::Answer { request_id, answer } = &operation {
        let requests = session.prompts.lock();
        let pending = requests.get(request_id).ok_or(RuntimeFailure::Superseded)?;
        let prompt = pending.prompt.as_ref().ok_or(RuntimeFailure::Superseded)?;
        super::model::validate_prompt_answer(prompt, answer)?;
        context.pending_request = Some(pending.request.clone());
    }
    let adapter = session
        .adapter
        .lock()
        .clone()
        .ok_or(RuntimeFailure::Superseded)?;
    let capability = adapter.agent_runtime().ok_or(RuntimeFailure::Superseded)?;
    validate_runtime_operation(&context, &operation)
        .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
    let mut next = plan(session, &context, &operation).await?;
    for _ in 0..MAX_RUNTIME_STEPS {
        validate_binding(svc, session)?;
        if session.context.lock().epoch != context.epoch {
            return Err(RuntimeFailure::Superseded);
        }
        if matches!(&operation, RuntimeOperation::Answer { .. })
            && !matches!(&next.action, RuntimeAction::Reply { .. })
        {
            return Err(RuntimeFailure::Refused(
                "Answer requires an own request reply".into(),
            ));
        }
        validate_runtime_plan(&capability, &context, &operation, &next)
            .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
        let reply = match &next.action {
            RuntimeAction::QuestionAnswer { .. } => {
                return Err(RuntimeFailure::Refused(
                    "Question formatting must enter the durable user submit door".into(),
                ))
            }
            RuntimeAction::Request { call } => {
                if !super::model::permits(&capability, &operation, &call.method) {
                    return Err(RuntimeFailure::Refused(
                        "RPC method outside declared contract".into(),
                    ));
                }
                let owner = svc.clone();
                let runtime = session.clone();
                let epoch = context.epoch;
                let captured_send = match &operation {
                    RuntimeOperation::SubmitTurn { send_id, .. } => Some(send_id.clone()),
                    _ => None,
                };
                let guard: super::rpc::PrewriteGuard = Arc::new(move || {
                    if runtime.context.lock().epoch != epoch {
                        return Err(RuntimeFailure::Superseded);
                    }
                    validate_binding(&owner, &runtime)?;
                    if let Some(send) = &captured_send {
                        let tab = runtime.context.lock().tab_id.clone();
                        super::capture::validate_if_present(&owner, &tab, send)?;
                    }
                    Ok(())
                });
                process.client.request_guarded(call, Some(guard)).await?
            }
            RuntimeAction::Notification { notification } => {
                if !super::model::permits(&capability, &operation, &notification.method) {
                    return Err(RuntimeFailure::Refused(
                        "Notification outside declared contract".into(),
                    ));
                }
                process.client.notify(notification).await?;
                Ok(Value::Null)
            }
            RuntimeAction::Reply { request_id, result } => {
                if !matches!(&operation,RuntimeOperation::Answer{request_id:expected,..} if expected==request_id)
                    || !session.prompts.lock().contains_key(request_id)
                {
                    return Err(RuntimeFailure::Refused(
                        "Reply does not belong to a pending own request".into(),
                    ));
                }
                let reply = match result {
                    RpcReply::Success { value } => Ok(value.clone()),
                    RpcReply::Error { error } => Err(error.clone()),
                };
                process.client.reply(request_id, reply).await?;
                session.prompts.lock().remove(request_id);
                Ok(Value::Null)
            }
        };
        let refused = reply.is_err();
        let reply = match reply {
            Ok(value) => RpcReply::Success { value },
            Err(error) => RpcReply::Error { error },
        };
        let step = tokio::time::timeout(
            RUNTIME_HOOK_DEADLINE,
            adapter.runtime_response(&context, &operation, &next.state, &reply),
        )
        .await
        .map_err(|_| RuntimeFailure::Deadline)?
        .map_err(|reason| RuntimeFailure::HookFailed(reason.to_string()))?;
        validate_runtime_step(&capability, &context, &operation, &reply, &step)
            .map_err(|reason| RuntimeFailure::Refused(reason.to_string()))?;
        validate_binding(svc, session)?;
        if session.context.lock().epoch != context.epoch {
            return Err(RuntimeFailure::Superseded);
        }
        match step {
            RuntimeStep::Continue {
                plan,
                before_acceptance,
            } => {
                if matches!(operation, RuntimeOperation::SubmitTurn { .. })
                    && (!refused || !before_acceptance)
                {
                    return Err(RuntimeFailure::Refused(
                        "Turn handoff cannot be replayed".into(),
                    ));
                }
                next = plan;
            }
            RuntimeStep::Refused { reason, .. } => {
                return Err(RuntimeFailure::Refused(reason.to_string()))
            }
            RuntimeStep::Complete {
                result,
                notification,
                events,
            } => {
                if let Some(notification) = notification {
                    if !super::model::permits(&capability, &operation, &notification.method) {
                        return Err(RuntimeFailure::Refused(
                            "Notification outside declared contract".into(),
                        ));
                    }
                    process.client.notify(&notification).await?;
                }
                if let Some(events) = events {
                    let at = chrono::Utc::now().timestamp_millis().max(0) as u64;
                    super::subscriber::publish_events(svc, session, context.epoch, events, at)
                        .await?;
                }
                return Ok(result);
            }
        }
    }
    Err(RuntimeFailure::HookFailed("Operation step limit".into()))
}

pub(crate) fn record_completed_turn(session: &Session, thread: &str, turn: Option<&str>) {
    if let Some(turn) = turn {
        session.completions.lock().record(thread, turn);
    }
}

pub fn validate_binding(svc: &ServiceContext, session: &Session) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    let current = svc
        .agent_registry
        .provider_arc(&session.provider)
        .ok_or(RuntimeFailure::Superseded)?;
    if session
        .adapter
        .lock()
        .as_ref()
        .is_none_or(|adapter| !Arc::ptr_eq(adapter, &current))
    {
        return Err(RuntimeFailure::Superseded);
    }
    if svc.mux.read().get_tab(&context.tab_id).is_some_and(|tab| {
        tab.agent_type.as_deref() == Some(session.provider.as_str())
            && tab.session_binding_nonce.as_deref() == Some(context.binding_nonce.as_str())
            && context
                .thread_id
                .as_deref()
                .is_none_or(|thread| tab.agent_session_id.as_deref() == Some(thread))
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
    if tab.session_binding_nonce.as_deref() != Some(context.binding_nonce.as_str())
        || tab
            .agent_session_id
            .as_deref()
            .is_some_and(|bound| bound != thread)
    {
        return Err(RuntimeFailure::Superseded);
    }
    tab.agent_session_id = Some(thread.into());
    tab.reported_session_binding_nonce = Some(context.binding_nonce);
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    if let crate::multiplexer::types::TabBacking::ChatBackend { session_id, .. } = &mut tab.backing
    {
        *session_id = thread.into();
    }
    Ok(())
}
pub fn publish(svc: &ServiceContext, tab: &str, session: &Session) {
    let context = session.context.lock().clone();
    let state = session.state.lock().clone();
    let value = serde_json::to_value(state.snapshot()).unwrap_or(Value::Null);
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
        target.agent_runtime_state = Some(value.clone());
        target.agent_booting = matches!(state.phase, RuntimePhase::Loading);
    }
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

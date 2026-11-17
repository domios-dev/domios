use super::{
    engine::{self, Session},
    model::{RuntimeFailure, RuntimePhase, RuntimeState},
};
use crate::service::MuxEmitCause;
use crate::{multiplexer::types::TabBacking, service::ServiceContext};
use std::sync::{atomic::Ordering, Arc};

pub async fn restart(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    old_nonce: &str,
    fresh_restart: Option<(&crate::service::agent_lifecycle::TabRestartSnapshot, bool)>,
    retry_refused_start: bool,
) -> Result<Arc<Session>, RuntimeFailure> {
    restart_with_account(
        svc,
        tab,
        provider,
        old_nonce,
        fresh_restart,
        retry_refused_start,
        None,
    )
    .await
}

pub async fn restart_with_account(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    old_nonce: &str,
    fresh_restart: Option<(&crate::service::agent_lifecycle::TabRestartSnapshot, bool)>,
    retry_refused_start: bool,
    account: Option<&crate::service::agent_lifecycle::AccountRestartTarget>,
) -> Result<Arc<Session>, RuntimeFailure> {
    let trigger = if retry_refused_start {
        Trigger::RetryRefused
    } else {
        Trigger::Request
    };
    let result = restart_inner(
        svc,
        tab,
        provider,
        old_nonce,
        fresh_restart,
        trigger,
        account,
    )
    .await
    .and_then(|session| session.ok_or(RuntimeFailure::Superseded));
    let changed = svc
        .mux
        .read()
        .get_tab(tab)
        .is_none_or(|target| target.session_binding_nonce.as_deref() != Some(old_nonce));
    if let Err(reason) = &result {
        if account.is_some() && !changed {
            svc.clear_runtime_restart_binding(tab);
            return result;
        }
        svc.clear_runtime_restart_binding(tab);
        let nonce = svc
            .mux
            .read()
            .get_tab(tab)
            .filter(|target| target.agent_type.as_deref() == Some(provider))
            .and_then(|target| target.session_binding_nonce.clone());
        if let Some(nonce) = nonce {
            publish_failure(svc, tab, &nonce, engine::for_tab(tab).as_ref(), reason);
        }
    }
    result
}

pub async fn restart_idle(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    nonce: &str,
) -> Result<Option<Arc<Session>>, RuntimeFailure> {
    restart_inner(svc, tab, provider, nonce, None, Trigger::IdleUpdate, None)
        .await
        .inspect_err(|_| svc.clear_runtime_restart_binding(tab))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Trigger {
    IdleUpdate,
    Request,
    /// Only this trigger retires a retained provider refusal, so it relaunches.
    RetryRefused,
}

async fn restart_inner(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    old_nonce: &str,
    fresh_restart: Option<(&crate::service::agent_lifecycle::TabRestartSnapshot, bool)>,
    trigger: Trigger,
    account: Option<&crate::service::agent_lifecycle::AccountRestartTarget>,
) -> Result<Option<Arc<Session>>, RuntimeFailure> {
    let idle_only = trigger == Trigger::IdleUpdate;
    let session = engine::for_tab(tab);
    let _permit = match &session {
        Some(session) => Some(
            tokio::time::timeout(std::time::Duration::from_secs(30), session.gate.acquire())
                .await
                .map_err(|_| RuntimeFailure::Deadline)?
                .map_err(|_| RuntimeFailure::Superseded)?,
        ),
        None => None,
    };
    let fresh = fresh_restart.is_some();
    if account.is_some() && (fresh || trigger != Trigger::Request) {
        return Err(RuntimeFailure::Refused(
            "Account restart cannot start a fresh conversation".into(),
        ));
    }
    if let Some((snapshot, force)) = fresh_restart {
        svc.lifecycle_port()
            .require_fresh_restart_allowed(snapshot, force)
            .map_err(|_| {
                RuntimeFailure::Refused(
                    "Fresh restart is no longer allowed for the current session".into(),
                )
            })?;
    }
    if let Some(session) = &session {
        if idle_only {
            engine::validate_binding(svc, session)?;
        } else if !fresh
            || !svc
                .mux
                .read()
                .get_tab(tab)
                .is_some_and(super::start_failure::for_tab)
        {
            engine::validate_restart_binding(svc, session)?;
        }
        if idle_only
            && (super::update::busy(session)
                || !crate::service::background_work::running(svc, tab).is_empty())
        {
            return Ok(None);
        }
    }
    let account_process = account
        .map(|account| {
            validate_account_restart(svc, tab, provider, old_nonce, session.as_ref(), account)
        })
        .transpose()?
        .flatten();
    if let Some(session) = &session {
        super::turn_requests::end_for_lifecycle(
            svc,
            session,
            "Runtime restarted before the turn completion reply",
        )
        .await;
    }
    let old_thread = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|target| target.agent_session_id.clone());
    let rebound = session
        .as_ref()
        .is_some_and(|session| session.context.lock().thread_id != old_thread);
    let binding = svc
        .authorize_runtime_restart_binding(tab, provider, old_nonce, fresh)
        .map_err(RuntimeFailure::Refused)?;
    let environment = match account {
        Some(target) => super::environment::compose_for_account_restart(
            svc,
            tab,
            provider,
            old_nonce,
            target.invocation.as_deref(),
        ),
        None => super::environment::compose(svc, tab, provider, old_nonce),
    }
    .map_err(|reason| RuntimeFailure::SpawnFailed { reason })?;
    for status in svc
        .chat_send_ledger
        .rpc_uncertain_for_launch(tab, Some(old_nonce))
    {
        svc.ui.emit(
            "chat-send-status",
            serde_json::to_value(status).unwrap_or_default(),
        );
    }
    svc.validate_completed_restart_binding(tab, binding.as_ref())
        .map_err(RuntimeFailure::Refused)?;
    let nonce = super::authority::rotate_restart(svc, tab, provider, old_nonce)
        .map_err(RuntimeFailure::Refused)?;
    svc.stage_runtime_restart_binding(tab, binding);
    let mut bridge_retired = Ok(());
    if let Some(session) = &session {
        // The client bridge is found by the old epoch and nonce, so it retires (its pending
        // requests answered, terminals killed) before the context moves to the new epoch.
        let process = session.process.lock().clone();
        if let Some(process) = process {
            bridge_retired = super::client_bridge::cancel(svc, session, &process, true).await;
        }
        if fresh {
            // The archive is read once the thread is gone, so it must hold the settled rows now.
            super::subscriber::settle_replaced_session(svc, session);
            if !session.cache.lock().tail(1).is_empty() {
                super::transcript::persist(session).await;
            }
        }
        let context = session.context.lock().clone();
        super::prompt_answers::clear(svc, &context);
        let epoch = session.state.lock().begin();
        {
            let mut context = session.context.lock();
            context.epoch = epoch;
            if !rebound {
                context.binding_nonce = nonce.clone();
            }
            if fresh {
                context.thread_id = None;
            }
        }
        super::declared_controls::reset(session, epoch);
        {
            let mut state = session.state.lock();
            state.failure_code = None;
            state.phase = RuntimePhase::Unloaded { reason: None };
            if fresh {
                state.thread_id = None;
                state.active_turn_started_at_ms = None;
            }
        }
        session.thread_loaded.store(false, Ordering::Release);
        *session.compaction.lock() = Default::default();
        session.prompts.lock().clear();
        if fresh {
            *session.cache.lock() = Default::default();
        }
    }
    {
        let mut mux = svc.mux.write();
        let target = mux.get_tab_mut(tab).ok_or(RuntimeFailure::Superseded)?;
        if target.agent_type.as_deref() != Some(provider)
            || target.session_binding_nonce.as_deref() != Some(nonce.as_str())
            || target.agent_session_id != old_thread
        {
            return Err(RuntimeFailure::Superseded);
        }
        target.agent_booting = true;
        target.boot_stage = None;
        // Without a live session the tab's retained state is all the launch guard reads.
        if rebound || (session.is_none() && (fresh || trigger == Trigger::RetryRefused)) {
            let mut state = RuntimeState::unloaded(old_thread.clone());
            state.restore_timing(target.agent_runtime_state.as_ref());
            if let Some(session) = &session {
                state.epoch = state.epoch.max(session.state.lock().epoch);
            }
            state.active_turn_started_at_ms = None;
            state.last_turn_duration_ms = None;
            target.agent_runtime_state = serde_json::to_value(state.snapshot()).ok();
        }
        if fresh {
            if let Some(thread) = &old_thread {
                target.archive_context_epoch(provider, thread);
            }
            target.agent_session_id = None;
            target.agent_session_name = None;
            target.restored_from_snapshot = false;
            target.agent_state = Some(codeterm_types::pty::AgentLiveState::from(
                codeterm_types::pty::TabVerdict {
                    state: codeterm_types::pty::TimelineState::Idle,
                    subtype: None,
                },
            ));
            if let Some(restore) = target.close_restore.as_mut() {
                restore.exact_session = false;
            }
            if let TabBacking::ChatBackend { session_id, .. } = &mut target.backing {
                session_id.clear();
            }
        }
        if let Some(options) = target.restorable_launch.runtime_launch_options.as_mut() {
            options.binding_nonce = nonce.clone();
        }
    }
    if let Some(session) = &session {
        engine::publish(svc, tab, session);
    }
    svc.emit_state_changed(MuxEmitCause::TabLifecycle);
    let result = async {
        svc.prepare_agent_runtime_launch_env(tab, &environment)
            .map_err(RuntimeFailure::Refused)?;
        if let Some(session) = &session {
            let process = account_process
                .clone()
                .or_else(|| session.process.lock().clone());
            if let Some(process) = process {
                process.finish().await?;
            }
            session.process.lock().take();
        }
        bridge_retired?;
        if let Some(account) = account {
            let mut mux = svc.mux.write();
            let target = mux.get_tab_mut(tab).ok_or(RuntimeFailure::Superseded)?;
            if target.agent_type.as_deref() != Some(provider)
                || target.agent_session_id != old_thread
                || target.session_binding_nonce.as_deref() != Some(nonce.as_str())
            {
                return Err(RuntimeFailure::Superseded);
            }
            account
                .commit_selection(target)
                .map_err(RuntimeFailure::Refused)?;
        }
        Ok(())
    }
    .await;
    drop(_permit);
    let loaded = match result {
        Ok(()) => {
            let result = match account {
                Some(target) => {
                    engine::ensure_with_invocation(svc, tab, target.invocation.clone()).await
                }
                None => engine::ensure(svc, tab).await,
            };
            result.or_else(|error| engine::login_pending_session(svc, tab, error))?
        }
        Err(reason) => return Err(reason),
    };
    if fresh && old_thread.is_some() && loaded.context.lock().thread_id == old_thread {
        return Err(RuntimeFailure::Refused(
            "Fresh runtime returned the previous thread".into(),
        ));
    }
    if !fresh && old_thread.is_some() && loaded.context.lock().thread_id != old_thread {
        return Err(RuntimeFailure::Superseded);
    }
    Ok(Some(loaded))
}

fn publish_failure(
    svc: &ServiceContext,
    tab: &str,
    nonce: &str,
    session: Option<&Arc<Session>>,
    reason: &RuntimeFailure,
) {
    if let Some(session) = session.filter(|session| session.context.lock().binding_nonce == nonce) {
        let epoch = session.state.lock().epoch;
        session.state.lock().degrade(epoch, reason);
        session.thread_loaded.store(false, Ordering::Release);
        engine::publish(svc, tab, session);
    } else {
        let mut mux = svc.mux.write();
        if let Some(target) = mux
            .get_tab_mut(tab)
            .filter(|target| target.session_binding_nonce.as_deref() == Some(nonce))
        {
            let mut state = RuntimeState::unloaded(target.agent_session_id.clone());
            state.restore_timing(target.agent_runtime_state.as_ref());
            state.degrade(state.epoch, reason);
            target.agent_runtime_state = serde_json::to_value(state.snapshot()).ok();
            target.agent_booting = false;
        }
    }
    {
        let mut mux = svc.mux.write();
        if let Some(target) = mux
            .get_tab_mut(tab)
            .filter(|target| target.session_binding_nonce.as_deref() == Some(nonce))
        {
            target.boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
        }
    }
    svc.emit_state_changed(MuxEmitCause::TabLifecycle);
}

fn validate_account_restart(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    nonce: &str,
    session: Option<&Arc<Session>>,
    account: &crate::service::agent_lifecycle::AccountRestartTarget,
) -> Result<Option<Arc<super::process::Process>>, RuntimeFailure> {
    account.validate_target().map_err(RuntimeFailure::Refused)?;
    let session = session.ok_or(RuntimeFailure::Superseded)?;
    let epoch = session.state.lock().epoch;
    let context = session.context.lock().clone();
    let unbound = account.expected.unbound_login;
    let thread_matches = if unbound {
        context.thread_id.is_none()
    } else {
        context.thread_id.as_deref() == Some(account.expected.session_id.as_str())
    };
    if account.expected.provider != provider
        || account.expected.binding_nonce != nonce
        || account.expected.runtime_epoch != Some(epoch)
        || context.epoch != epoch
        || !thread_matches
        || (!unbound && session.process.lock().is_none())
        || super::update::busy(session)
        || !session.turn_requests.lock().is_empty()
    {
        return Err(RuntimeFailure::Refused(
            "Account restart requires the exact idle runtime epoch".into(),
        ));
    }
    let pending_delivery = {
        let mux = svc.mux.read();
        let target = mux.get_tab(tab).ok_or(RuntimeFailure::Superseded)?;
        account
            .expected
            .validate(target, Some(epoch))
            .map_err(RuntimeFailure::Refused)?;
        target.pending_delivery.is_some()
    };
    if unbound {
        let login_pending = {
            let state = session.state.lock();
            matches!(state.phase, RuntimePhase::LoginPending) && state.thread_id.is_none()
        };
        if !login_pending
            || context.pending_request.is_some()
            || !context.claimed_sends.is_empty()
            || !session.prompts.lock().is_empty()
            || !session.cache.lock().tail(1).is_empty()
            || session.thread_loaded.load(Ordering::Acquire)
            || pending_delivery
            || svc.chat_send_ledger.has_records_for_tab(tab)
        {
            return Err(RuntimeFailure::Refused(
                "Unbound account restart requires an empty sign-in state with no delivery records"
                    .into(),
            ));
        }
    }
    let process = session.process.lock().clone();
    if !unbound && process.is_none() {
        return Err(RuntimeFailure::Superseded);
    }
    Ok(process)
}

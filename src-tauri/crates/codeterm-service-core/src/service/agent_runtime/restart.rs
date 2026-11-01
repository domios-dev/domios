use super::{
    engine::{self, Session},
    model::{RuntimeFailure, RuntimePhase, RuntimeState},
};
use crate::service::MuxEmitCause;
use crate::{multiplexer::types::TabBacking, service::ServiceContext};
use std::sync::{atomic::Ordering, Arc};

pub(crate) async fn restart(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    old_nonce: &str,
    fresh: bool,
) -> Result<Arc<Session>, RuntimeFailure> {
    let result = restart_inner(svc, tab, provider, old_nonce, fresh, false)
        .await
        .and_then(|session| session.ok_or(RuntimeFailure::Superseded));
    if let Err(reason) = &result {
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

pub(crate) async fn restart_idle(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    nonce: &str,
) -> Result<Option<Arc<Session>>, RuntimeFailure> {
    restart_inner(svc, tab, provider, nonce, false, true)
        .await
        .inspect_err(|_| svc.clear_runtime_restart_binding(tab))
}

async fn restart_inner(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    old_nonce: &str,
    fresh: bool,
    idle_only: bool,
) -> Result<Option<Arc<Session>>, RuntimeFailure> {
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
    if let Some(session) = &session {
        if idle_only {
            engine::validate_binding(svc, session)?;
        } else {
            engine::validate_restart_binding(svc, session)?;
        }
        if idle_only
            && (super::update::busy(session)
                || !crate::service::background_work::running(svc, tab).is_empty())
        {
            return Ok(None);
        }
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
    let environment = super::environment::compose(svc, tab, provider, old_nonce);
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
    if let Some(session) = &session {
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
        {
            let mut state = session.state.lock();
            state.phase = RuntimePhase::Unloaded { reason: None };
            if fresh {
                state.thread_id = None;
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
        if rebound {
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
            target.agent_session_id = None;
            target.agent_session_name = None;
            target.restored_from_snapshot = false;
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
            let process = session.process.lock().clone();
            if let Some(process) = process {
                process.finish().await?;
            }
            session.process.lock().take();
        }
        Ok(())
    }
    .await;
    drop(_permit);
    let loaded = match result {
        Ok(()) => engine::ensure(svc, tab)
            .await
            .or_else(|error| engine::login_pending_session(svc, tab, error))?,
        Err(reason) => return Err(reason),
    };
    if fresh && old_thread.is_some() && loaded.context.lock().thread_id == old_thread {
        return Err(RuntimeFailure::Refused(
            "Fresh runtime returned the previous thread".into(),
        ));
    }
    if !fresh && loaded.context.lock().thread_id != old_thread {
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

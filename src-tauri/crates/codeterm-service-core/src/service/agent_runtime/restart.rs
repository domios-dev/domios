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
    restart_with_selection(
        svc,
        tab,
        provider,
        old_nonce,
        fresh_restart,
        retry_refused_start,
        None,
    )
    .await
    .session
}

#[derive(PartialEq, Eq)]
pub struct ModelSelection {
    pub provider: String,
    pub binding_nonce: String,
    pub thread_id: Option<String>,
    pub model: String,
    pub effort: Option<String>,
}

pub enum RestartSelection<'a> {
    Account(&'a crate::service::agent_lifecycle::AccountRestartTarget),
    Model(&'a ModelSelection),
}

pub struct AccountRestartResult {
    pub session: Result<Arc<Session>, RuntimeFailure>,
    pub target_refusal: Option<String>,
    pub source_account_restored: bool,
    pub account_refusal_nonce: Option<String>,
}

pub async fn restart_with_selection(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    old_nonce: &str,
    fresh_restart: Option<(&crate::service::agent_lifecycle::TabRestartSnapshot, bool)>,
    retry_refused_start: bool,
    requested: Option<RestartSelection<'_>>,
) -> AccountRestartResult {
    let (account, selection) = match requested {
        Some(RestartSelection::Account(account)) => (Some(account), None),
        Some(RestartSelection::Model(model)) => (None, Some(model)),
        None => (None, None),
    };
    let trigger = if let Some(selection) = selection {
        Trigger::Model(selection)
    } else if retry_refused_start {
        Trigger::RetryRefused
    } else {
        Trigger::Request
    };
    let mut source = None;
    let mut result = restart_inner(
        svc,
        tab,
        provider,
        old_nonce,
        fresh_restart,
        trigger,
        account.map(|target| AccountTransition::Requested {
            target,
            source: &mut source,
        }),
    )
    .await
    .and_then(|session| session.ok_or(RuntimeFailure::Superseded));
    let account_refusal_nonce = account.and_then(|target| {
        result
            .as_ref()
            .err()
            .and_then(|reason| authenticated_target_refusal(svc, tab, target, reason))
    });
    let target_refusal = account_refusal_nonce
        .as_ref()
        .and_then(|_| result.as_ref().err().map(ToString::to_string));
    let mut source_account_restored = false;
    if target_refusal.is_some() {
        if let (Some(source), Some(target)) = (source, account) {
            match RollbackWitness::capture(svc, tab, source, target) {
                Ok((witness, reverse)) => {
                    let rollback = restart_inner(
                        svc,
                        tab,
                        provider,
                        &reverse.expected.binding_nonce,
                        None,
                        Trigger::Request,
                        Some(AccountTransition::Rollback {
                            target: &reverse,
                            witness: &witness,
                        }),
                    )
                    .await
                    .and_then(|session| session.ok_or(RuntimeFailure::Superseded));
                    match rollback {
                        Ok(session)
                            if session.thread_loaded.load(Ordering::Acquire)
                                && matches!(session.state.lock().phase, RuntimePhase::Ready) =>
                        {
                            source_account_restored = true;
                            result = Ok(session);
                        }
                        Ok(_) => {
                            result = Err(RuntimeFailure::Refused(
                                "The original account did not restore its retained conversation"
                                    .into(),
                            ))
                        }
                        Err(reason) => {
                            log::debug!(
                                "[account-restart] original account restore refused: {reason}"
                            );
                            if svc.mux.read().get_tab(tab).is_none_or(|current| {
                                current.session_binding_nonce.as_deref()
                                    != Some(reverse.expected.binding_nonce.as_str())
                            }) {
                                result = Err(reason);
                            }
                        }
                    }
                }
                Err(reason) => {
                    log::debug!("[account-restart] rollback admission refused: {reason}")
                }
            }
        }
    }
    let changed = svc
        .mux
        .read()
        .get_tab(tab)
        .is_none_or(|target| target.session_binding_nonce.as_deref() != Some(old_nonce));
    if let Err(reason) = &result {
        if (account.is_some() || selection.is_some()) && !changed {
            svc.clear_runtime_restart_binding(tab);
            return AccountRestartResult {
                session: result,
                target_refusal,
                source_account_restored,
                account_refusal_nonce,
            };
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
    AccountRestartResult {
        session: result,
        target_refusal,
        source_account_restored,
        account_refusal_nonce,
    }
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
enum Trigger<'a> {
    Model(&'a ModelSelection),
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
    trigger: Trigger<'_>,
    mut transition: Option<AccountTransition<'_>>,
) -> Result<Option<Arc<Session>>, RuntimeFailure> {
    let selection = match trigger {
        Trigger::Model(selection) => Some(selection),
        _ => None,
    };
    let account = transition.as_ref().map(AccountTransition::target);
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
    if let Some(selection) = selection {
        let mux = svc.mux.read();
        let target = mux.get_tab(tab).ok_or(RuntimeFailure::Superseded)?;
        if selection.provider != provider
            || selection.binding_nonce != old_nonce
            || target.agent_type.as_deref() != Some(selection.provider.as_str())
            || target.session_binding_nonce.as_deref() != Some(selection.binding_nonce.as_str())
            || target.agent_session_id != selection.thread_id
            || target
                .agent_runtime_config
                .as_ref()
                .is_none_or(|config| !config.is_object())
        {
            return Err(RuntimeFailure::Superseded);
        }
    }
    if selection.is_some() && (fresh_restart.is_some() || transition.is_some()) {
        return Err(RuntimeFailure::Refused(
            "Model selection requires a same-session restart".into(),
        ));
    }
    if selection.is_some() && session.as_deref().is_some_and(model_restart_busy) {
        return Err(RuntimeFailure::Refused(
            "RestartResume requires an idle runtime".into(),
        ));
    }
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
    let account_process = match transition.as_mut() {
        Some(AccountTransition::Requested { target, source }) => {
            let process =
                validate_account_restart(svc, tab, provider, old_nonce, session.as_ref(), target)?;
            if !target.expected.unbound_login {
                match SourceAccountRestore::capture(svc, tab, target, session.as_ref()) {
                    Ok(captured) => **source = Some(captured),
                    Err(reason) => log::debug!(
                        "[account-restart] source cannot authorize compensation: {reason}"
                    ),
                }
            }
            process
        }
        Some(AccountTransition::Rollback { target, witness }) => {
            Some(witness.validate(svc, tab, old_nonce, session.as_ref(), target)?)
        }
        None => None,
    };
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
    let binding = match transition.as_ref() {
        Some(AccountTransition::Rollback { witness, .. }) => witness.source.restart_binding.clone(),
        _ => svc
            .authorize_runtime_restart_binding(tab, provider, old_nonce, fresh)
            .map_err(RuntimeFailure::Refused)?,
    };
    if let Some(AccountTransition::Requested { source, .. }) = transition.as_mut() {
        if let Some(source) = source.as_mut() {
            source.restart_binding = binding.clone();
        }
    }
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
    if let Some(AccountTransition::Rollback { witness, .. }) = transition.as_ref() {
        if !witness.source.unchanged(svc, tab) {
            return Err(RuntimeFailure::Refused(
                "Account rollback input authority changed before teardown".into(),
            ));
        }
    }
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
        if let Some(selection) = selection {
            let config = target
                .agent_runtime_config
                .as_mut()
                .and_then(serde_json::Value::as_object_mut)
                .ok_or_else(|| RuntimeFailure::Refused("Runtime configuration missing".into()))?;
            config.insert("model".into(), serde_json::json!(selection.model));
            if let Some(effort) = &selection.effort {
                config.insert("effort".into(), serde_json::json!(effort));
            }
            target.restorable_launch.model = Some(selection.model.clone());
            target.restorable_launch.requested_model = Some(selection.model.clone());
            if let Some(effort) = &selection.effort {
                target.restorable_launch.requested_reasoning_effort = Some(effort.clone());
                target.restorable_launch.resolved_reasoning_effort = Some(effort.clone());
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
        if let Some(AccountTransition::Rollback { witness, .. }) = transition.as_ref() {
            if !witness.source.unchanged(svc, tab) {
                return Err(RuntimeFailure::Refused(
                    "Account rollback input authority changed during cleanup".into(),
                ));
            }
        }
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

pub fn model_restart_busy(session: &Session) -> bool {
    super::update::busy(session)
        || !session.prompts.lock().is_empty()
        || !session.turn_requests.lock().is_empty()
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
    let refresh = account.expected.refresh_login;
    if refresh {
        account
            .expected
            .validate_refresh_authority(svc, tab, session)
            .map_err(RuntimeFailure::Refused)?;
    }
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
        || (!unbound && !refresh && session.process.lock().is_none())
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
    if refresh
        && (context.pending_request.is_some()
            || !context.claimed_sends.is_empty()
            || !session.prompts.lock().is_empty())
    {
        return Err(RuntimeFailure::Refused(
            "Credential refresh cannot replace active or claimed work".into(),
        ));
    }
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
    if !unbound && !refresh && process.is_none() {
        return Err(RuntimeFailure::Superseded);
    }
    Ok(process)
}

enum AccountTransition<'a> {
    Requested {
        target: &'a crate::service::agent_lifecycle::AccountRestartTarget,
        source: &'a mut Option<SourceAccountRestore>,
    },
    Rollback {
        target: &'a crate::service::agent_lifecycle::AccountRestartTarget,
        witness: &'a RollbackWitness,
    },
}

impl<'a> AccountTransition<'a> {
    fn target(&self) -> &'a crate::service::agent_lifecycle::AccountRestartTarget {
        match self {
            Self::Requested { target, .. } | Self::Rollback { target, .. } => target,
        }
    }
}

struct SourceAccountRestore {
    owner: ServiceContext,
    expected: crate::service::agent_lifecycle::AccountRestartBinding,
    invocation: Option<Arc<codeterm_plugin::provider_account::InvocationContext>>,
    input_revision: (String, u64, u64),
    pending_delivery: Option<crate::multiplexer::types::PendingDeliveryRecord>,
    _reservation: crate::service::provider_accounts::admission::Reservation,
    restart_binding:
        Option<crate::service::agent_lifecycle::worker_binding::RestartBindingAuthority>,
}

impl SourceAccountRestore {
    fn capture(
        svc: &ServiceContext,
        tab: &str,
        target: &crate::service::agent_lifecycle::AccountRestartTarget,
        session: Option<&Arc<Session>>,
    ) -> Result<Self, RuntimeFailure> {
        let session = session.ok_or(RuntimeFailure::Superseded)?;
        let reservation = crate::service::provider_accounts::admission::reserve(
            svc,
            &target.expected.provider,
            target.expected.account_id.as_deref(),
        )
        .map_err(RuntimeFailure::Refused)?;
        let pending_delivery = svc
            .mux
            .read()
            .get_tab(tab)
            .ok_or(RuntimeFailure::Superseded)?
            .pending_delivery
            .clone();
        Ok(Self {
            owner: svc.clone(),
            expected: target.expected.clone(),
            invocation: session.invocation.lock().clone(),
            input_revision: svc.chat_send_ledger.input_revision_for_tab(tab),
            pending_delivery,
            _reservation: reservation,
            restart_binding: None,
        })
    }

    fn unchanged(&self, svc: &ServiceContext, tab: &str) -> bool {
        Arc::ptr_eq(&self.owner.mux, &svc.mux)
            && Arc::ptr_eq(&self.owner.chat_send_ledger, &svc.chat_send_ledger)
            && self.input_revision == svc.chat_send_ledger.input_revision_for_tab(tab)
            && svc
                .mux
                .read()
                .get_tab(tab)
                .is_some_and(|target| target.pending_delivery == self.pending_delivery)
    }
}

fn authenticated_target_refusal(
    svc: &ServiceContext,
    tab: &str,
    target: &crate::service::agent_lifecycle::AccountRestartTarget,
    reason: &RuntimeFailure,
) -> Option<String> {
    use codeterm_plugin::agent_runtime::{RuntimeAuthState, RuntimeFailureCode};
    if !matches!(reason, RuntimeFailure::SessionLoadRefused(_)) {
        return None;
    }
    let Some(session) = engine::for_tab(tab) else {
        return None;
    };
    let context = session.context.lock().clone();
    let state_refused =
        session.state.lock().failure_code == Some(RuntimeFailureCode::SessionLoadRefused);
    let refused = state_refused
        && *session.auth.lock() == RuntimeAuthState::SignedIn
        && context.thread_id.as_deref() == Some(target.expected.session_id.as_str())
        && svc.mux.read().get_tab(tab).is_some_and(|current| {
            current.agent_type.as_deref() == Some(target.expected.provider.as_str())
                && current.agent_session_id.as_deref() == Some(target.expected.session_id.as_str())
                && current.session_binding_nonce.as_deref() == Some(context.binding_nonce.as_str())
                && current.restorable_launch.account_id == target.account_id
                && current.restorable_launch.account_namespace == target.account_namespace
        });
    refused.then_some(context.binding_nonce)
}

struct RollbackWitness {
    source: SourceAccountRestore,
    failed: crate::service::agent_lifecycle::AccountRestartBinding,
    session: Arc<Session>,
    process: Arc<super::process::Process>,
}

impl RollbackWitness {
    fn capture(
        svc: &ServiceContext,
        tab: &str,
        source: SourceAccountRestore,
        target: &crate::service::agent_lifecycle::AccountRestartTarget,
    ) -> Result<(Self, crate::service::agent_lifecycle::AccountRestartTarget), RuntimeFailure> {
        let session = engine::for_tab(tab).ok_or(RuntimeFailure::Superseded)?;
        let context = session.context.lock().clone();
        let failed = {
            let mux = svc.mux.read();
            let current = mux.get_tab(tab).ok_or(RuntimeFailure::Superseded)?;
            if current.restorable_launch.account_id != target.account_id
                || current.restorable_launch.account_namespace != target.account_namespace
            {
                return Err(RuntimeFailure::Superseded);
            }
            crate::service::agent_lifecycle::AccountRestartBinding {
                provider: target.expected.provider.clone(),
                account_id: current.restorable_launch.account_id.clone(),
                account_namespace: current.restorable_launch.account_namespace.clone(),
                binding_nonce: context.binding_nonce.clone(),
                session_id: source.expected.session_id.clone(),
                runtime_epoch: Some(context.epoch),
                lifecycle_epoch: current.lifecycle_epoch,
                unbound_login: false,
                refresh_login: false,
            }
        };
        let process = session
            .process
            .lock()
            .clone()
            .ok_or(RuntimeFailure::Superseded)?;
        let reverse = crate::service::agent_lifecycle::AccountRestartTarget {
            expected: failed.clone(),
            account_id: source.expected.account_id.clone(),
            account_namespace: source.expected.account_namespace.clone(),
            invocation: source.invocation.clone(),
            terminal_turn: None,
        };
        let witness = Self {
            source,
            failed,
            session,
            process,
        };
        witness.validate(
            svc,
            tab,
            &reverse.expected.binding_nonce,
            Some(&witness.session),
            &reverse,
        )?;
        Ok((witness, reverse))
    }

    fn validate(
        &self,
        svc: &ServiceContext,
        tab: &str,
        nonce: &str,
        session: Option<&Arc<Session>>,
        target: &crate::service::agent_lifecycle::AccountRestartTarget,
    ) -> Result<Arc<super::process::Process>, RuntimeFailure> {
        use codeterm_plugin::agent_runtime::{RuntimeAuthState, RuntimeFailureCode};
        let session = session.ok_or(RuntimeFailure::Superseded)?;
        if !Arc::ptr_eq(session, &self.session)
            || !engine::for_tab(tab).is_some_and(|installed| Arc::ptr_eq(&installed, session))
            || !self.source.unchanged(svc, tab)
            || target.expected != self.failed
            || target.account_id != self.source.expected.account_id
            || target.account_namespace != self.source.expected.account_namespace
            || target
                .invocation
                .as_ref()
                .map(|context| context.cache_key())
                != self
                    .source
                    .invocation
                    .as_ref()
                    .map(|context| context.cache_key())
            || nonce != self.failed.binding_nonce
            || nonce == self.source.expected.binding_nonce
        {
            return Err(RuntimeFailure::Refused(
                "Account rollback source or input authority changed".into(),
            ));
        }
        target.validate_target().map_err(RuntimeFailure::Refused)?;
        engine::validate_binding(svc, session)?;
        if !super::authority::verifies(svc, tab, &self.failed.provider, nonce) {
            return Err(RuntimeFailure::Superseded);
        }
        let context = session.context.lock().clone();
        let failed_epoch = {
            let state = session.state.lock();
            state.failure_code == Some(RuntimeFailureCode::SessionLoadRefused)
                && matches!(state.phase, RuntimePhase::Degraded { .. })
                && Some(state.epoch) == self.failed.runtime_epoch
                && state.thread_id.as_deref() == Some(self.failed.session_id.as_str())
        };
        if !failed_epoch
            || *session.auth.lock() != RuntimeAuthState::SignedIn
            || session.thread_loaded.load(Ordering::Acquire)
            || context.binding_nonce != nonce
            || Some(context.epoch) != self.failed.runtime_epoch
            || context.thread_id.as_deref() != Some(self.failed.session_id.as_str())
            || context.active_turn_id.is_some()
            || !context.active_turns.is_empty()
            || context.pending_request.is_some()
            || !context.claimed_sends.is_empty()
            || !session.turn_requests.lock().is_empty()
            || !session.prompts.lock().is_empty()
            || !session
                .process
                .lock()
                .as_ref()
                .is_some_and(|process| Arc::ptr_eq(process, &self.process))
        {
            return Err(RuntimeFailure::Refused(
                "Account rollback requires the exact authenticated unloaded refusal".into(),
            ));
        }
        let mux = svc.mux.read();
        let current = mux.get_tab(tab).ok_or(RuntimeFailure::Superseded)?;
        self.failed
            .validate_selection(current)
            .map_err(RuntimeFailure::Refused)?;
        if current.agent_type.as_deref() != Some(self.failed.provider.as_str())
            || current.agent_session_id.as_deref() != Some(self.failed.session_id.as_str())
            || current.session_binding_nonce.as_deref() != Some(nonce)
            || current.reported_session_binding_nonce.is_some()
        {
            return Err(RuntimeFailure::Superseded);
        }
        Ok(self.process.clone())
    }
}

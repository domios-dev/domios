use super::*;
use crate::multiplexer::types::Tab;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct ExpectedEnsureOutcome {
    result: Result<Arc<Session>, RuntimeFailure>,
    observation: Option<Arc<ExpectedAcquisition>>,
    issued: bool,
}

pub(crate) struct RejectedBeforeAcquisition {
    call: Arc<ExpectedAcquisition>,
}

pub(super) struct ExpectedAcquisition {
    owner:
        std::sync::Weak<crate::lock_hold::TimedRwLock<crate::multiplexer::state::MultiplexerState>>,
    original: Tab,
    fork: Option<(String, String)>,
    generation: Arc<super::super::authority::NativeGeneration>,
    acquired: AtomicBool,
    settled: AtomicBool,
}

impl ExpectedAcquisition {
    pub(super) fn mark_acquired(&self) {
        self.acquired.store(true, Ordering::Release);
    }
    fn can_reject(&self) -> bool {
        self.settled.load(Ordering::Acquire)
            && !self.acquired.load(Ordering::Acquire)
            && self.generation.unacquired()
    }
}

impl ExpectedEnsureOutcome {
    pub(crate) fn into_result(self) -> Result<Arc<Session>, RuntimeFailure> {
        self.result
    }
    pub(crate) fn take_rejection(&mut self) -> Option<RejectedBeforeAcquisition> {
        if self.issued {
            return None;
        }
        self.issued = true;
        self.result
            .is_err()
            .then(|| {
                self.observation
                    .as_ref()
                    .filter(|call| call.can_reject())
                    .map(|call| RejectedBeforeAcquisition { call: call.clone() })
            })
            .flatten()
    }
}

impl RejectedBeforeAcquisition {
    pub(crate) fn matches_original(
        self,
        outcome: &ExpectedEnsureOutcome,
        svc: &ServiceContext,
        original: &Tab,
        fork: Option<(&str, &str)>,
    ) -> bool {
        outcome.result.is_err()
            && outcome
                .observation
                .as_ref()
                .is_some_and(|call| Arc::ptr_eq(call, &self.call))
            && self.call.can_reject()
            && self
                .call
                .owner
                .upgrade()
                .is_some_and(|owner| Arc::ptr_eq(&owner, &svc.mux))
            && crate::service::messenger::switch_handoff::same_binding(
                &self.call.original,
                original,
            )
            && self
                .call
                .fork
                .as_ref()
                .map(|(thread, turn)| (thread.as_str(), turn.as_str()))
                == fork
    }
}

fn capture_acquisition(
    svc: &ServiceContext,
    tab: &str,
    nonce: &str,
    fork: Option<(&str, &str)>,
) -> Option<Arc<ExpectedAcquisition>> {
    let mux = svc.mux.read();
    let original = mux.get_tab(tab).filter(|tab| {
        tab.session_binding_nonce.as_deref() == Some(nonce)
            && !tab.restored_from_snapshot
            && !tab.frozen
            && tab.lifecycle_epoch.is_some()
            && tab.lifecycle_operation_id.is_some()
    })?;
    let sessions = SESSIONS.lock();
    let generation = super::super::authority::generation(svc, original)?;
    if let Some(session) = sessions.get(tab) {
        if session.context.lock().binding_nonce == nonce {
            generation.mark_acquired();
        }
        return None;
    }
    if !generation.unacquired() {
        return None;
    }
    Some(Arc::new(ExpectedAcquisition {
        owner: Arc::downgrade(&svc.mux),
        original: original.clone(),
        fork: fork.map(|(thread, turn)| (thread.into(), turn.into())),
        generation,
        acquired: AtomicBool::new(false),
        settled: AtomicBool::new(false),
    }))
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) struct ExpectedLaunchCommit {
    pub stage: &'static str,
    pub callback: Mutex<Option<Box<dyn FnOnce(&ServiceContext) + Send>>>,
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn expected_launch_commit(svc: &ServiceContext, stage: &str) {
    if let Some(boundary) = svc
        .try_ext::<super::ExpectedLaunchCommit>()
        .filter(|boundary| boundary.stage == stage)
    {
        let callback = boundary.callback.lock().take();
        if let Some(callback) = callback {
            callback(svc);
        }
    }
}

pub(super) async fn ensure(
    svc: &ServiceContext,
    tab: &str,
    fork: Option<(String, String)>,
) -> Result<Arc<Session>, RuntimeFailure> {
    ensure_account(svc, tab, fork, None, None).await
}

#[cfg(test)]
pub(crate) async fn ensure_expected(
    svc: &ServiceContext,
    tab: &str,
    expected_nonce: &str,
    fork: Option<(&str, &str)>,
) -> Result<Arc<Session>, RuntimeFailure> {
    super::ensure_expected_observed(svc, tab, expected_nonce, fork)
        .await
        .into_result()
}

pub(crate) async fn ensure_expected_observed(
    svc: &ServiceContext,
    tab: &str,
    expected_nonce: &str,
    fork: Option<(&str, &str)>,
) -> ExpectedEnsureOutcome {
    let observation = capture_acquisition(svc, tab, expected_nonce, fork);
    let result = ensure_account(
        svc,
        tab,
        fork.map(|(thread, turn)| (thread.into(), turn.into())),
        Some(expected_nonce.into()),
        observation.clone(),
    )
    .await;
    ExpectedEnsureOutcome {
        result,
        observation,
        issued: false,
    }
}

async fn ensure_account(
    svc: &ServiceContext,
    tab: &str,
    fork: Option<(String, String)>,
    expected_nonce: Option<String>,
    acquisition: Option<Arc<ExpectedAcquisition>>,
) -> Result<Arc<Session>, RuntimeFailure> {
    check_expected_nonce(svc, tab, expected_nonce.as_deref())?;
    let authority = if expected_nonce.is_some() {
        None
    } else {
        crate::service::messenger::switch_handoff::launch_authority(svc, tab)
    };
    crate::service::messenger::switch_handoff::scope_launch(authority.clone(), async {
        let expected = match &authority {
            Some(authority) => {
                authority
                    .validate(svc)
                    .map_err(|_| RuntimeFailure::Superseded)?;
                authority.snapshot()
            }
            None => svc
                .mux
                .read()
                .get_tab(tab)
                .filter(|current| {
                    expected_nonce
                        .as_deref()
                        .is_none_or(|nonce| current.session_binding_nonce.as_deref() == Some(nonce))
                })
                .cloned()
                .ok_or(RuntimeFailure::Superseded)?,
        };
        if expected.frozen {
            return Err(RuntimeFailure::Refused(
                "Tab is sleeping; open or address it to wake".into(),
            ));
        }
        let owner = svc.clone();
        let target = tab.to_owned();
        let admitted = authority.clone();
        let selection = expected.clone();
        let original_generation = expected_nonce.clone();
        use crate::service::provider_accounts::provisioning;
        let startup = provisioning::in_startup(svc, tab);
        let implicit = crate::service::provider_accounts::selected_account(&expected)
            == codeterm_types::config::provider_accounts::IMPLICIT_ACCOUNT;
        let provision = move || {
            #[cfg(any(test, feature = "test-support"))]
            expected_launch_commit(&owner, "account_provision");
            if admitted.is_some() || original_generation.is_some() || startup {
                if let Some(authority) = &admitted {
                    authority.validate(&owner)?;
                }
                if !owner
                    .mux
                    .read()
                    .get_tab(&target)
                    .is_some_and(|current| account_authority_matches(&selection, current))
                {
                    return Err("Runtime launch generation superseded".into());
                }
                let provider = selection
                    .agent_type
                    .as_deref()
                    .ok_or("Account tab has no harness")?;
                let account = crate::service::provider_accounts::selected_account(&selection);
                let namespace = crate::service::provider_accounts::credential_namespace(
                    &owner,
                    provider,
                    Some(account),
                    selection.restorable_launch.account_namespace.as_deref(),
                )?;
                let invocation = crate::service::provider_accounts::invocation_at(
                    &owner,
                    provider,
                    Some(account),
                    namespace.as_deref(),
                )?;
                if let Some(authority) = &admitted {
                    authority.validate(&owner)?;
                }
                if !owner
                    .mux
                    .read()
                    .get_tab(&target)
                    .is_some_and(|current| account_authority_matches(&selection, current))
                {
                    return Err("Runtime launch generation superseded".into());
                }
                return Ok(invocation);
            }
            crate::service::provider_accounts::for_tab(&owner, &target)
        };
        let invocation = if implicit {
            let invocation = provision();
            if let Some(acquisition) = &acquisition {
                let completed_refusal = invocation
                    .as_ref()
                    .err()
                    .is_some_and(|reason| !reason.starts_with("account_provisioning_busy:"));
                acquisition
                    .settled
                    .store(invocation.is_ok() || completed_refusal, Ordering::Release);
            }
            invocation
        } else {
            let observation =
                provisioning::start(svc, provision).map_err(RuntimeFailure::Refused)?;
            let invocation = observation.wait(std::time::Duration::from_secs(60)).await;
            if let Some(acquisition) = &acquisition {
                acquisition.settled.store(
                    invocation.is_ok() || observation.observed_completed_refusal(),
                    Ordering::Release,
                );
            }
            invocation
        };
        check_expected_nonce(svc, tab, expected_nonce.as_deref())?;
        let invocation = invocation.map_err(RuntimeFailure::Refused)?;
        if crate::crash::shutdown::is_shutting_down()
            || !svc.mux.read().get_tab(tab).is_some_and(|current| {
                if authority.is_some() || expected_nonce.is_some() || startup {
                    account_authority_matches(&expected, current)
                } else {
                    concurrent_ensure_authority_matches(&expected, current)
                }
            })
        {
            return Err(RuntimeFailure::Superseded);
        }
        let original = expected_nonce.as_ref().map(|_| expected.clone());
        codeterm_plugin::provider_account::scope(
            invocation,
            ensure_scoped(svc, tab, fork, expected_nonce, original, acquisition),
        )
        .await
    })
    .await
}

pub fn reserve_switch_launch(
    svc: &ServiceContext,
    authority: &Arc<crate::service::messenger::switch_handoff::SwitchLaunchAuthority>,
) -> Result<String, String> {
    authority.validate(svc)?;
    let (nonce, expected) =
        svc.reserve_agent_launch_binding_nonce_if_current(&authority.snapshot(), None, None)?;
    authority.reserved(expected);
    nonce.ok_or_else(|| {
        "provider_switch_superseded: target unexpectedly authenticated before launch".into()
    })
}

pub fn launch_switch_target(
    svc: &ServiceContext,
    authority: &Arc<crate::service::messenger::switch_handoff::SwitchLaunchAuthority>,
    config: Value,
) -> Result<(), String> {
    let expected = authority.snapshot();
    let provider = expected
        .agent_type
        .as_deref()
        .ok_or("Runtime provider absent")?;
    let nonce = expected
        .session_binding_nonce
        .as_deref()
        .ok_or("Runtime binding absent")?;
    {
        let mut mux = svc.mux.write();
        let current = mux.get_tab_mut(&expected.id).ok_or("Runtime tab absent")?;
        if !authority.matches(current) {
            return Err("provider_switch_superseded: target runtime launch changed".into());
        }
        current.agent_runtime_config = Some(config);
        current.chat_view = true;
    }
    super::super::authority::record_spawn(svc, &expected.id, provider, nonce)?;
    crate::async_runtime::block_on(ensure(svc, &expected.id, None))
        .or_else(|error| super::login_pending_session(svc, &expected.id, error))
        .map(|_| ())
        .map_err(|error| format!("Runtime tab {} degraded: {error}", expected.id))
}

/// A plain concurrent `ensure()` may observe the same launch generation committing its initial
/// thread binding (`None -> Some(thread)` with matching `reported_session_binding_nonce`) while
/// waiting on account provisioning; reuse that bound runtime instead of falsely returning Superseded.
fn concurrent_ensure_authority_matches(
    expected: &crate::multiplexer::types::Tab,
    current: &crate::multiplexer::types::Tab,
) -> bool {
    if account_authority_matches(expected, current) {
        return true;
    }
    let Some(bound_thread) = current.agent_session_id.as_deref() else {
        return false;
    };
    if expected.agent_session_id.is_some()
        || current.session_binding_nonce.is_none()
        || current.reported_session_binding_nonce != current.session_binding_nonce
    {
        return false;
    }
    let mut bound = expected.clone();
    bound.agent_session_id = Some(bound_thread.to_owned());
    if let crate::multiplexer::types::TabBacking::ChatBackend { session_id, .. } =
        &mut bound.backing
    {
        if session_id.is_empty() {
            *session_id = bound_thread.to_owned();
        }
    }
    account_authority_matches(&bound, current)
}

pub(super) fn account_authority_matches(
    expected: &crate::multiplexer::types::Tab,
    current: &crate::multiplexer::types::Tab,
) -> bool {
    current.agent_type == expected.agent_type
        && current.agent_session_id == expected.agent_session_id
        && current.session_binding_nonce == expected.session_binding_nonce
        && current.lifecycle_epoch == expected.lifecycle_epoch
        && current.lifecycle_operation_id == expected.lifecycle_operation_id
        && current.restorable_launch.account_id == expected.restorable_launch.account_id
        && current.restorable_launch.account_namespace
            == expected.restorable_launch.account_namespace
        && current.team_id == expected.team_id
        && current.parent_tab_id == expected.parent_tab_id
        && current.task_binding == expected.task_binding
        && current.role == expected.role
        && current.role_profile == expected.role_profile
        && current.report_target_tab_id == expected.report_target_tab_id
        && current.report_target_mesh_peer == expected.report_target_mesh_peer
        && current.spawned_by_tab_id == expected.spawned_by_tab_id
        && current.depth == expected.depth
        && current.can_spawn == expected.can_spawn
        && current.neighbor_authority == expected.neighbor_authority
        && current.ownership == expected.ownership
        && current.backing == expected.backing
        && current.cwd == expected.cwd
        && !current.frozen
}

pub async fn ensure_with_invocation(
    svc: &ServiceContext,
    tab: &str,
    invocation: Option<Arc<codeterm_plugin::provider_account::InvocationContext>>,
) -> Result<Arc<Session>, RuntimeFailure> {
    let authority = crate::service::messenger::switch_handoff::launch_authority(svc, tab);
    crate::service::messenger::switch_handoff::scope_launch(
        authority,
        codeterm_plugin::provider_account::scope(
            invocation,
            ensure_scoped(svc, tab, None, None, None, None),
        ),
    )
    .await
}

pub(crate) fn account_boundary_ready(session: &Session, completion: Option<(&str, &str)>) -> bool {
    let context = session.context.lock();
    context.active_turn_id.is_none()
        && context.active_turns.is_empty()
        && context.pending_request.is_none()
        && session.turn_requests.lock().is_empty()
        && completion.is_none_or(|(thread, turn)| session.completions.lock().contains(thread, turn))
}

pub(crate) fn account_trial_idle(svc: &ServiceContext, session: &Session) -> bool {
    validate_binding(svc, session).is_ok()
        && session
            .thread_loaded
            .load(std::sync::atomic::Ordering::Acquire)
        && account_boundary_ready(session, None)
        && session.prompts.lock().is_empty()
}

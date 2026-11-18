use super::{
    policy,
    switching::{Authentication, Binding, Claim, Decision, Switches},
    State,
};
use crate::service::{
    agent_lifecycle::{AccountRestartBinding, AccountRestartTarget},
    agent_runtime::{engine, RuntimeEvent},
    ServiceContext,
};
use codeterm_plugin::agent_runtime::{
    RuntimeAuthState, RuntimeErrorSignal, RuntimeTurnDetail, RuntimeTurnState,
};
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};

mod dispatch;
mod limits;
mod pty_input;
mod requests;
pub(super) use dispatch::dispatch;
pub use pty_input::PtyInputRevision;
pub(crate) use pty_input::{admit_pty_input, admit_pty_write, pty_input_witness};
pub(super) use requests::evacuate;
pub(super) use requests::reauthenticated;
pub use requests::{select, SelectRequest};

#[derive(Default)]
pub(super) struct Scheduler {
    pub book: Switches,
    errors: BTreeMap<String, Limit>,
    pub boundaries: BTreeMap<String, Boundary>,
    pub pending: BTreeMap<String, Pending>,
    pub transitions: BTreeMap<String, Transition>,
    auth_contexts: BTreeMap<(String, String), String>,
    quota_versions: BTreeMap<(String, String), String>,
    input_generation: u64,
    inputs: BTreeMap<String, InputAdmission>,
}

impl Scheduler {
    pub(super) fn forget_account(&mut self, provider: &str, account: &str) {
        self.book.forget(provider, account);
        self.auth_contexts
            .remove(&(provider.into(), account.into()));
        self.quota_versions
            .remove(&(provider.into(), account.into()));
    }
    pub(super) fn retire(&mut self, tab: &str) {
        self.book.close_tab(tab);
        self.errors.remove(tab);
        self.pending.remove(tab);
        self.transitions.remove(tab);
        self.boundaries.remove(tab);
        self.inputs.remove(tab);
    }
}

#[derive(Clone)]
struct InputAdmission {
    generation: u64,
    user_send_id: Option<String>,
}

struct Limit {
    version: String,
    expected: AccountRestartBinding,
    turn: String,
    resets_at: Option<u64>,
    input: Option<PtyInputWitness>,
}
#[derive(Clone)]
pub(crate) struct PtyInputWitness {
    generation: u64,
    revision: (String, u64, u64),
    turn: String,
    path: std::path::PathBuf,
    epoch: crate::service::tab_chat::resume_cursor::ChatEpoch,
    source: crate::service::tab_chat::registry::SourceIncarnation,
    offset: u64,
}
pub(super) struct Boundary {
    pub expected: AccountRestartBinding,
    pub turn: String,
    pub input: Option<PtyInputWitness>,
}
pub(super) struct Pending {
    pub claim: Claim,
    pub target: AccountRestartTarget,
    pub reservation: super::admission::Reservation,
    pub resume_work: bool,
    pub automatic: bool,
    pub verify: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub source_account_restored: bool,
    pub cause: &'static str,
    pub account_identity: String,
    pub id: String,
    pub provider_id: String,
    pub account_id: String,
    pub status: &'static str,
    pub next_account_id: Option<String>,
    pub next_identity: Option<String>,
    pub resets_at_ms: Option<u64>,
    pub turn_id: Option<String>,
    pub reason: Option<String>,
    pub continuation: codeterm_plugin::provider_account::storage::AccountContinuation,
}

pub fn transition(svc: &ServiceContext, tab: &str) -> Option<Transition> {
    if let Some(transition) = svc
        .try_ext::<State>()
        .and_then(|state| state.scheduler.lock().transitions.get(tab).cloned())
    {
        return Some(transition);
    }
    let (provider, account, nonce) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab)?;
        (
            tab.agent_type.clone()?,
            super::selected_account(tab).to_owned(),
            tab.session_binding_nonce.clone()?,
        )
    };
    let policy = policy(svc, &provider);
    let Authentication::Exhausted { resets_at } = limits::active(&policy, &account)? else {
        return None;
    };
    let available = availability(svc, &provider, &policy.order);
    let next = policy
        .next_eligible(&account, |id| {
            available.get(id) == Some(&Authentication::Ready)
                && super::admission::available(svc, &provider, id)
        })
        .ok()
        .flatten()
        .map(str::to_owned);
    let all = policy.order.iter().all(|id| {
        matches!(
            limits::active(&policy, id),
            Some(Authentication::Exhausted { .. })
        )
    });
    Some(Transition {
        source_account_restored: false,
        cause: "limit",
        account_identity: identity(svc, &provider, &account),
        id: format!("saved-limit-{nonce}"),
        provider_id: provider.clone(),
        account_id: account,
        status: if all { "allExhausted" } else { "exhausted" },
        next_identity: next
            .as_ref()
            .map(|account| identity(svc, &provider, account)),
        next_account_id: next,
        resets_at_ms: resets_at,
        turn_id: None,
        reason: None,
        continuation: super::continuation(svc, &provider),
    })
}

pub(super) fn availability(
    svc: &ServiceContext,
    provider: &str,
    accounts: &[String],
) -> BTreeMap<String, Authentication> {
    let state = svc.ext_or_init::<State>(State::default);
    let mut scheduler = state.scheduler.lock();
    hydrate(svc, provider, &mut scheduler.book);
    limits::expire(&mut scheduler.book, provider, accounts);
    accounts
        .iter()
        .filter_map(|account| {
            scheduler
                .book
                .authentication(provider, account)
                .map(|authentication| (account.clone(), authentication))
        })
        .collect()
}

fn runtime_epoch(tab: &str) -> Option<u64> {
    engine::for_tab(tab).map(|session| session.context.lock().epoch)
}

fn capture(svc: &ServiceContext, tab: &str) -> Result<AccountRestartBinding, String> {
    let epoch = runtime_epoch(tab);
    let mux = svc.mux.read();
    AccountRestartBinding::capture(
        mux.get_tab(tab).ok_or("The account tab is unavailable")?,
        epoch,
    )
}

fn bound(svc: &ServiceContext, tab: &str, expected: &AccountRestartBinding) -> bool {
    let epoch = runtime_epoch(tab);
    svc.mux
        .read()
        .get_tab(tab)
        .is_some_and(|tab| expected.validate(tab, epoch).is_ok())
}

fn hydrate(svc: &ServiceContext, provider: &str, book: &mut Switches) {
    let policy = policy(svc, provider);
    let state = svc.ext_or_init::<State>(State::default);
    let observations = state.observations.lock();
    for account in &policy.order {
        if book.authentication(provider, account).is_none() {
            let authentication = match observations
                .get(&(provider.into(), account.clone()))
                .and_then(|observation| observation.authenticated)
            {
                Some(true) => {
                    Some(limits::active(&policy, account).unwrap_or(Authentication::Ready))
                }
                Some(false) => Some(Authentication::SignedOut),
                None => limits::active(&policy, account),
            };
            if let Some(authentication) = authentication {
                book.set_authentication(provider, account, authentication);
            }
        }
        if !super::admission::available(svc, provider, account) {
            book.mark_removing(provider, account);
        }
    }
}

fn binding(expected: &AccountRestartBinding, tab: &str, book: &Switches) -> Binding {
    let account = expected
        .account_id
        .as_deref()
        .unwrap_or(super::IMPLICIT_ACCOUNT);
    Binding {
        provider: expected.provider.clone(),
        account: account.into(),
        tab: tab.into(),
        nonce: expected.binding_nonce.clone(),
        epoch: expected
            .runtime_epoch
            .or(expected.lifecycle_epoch)
            .unwrap_or_default(),
        session: expected.session_id.clone(),
        authentication_generation: book
            .generation(&expected.provider, account)
            .unwrap_or_default(),
    }
}

pub(super) fn authenticated(svc: &ServiceContext, provider: &str, account: &str) {
    let policy = policy(svc, provider);
    let authentication = limits::active(&policy, account).unwrap_or(Authentication::Ready);
    let context = if account == super::IMPLICIT_ACCOUNT {
        policy
            .default_identity
            .as_ref()
            .map(|identity| identity.subject.clone())
            .unwrap_or_else(|| "usual".into())
    } else {
        policy
            .profiles
            .get(account)
            .and_then(|profile| profile.credential_namespace.clone())
            .unwrap_or_else(|| account.into())
    };
    let mut scheduler = svc.ext_or_init::<State>(State::default).scheduler.lock();
    if scheduler
        .auth_contexts
        .insert((provider.into(), account.into()), context.clone())
        .is_some_and(|previous| previous != context)
    {
        scheduler
            .book
            .set_authentication(provider, account, Authentication::SignedOut);
    }
    scheduler
        .book
        .set_authentication(provider, account, authentication);
}

pub(super) fn signed_out(svc: &ServiceContext, provider: &str, account: &str) {
    svc.ext_or_init::<State>(State::default)
        .scheduler
        .lock()
        .book
        .set_authentication(provider, account, Authentication::SignedOut);
}

pub(super) fn auth_changed(svc: &ServiceContext, tab: &str) {
    let selected = svc
        .mux
        .read()
        .get_tab(tab)
        .filter(|tab| tab.description.as_deref() != Some("domios:provider-account-sign-in"))
        .map(|tab| {
            (
                tab.agent_type.clone(),
                super::selected_account(tab).to_owned(),
                tab.agent_runtime_state
                    .as_ref()
                    .and_then(|state| state["auth"].as_str().map(str::to_owned)),
            )
        });
    let Some((Some(provider), account, Some(auth))) = selected else {
        return;
    };
    if super::capability(svc, &provider).is_none() {
        return;
    }
    let state = svc.ext_or_init::<State>(State::default);
    let mut scheduler = state.scheduler.lock();
    if auth == "signedOut" {
        scheduler
            .book
            .set_authentication(&provider, &account, Authentication::SignedOut);
    } else if auth == "signedIn" && scheduler.book.authentication(&provider, &account).is_none() {
        scheduler
            .book
            .set_authentication(&provider, &account, Authentication::Ready);
    }
}

pub(crate) fn runtime_observe(
    svc: &ServiceContext,
    session: &Arc<engine::Session>,
    event: &RuntimeEvent,
) -> bool {
    let context = session.context.lock().clone();
    if super::capability(svc, &session.provider).is_none() {
        return false;
    }
    match event {
        RuntimeEvent::ItemUpdated { thread_id, item }
            if context.thread_id.as_ref() == Some(thread_id)
                && item.system_subtype.as_deref() == Some("api_error") =>
        {
            let signal = item
                .tool_input
                .as_ref()
                .and_then(|value| serde_json::from_value::<RuntimeErrorSignal>(value.clone()).ok())
                .filter(|signal| !signal.retryable);
            if let (Some(limit), Some(turn)) = (
                signal.and_then(|signal| signal.account_limit),
                item.tool_flow_id.as_ref(),
            ) {
                if *session.auth.lock() != RuntimeAuthState::SignedIn
                    || !(context.active_turn_id.as_ref() == Some(turn)
                        || context.active_turns.iter().any(|active| {
                            active.thread_id == *thread_id && active.turn_id == *turn
                        }))
                {
                    return false;
                }
                let Ok(expected) = capture(svc, &context.tab_id) else {
                    return false;
                };
                svc.ext_or_init::<State>(State::default)
                    .scheduler
                    .lock()
                    .errors
                    .insert(
                        context.tab_id.clone(),
                        Limit {
                            input: None,
                            version: String::new(),
                            expected,
                            turn: turn.clone(),
                            resets_at: limit.resets_at_ms,
                        },
                    );
                return true;
            }
        }
        RuntimeEvent::TurnUpdated {
            thread_id,
            turn_id: Some(turn),
            state: RuntimeTurnState::Idle,
            detail: Some(RuntimeTurnDetail::Failed),
            ..
        } if context.thread_id.as_ref() == Some(thread_id) => {
            let limit = svc
                .ext_or_init::<State>(State::default)
                .scheduler
                .lock()
                .errors
                .remove(&context.tab_id);
            if let Some(limit) = limit
                .filter(|limit| limit.turn == *turn && bound(svc, &context.tab_id, &limit.expected))
            {
                queue_exhausted(svc, &context.tab_id, limit);
                return true;
            }
            return svc
                .ext_or_init::<State>(State::default)
                .scheduler
                .lock()
                .boundaries
                .get(&context.tab_id)
                .is_some_and(|boundary| boundary.turn == *turn);
        }
        RuntimeEvent::TurnUpdated {
            thread_id,
            turn_id: Some(_),
            state: RuntimeTurnState::Idle,
            detail: None,
            ..
        } if context.thread_id.as_ref() == Some(thread_id)
            && *session.auth.lock() == RuntimeAuthState::SignedIn =>
        {
            successful_turn(svc, &context.tab_id, thread_id, &context.binding_nonce);
        }
        RuntimeEvent::TurnUpdated {
            state: RuntimeTurnState::Working,
            ..
        } => {
            invalidate_boundary(svc, &context.tab_id);
        }
        _ => {}
    }
    false
}

pub(crate) fn transcript_limit(
    svc: &ServiceContext,
    tab: &str,
    session: &str,
    nonce: &str,
    turn: &str,
    signal: &RuntimeErrorSignal,
    input: Option<&PtyInputWitness>,
) -> bool {
    let Some(limit) = &signal.account_limit else {
        return false;
    };
    if signal.retryable {
        return false;
    }
    let Ok(expected) = capture(svc, tab) else {
        return false;
    };
    if expected.session_id != session
        || expected.binding_nonce != nonce
        || super::capability(svc, &expected.provider).is_none()
    {
        return false;
    }
    let Some(input) = input.filter(|input| input.turn == turn && input.current(svc, tab)) else {
        return true;
    };
    queue_exhausted(
        svc,
        tab,
        Limit {
            input: Some(input.clone()),
            version: String::new(),
            expected,
            turn: turn.into(),
            resets_at: limit.resets_at_ms,
        },
    );
    true
}

fn queue_exhausted(svc: &ServiceContext, tab: &str, mut limit: Limit) {
    if limit
        .input
        .as_ref()
        .is_some_and(|input| !input.current(svc, tab))
    {
        return;
    }
    limit.version = uuid::Uuid::new_v4().to_string();
    {
        let mut scheduler = svc.ext_or_init::<State>(State::default).scheduler.lock();
        scheduler.quota_versions.insert(
            (
                limit.expected.provider.clone(),
                limit
                    .expected
                    .account_id
                    .clone()
                    .unwrap_or_else(|| super::IMPLICIT_ACCOUNT.into()),
            ),
            limit.version.clone(),
        );
        scheduler.boundaries.insert(
            tab.into(),
            Boundary {
                expected: limit.expected.clone(),
                turn: limit.turn.clone(),
                input: limit.input.clone(),
            },
        );
    }
    let owner = svc.clone();
    let tab = tab.to_owned();
    let receipt = super::work::Receipt::capture(svc);
    crate::async_runtime::spawn(async move {
        let _ = codeterm_plugin::js_runtime::spawn_blocking(move || {
            let _receipt = receipt;
            exhausted(&owner, &tab, limit)
        })
        .await;
    });
}

pub(crate) fn invalidate_boundary(svc: &ServiceContext, tab: &str) {
    if let Some(state) = svc.try_ext::<State>() {
        let mut scheduler = state.scheduler.lock();
        scheduler.boundaries.remove(tab);
        scheduler.errors.remove(tab);
        if scheduler
            .pending
            .get(tab)
            .is_some_and(|pending| pending.resume_work)
        {
            if let Some(pending) = scheduler.pending.remove(tab) {
                scheduler.book.cancel(&pending.claim);
            }
            scheduler.transitions.remove(tab);
        }
    }
}

fn exhausted(svc: &ServiceContext, tab: &str, limit: Limit) {
    if limit
        .input
        .as_ref()
        .is_some_and(|input| !input.current(svc, tab))
    {
        return;
    }
    let policy = policy(svc, &limit.expected.provider);
    if super::capability(svc, &limit.expected.provider).is_none() {
        return;
    }
    let account = limit
        .expected
        .account_id
        .as_deref()
        .unwrap_or(super::IMPLICIT_ACCOUNT);
    let operation = super::operation(svc, &limit.expected.provider, account);
    let Some(_operation) = operation.try_lock_for(std::time::Duration::from_secs(30)) else {
        return;
    };
    let state = svc.ext_or_init::<State>(State::default);
    let decision = {
        let mut scheduler = state.scheduler.lock();
        if scheduler
            .quota_versions
            .get(&(limit.expected.provider.clone(), account.into()))
            != Some(&limit.version)
        {
            return;
        }
        if !scheduler.boundaries.get(tab).is_some_and(|boundary| {
            boundary.expected == limit.expected && boundary.turn == limit.turn
        }) {
            return;
        }
        // A structured account limit is itself authenticated provider evidence for this source.
        if scheduler
            .book
            .authentication(&limit.expected.provider, account)
            .is_none()
        {
            scheduler.book.set_authentication(
                &limit.expected.provider,
                account,
                Authentication::Ready,
            );
        }
        hydrate(svc, &limit.expected.provider, &mut scheduler.book);
        let binding = binding(&limit.expected, tab, &scheduler.book);
        scheduler.book.bind(binding.clone());
        scheduler.book.exhausted(
            &binding,
            &limit.turn,
            limit.resets_at,
            &policy,
            super::continuation(svc, &limit.expected.provider)
                == codeterm_plugin::provider_account::storage::AccountContinuation::Proven,
        )
    };
    let next = match &decision {
        Decision::Switch(claim) => Some(claim.target.clone()),
        Decision::AutomaticDisabled { next } | Decision::ContinuationUnverified { next } => {
            next.clone()
        }
        Decision::AllExhausted => None,
        Decision::Stale | Decision::AlreadyHandled => return,
    };
    if let Err(reason) = limits::remember(svc, &limit.expected.provider, account, limit.resets_at) {
        log::debug!("[provider-accounts] could not persist exhausted account state: {reason}");
    }
    let (all, first_reset) = {
        let scheduler = state.scheduler.lock();
        let limits: Vec<_> = policy
            .order
            .iter()
            .map(|account| {
                scheduler
                    .book
                    .authentication(&limit.expected.provider, account)
            })
            .collect();
        let all = limits
            .iter()
            .all(|limit| matches!(limit, Some(Authentication::Exhausted { .. })));
        let resets: Option<Vec<u64>> = limits
            .into_iter()
            .map(|limit| match limit {
                Some(Authentication::Exhausted {
                    resets_at: Some(at),
                }) => Some(at),
                _ => None,
            })
            .collect();
        (all, resets.and_then(|resets| resets.into_iter().min()))
    };
    let mut transition = Transition {
        source_account_restored: false,
        cause: "limit",
        account_identity: identity(svc, &limit.expected.provider, account),
        id: uuid::Uuid::new_v4().to_string(),
        provider_id: limit.expected.provider.clone(),
        account_id: account.into(),
        status: if all { "allExhausted" } else { "exhausted" },
        next_identity: next
            .as_ref()
            .map(|account| identity(svc, &limit.expected.provider, account)),
        next_account_id: next,
        resets_at_ms: if all { first_reset } else { limit.resets_at },
        turn_id: Some(limit.turn),
        reason: None,
        continuation: super::continuation(svc, &limit.expected.provider),
    };
    if let Decision::Switch(claim) = decision {
        match prepare(svc, tab, claim.clone(), limit.expected.clone(), true, false) {
            Ok(mut pending) => {
                pending.automatic = true;
                let mut scheduler = state.scheduler.lock();
                if !scheduler.boundaries.get(tab).is_some_and(|boundary| {
                    boundary.expected == limit.expected && boundary.turn == pending.claim.turn
                }) {
                    scheduler.book.cancel(&pending.claim);
                    return;
                }
                scheduler.pending.insert(tab.into(), pending);
                transition.status = "waiting";
            }
            Err(reason) => {
                state.scheduler.lock().book.cancel(&claim);
                transition.status = "failed";
                transition.reason = Some(reason);
            }
        }
    }
    state
        .scheduler
        .lock()
        .transitions
        .insert(tab.into(), transition);
    emit(svc, tab);
    dispatch(svc, tab);
}

fn prepare(
    svc: &ServiceContext,
    _tab: &str,
    claim: Claim,
    expected: AccountRestartBinding,
    resume_work: bool,
    verify: bool,
) -> Result<Pending, String> {
    let reservation = super::admission::reserve(svc, &expected.provider, Some(&claim.target))?;
    let namespace =
        super::credential_namespace(svc, &expected.provider, Some(&claim.target), None)?;
    let invocation = super::invocation_at(
        svc,
        &expected.provider,
        Some(&claim.target),
        namespace.as_deref(),
    )?;
    let terminal_turn = svc
        .ext_or_init::<State>(State::default)
        .scheduler
        .lock()
        .boundaries
        .get(&claim.source.tab)
        .filter(|boundary| boundary.expected == expected && boundary.turn == claim.turn)
        .map(|boundary| boundary.turn.clone());
    if resume_work && expected.runtime_epoch.is_none() && terminal_turn.is_none() {
        return Err("The failed turn no longer owns this PTY input generation".into());
    }
    Ok(Pending {
        claim: claim.clone(),
        target: AccountRestartTarget {
            expected,
            account_id: Some(claim.target.clone()),
            account_namespace: namespace,
            invocation,
            terminal_turn,
        },
        reservation,
        resume_work,
        automatic: false,
        verify,
    })
}

fn identity(svc: &ServiceContext, provider: &str, account: &str) -> String {
    let policy = policy(svc, provider);
    if account == super::IMPLICIT_ACCOUNT {
        policy
            .default_name
            .or_else(|| policy.default_identity.map(|identity| identity.label))
    } else {
        policy.profiles.get(account).and_then(|profile| {
            profile.name.clone().or_else(|| {
                profile
                    .identity
                    .as_ref()
                    .map(|identity| identity.label.clone())
            })
        })
    }
    .unwrap_or_else(|| {
        svc.agent_registry
            .with_provider(provider, |provider| provider.display_name().to_owned())
            .unwrap_or_else(|| "Account".into())
    })
}

pub fn account_turn_boundary(
    svc: &ServiceContext,
    tab: &str,
    expected: &AccountRestartBinding,
    turn: &str,
) -> bool {
    let Some(state) = svc.try_ext::<State>() else {
        return false;
    };
    let input = {
        let scheduler = state.scheduler.lock();
        let Some(boundary) = scheduler
            .boundaries
            .get(tab)
            .filter(|boundary| boundary.expected == *expected && boundary.turn == turn)
        else {
            return false;
        };
        boundary.input.clone()
    };
    input.as_ref().is_none_or(|input| input.current(svc, tab))
}

fn emit(svc: &ServiceContext, tab: &str) {
    svc.ui.emit(
        "provider-account-transition",
        serde_json::json!({"tabId":tab,"transition":transition(svc,tab)}),
    );
}

pub fn can_try_now(svc: &ServiceContext, tab: &str) -> bool {
    if let Some(session) = engine::for_tab(tab) {
        engine::account_trial_idle(svc, &session)
    } else {
        svc.try_snapshot_with_reconciled_status()
            .is_some_and(|snapshot| {
                snapshot.tab_statuses.get(tab) == Some(&codeterm_types::pty::TabStatus::Idle)
            })
    }
}

pub(crate) fn successful_turn(svc: &ServiceContext, tab: &str, session: &str, nonce: &str) {
    let Ok(expected) = capture(svc, tab) else {
        return;
    };
    if expected.session_id != session || expected.binding_nonce != nonce {
        return;
    }
    let account = expected
        .account_id
        .as_deref()
        .unwrap_or(super::IMPLICIT_ACCOUNT);
    let policy = policy(svc, &expected.provider);
    let version = uuid::Uuid::new_v4().to_string();
    {
        let mut scheduler = svc.ext_or_init::<State>(Default::default).scheduler.lock();
        if limits::active(&policy, account).is_none()
            && !matches!(
                scheduler.book.authentication(&expected.provider, account),
                Some(Authentication::Exhausted { .. })
            )
            && !scheduler.boundaries.values().any(|boundary| {
                boundary.expected.provider == expected.provider
                    && boundary.expected.account_id == expected.account_id
            })
        {
            return;
        }
        scheduler
            .quota_versions
            .insert((expected.provider.clone(), account.into()), version.clone());
    }
    let owner = svc.clone();
    let tab = tab.to_owned();
    let receipt = super::work::Receipt::capture(svc);
    crate::async_runtime::spawn(async move {
        let _ = codeterm_plugin::js_runtime::spawn_blocking(move || {
            let _receipt = receipt;
            if !bound(&owner, &tab, &expected) {
                return;
            }
            let account = expected
                .account_id
                .as_deref()
                .unwrap_or(super::IMPLICIT_ACCOUNT);
            let operation = super::operation(&owner, &expected.provider, account);
            let Some(_operation) = operation.try_lock_for(std::time::Duration::from_secs(30))
            else {
                return;
            };
            if owner
                .ext_or_init::<State>(Default::default)
                .scheduler
                .lock()
                .quota_versions
                .get(&(expected.provider.clone(), account.into()))
                != Some(&version)
            {
                return;
            }
            if let Err(reason) = limits::clear(&owner, &expected.provider, account) {
                log::debug!("[provider-accounts] completed-turn quota update failed: {reason}");
                return;
            }
            let mut scheduler = owner
                .ext_or_init::<State>(Default::default)
                .scheduler
                .lock();
            if scheduler
                .quota_versions
                .get(&(expected.provider.clone(), account.into()))
                != Some(&version)
            {
                return;
            }
            scheduler
                .book
                .set_authentication(&expected.provider, account, Authentication::Ready);
            let retired: Vec<_> = scheduler
                .transitions
                .iter()
                .filter(|(_, transition)| {
                    transition.provider_id == expected.provider
                        && transition.account_id == account
                        && transition.cause == "limit"
                        && matches!(transition.status, "exhausted" | "allExhausted")
                })
                .map(|(tab, _)| tab.clone())
                .collect();
            for tab in &retired {
                scheduler.transitions.remove(tab);
            }
            drop(scheduler);
            for tab in retired {
                emit(&owner, &tab);
            }
            owner.ui.emit(
                "provider-accounts-changed",
                serde_json::json!({"providerId":expected.provider}),
            );
        })
        .await;
    });
}

use super::{capability, policy, roster, storage, State};
use crate::service::{agent_runtime::login, ServiceContext};
use codeterm_plugin::provider_account::storage::AccountIdentity;
use codeterm_types::config::provider_accounts::{
    ProviderAccountConfig, IMPLICIT_ACCOUNT, MAX_PROVIDER_ACCOUNTS,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    SigningIn,
    Verifying,
    Finishing,
    Closing,
    Duplicate,
    Failed,
}

#[derive(Clone)]
pub(super) struct Entry {
    generation: String,
    account: String,
    context_account: String,
    expected_identity: Option<AccountIdentity>,
    adding: bool,
    initializing: bool,
    context: Option<login::OpenedContext>,
    nonce: Option<String>,
    epoch: Option<u64>,
    login_id: Option<String>,
    phase: Phase,
    identity: Option<AccountIdentity>,
    error: Option<String>,
    details: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginContext {
    pub tab_id: String,
    pub state: Value,
}

#[derive(Serialize)]
pub struct Adding {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<LoginContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

pub(super) fn draft_allows(svc: &ServiceContext, provider: &str, account: &str) -> bool {
    svc.ext_or_init::<State>(State::default)
        .sign_ins
        .lock()
        .get(provider)
        .is_some_and(|entry| {
            entry.context_account == account
                && entry.context_account != IMPLICIT_ACCOUNT
                && matches!(entry.phase, Phase::SigningIn | Phase::Verifying)
        })
}

pub(super) fn active(svc: &ServiceContext, provider: &str, account: &str) -> bool {
    svc.ext_or_init::<State>(State::default)
        .sign_ins
        .lock()
        .get(provider)
        .is_some_and(|entry| entry.account == account)
}

pub(super) fn permits_login(svc: &ServiceContext, tab: &str) -> bool {
    let Some(state) = svc.try_ext::<State>() else {
        return true;
    };
    state
        .sign_ins
        .lock()
        .values()
        .find(|entry| {
            entry
                .context
                .as_ref()
                .is_some_and(|context| context.tab == tab)
        })
        .is_none_or(|entry| entry.phase == Phase::SigningIn)
}

pub(super) fn begin(
    svc: &ServiceContext,
    provider: &str,
    requested: Option<&str>,
) -> Result<(), String> {
    capability(svc, provider).ok_or("This harness does not support isolated accounts")?;
    let policy = policy(svc, provider);
    policy.validate()?;
    let adding = requested.is_none();
    if adding && policy.order.len() >= MAX_PROVIDER_ACCOUNTS {
        return Err("The harness account list is full".into());
    }
    let account = requested
        .map(str::to_owned)
        .unwrap_or_else(|| format!("account-{}", uuid::Uuid::new_v4().simple()));
    if !adding && !policy.order.contains(&account) {
        return Err("The selected harness account no longer exists".into());
    }
    let expected_identity = if account == IMPLICIT_ACCOUNT {
        policy.default_identity.clone()
    } else {
        policy
            .profiles
            .get(&account)
            .and_then(|profile| profile.identity.clone())
    };
    if !adding
        && account != IMPLICIT_ACCOUNT
        && expected_identity.is_none()
        && super::tabs_in_use(svc, provider, &account) > 0
    {
        return Err("Close this account's tabs before changing an unconfirmed sign-in".into());
    }
    let context_account = if !adding && account != IMPLICIT_ACCOUNT {
        format!("auth-{}", uuid::Uuid::new_v4().simple())
    } else {
        account.clone()
    };
    let generation = uuid::Uuid::new_v4().to_string();
    let state = svc.ext_or_init::<State>(State::default);
    {
        let mut sign_ins = state.sign_ins.lock();
        if sign_ins.contains_key(provider) {
            return Err("Finish or cancel this harness's current sign-in first".into());
        }
        sign_ins.insert(
            provider.into(),
            Entry {
                generation: generation.clone(),
                account: account.clone(),
                context_account: context_account.clone(),
                expected_identity,
                adding,
                initializing: true,
                context: None,
                nonce: None,
                epoch: None,
                login_id: None,
                phase: Phase::SigningIn,
                identity: None,
                error: None,
                details: None,
            },
        );
    }
    state
        .observations
        .lock()
        .remove(&(provider.into(), account.clone()));
    let opened = login::open_account_context(svc, provider, &context_account);
    let mut sign_ins = state.sign_ins.lock();
    let entry = sign_ins
        .get_mut(provider)
        .filter(|entry| entry.generation == generation)
        .ok_or("The account sign-in changed")?;
    entry.initializing = false;
    match opened {
        Ok(context) => {
            entry.nonce = svc
                .mux
                .read()
                .get_tab(&context.tab)
                .and_then(|tab| tab.session_binding_nonce.clone());
            let tab = context.tab.clone();
            entry.context = Some(context);
            drop(sign_ins);
            changed(svc, &tab);
        }
        Err(error) => {
            entry.phase = Phase::Failed;
            entry.error = Some("Couldn't add the account.".into());
            entry.details = Some(error);
        }
    }
    emit(svc, provider);
    Ok(())
}

pub(super) fn cancel(
    svc: &ServiceContext,
    provider: &str,
    expected_account: Option<&str>,
    expected_id: Option<&str>,
) -> Result<(), String> {
    let entry = {
        let mut sign_ins = svc.ext_or_init::<State>(State::default).sign_ins.lock();
        let Some(entry) = sign_ins.get_mut(provider) else {
            return Ok(());
        };
        if expected_id != Some(entry.generation.as_str()) {
            return Err("The account sign-in changed. Refresh it before trying again.".into());
        }
        if expected_account.is_some_and(|account| account != entry.account) {
            return Err("The account sign-in changed".into());
        }
        if entry.initializing || matches!(entry.phase, Phase::Closing | Phase::Finishing) {
            return Err("The account sign-in is still finishing".into());
        }
        entry.phase = Phase::Closing;
        entry.clone()
    };
    let result = close_and_discard(
        svc,
        provider,
        &entry,
        entry.context_account != IMPLICIT_ACCOUNT,
    );
    let mut sign_ins = svc.ext_or_init::<State>(State::default).sign_ins.lock();
    if let Some(current) = sign_ins
        .get_mut(provider)
        .filter(|current| current.generation == entry.generation)
    {
        match result {
            Ok(()) => {
                sign_ins.remove(provider);
            }
            Err(error) => {
                current.phase = Phase::Failed;
                current.error = Some("Account cleanup could not finish.".into());
                current.details = Some(error);
            }
        }
    }
    drop(sign_ins);
    emit(svc, provider);
    Ok(())
}

fn close_and_discard(
    svc: &ServiceContext,
    provider: &str,
    entry: &Entry,
    discard: bool,
) -> Result<(), String> {
    if let Some(context) = &entry.context {
        if svc.mux.read().get_tab(&context.tab).is_some() {
            crate::async_runtime::block_on(login::close_account_context(svc, context))?;
        }
    }
    if discard {
        let policy = policy(svc, provider);
        if policy.profiles.iter().any(|(id, profile)| {
            profile.credential_namespace.as_deref().unwrap_or(id) == entry.context_account
                || profile.retained_namespaces.contains(&entry.context_account)
        }) {
            return Err("The account was added during cleanup; credentials were retained".into());
        }
        let operation = super::operation(svc, provider, &entry.context_account);
        let _operation = operation
            .try_lock_for(Duration::from_secs(30))
            .ok_or("An account operation is still finishing; credentials were retained")?;
        let state = svc.ext_or_init::<State>(State::default);
        let _provisioning = state.provisioning.lock();
        storage::discard(&state.root, provider, &entry.context_account)?;
    }
    Ok(())
}

fn context(svc: &ServiceContext, entry: &Entry) -> Option<LoginContext> {
    let context = entry.context.as_ref()?;
    let mut state = svc
        .mux
        .read()
        .get_tab(&context.tab)
        .and_then(|tab| tab.agent_runtime_state.clone())
        .unwrap_or_else(|| json!({"state":"loading","epoch":0,"auth":"unknown"}));
    if entry.phase != Phase::SigningIn {
        state = json!({"state":"loading","epoch":state["epoch"],"auth":"unknown","notice":"Checking account"});
    }
    Some(LoginContext {
        tab_id: context.tab.clone(),
        state,
    })
}

pub(super) struct Projection {
    pub id: Option<String>,
    pub adding: Option<Adding>,
    pub reauth: Option<(String, Option<LoginContext>, Option<String>)>,
}

pub(super) fn project(svc: &ServiceContext, provider: &str) -> Projection {
    let entry = svc
        .ext_or_init::<State>(State::default)
        .sign_ins
        .lock()
        .get(provider)
        .cloned();
    let Some(entry) = entry else {
        return Projection {
            id: None,
            adding: None,
            reauth: None,
        };
    };
    let id = Some(entry.generation.clone());
    let context = context(svc, &entry);
    if !entry.adding {
        return Projection {
            id,
            adding: None,
            reauth: Some((entry.account, context, entry.error)),
        };
    }
    if entry.initializing {
        return Projection {
            id,
            adding: None,
            reauth: None,
        };
    }
    let status = match entry.phase {
        Phase::Duplicate => "duplicate",
        Phase::Failed => "failed",
        _ => "signingIn",
    };
    Projection {
        id,
        adding: Some(Adding {
            status,
            context,
            identity: entry.identity.map(|identity| identity.label),
            error: entry.error,
            details: entry.details,
        }),
        reauth: None,
    }
}

pub(super) fn retry(svc: &ServiceContext, provider: &str, id: Option<&str>) -> Result<(), String> {
    cancel(svc, provider, None, id)?;
    if svc
        .ext_or_init::<State>(State::default)
        .sign_ins
        .lock()
        .contains_key(provider)
    {
        return Ok(());
    }
    begin(svc, provider, None)
}

fn emit(svc: &ServiceContext, provider: &str) {
    svc.ui
        .emit("provider-accounts-changed", json!({"providerId":provider}));
}

pub(super) fn changed(svc: &ServiceContext, tab: &str) {
    let Some(state) = svc.try_ext::<State>() else {
        return;
    };
    let owner = {
        let sign_ins = state.sign_ins.lock();
        sign_ins
            .iter()
            .find(|(_, entry)| {
                entry
                    .context
                    .as_ref()
                    .is_some_and(|context| context.tab == tab)
            })
            .map(|(provider, entry)| (provider.clone(), entry.clone()))
    };
    let Some((provider, mut entry)) = owner else {
        return;
    };
    if entry.phase != Phase::SigningIn {
        return;
    }
    let current = svc
        .mux
        .read()
        .get_tab(tab)
        .filter(|target| {
            target.agent_type.as_deref() == Some(&provider)
                && super::selected_account(target) == entry.context_account
                && target.session_binding_nonce == entry.nonce
        })
        .and_then(|target| target.agent_runtime_state.clone());
    let Some(current) = current else {
        return;
    };
    emit(svc, &provider);
    if current["state"] == "degraded" {
        fail(
            svc,
            &provider,
            &entry,
            "The sign-in runtime could not start. Cancel and try again.".into(),
        );
        return;
    }
    if current["auth"] != "signedIn"
        || current["state"] != "loaded"
        || current["login"]["status"] == "pending"
    {
        return;
    }
    entry.epoch = current["epoch"].as_u64();
    entry.login_id = current["login"]["loginId"].as_str().map(str::to_owned);
    {
        let mut sign_ins = state.sign_ins.lock();
        let Some(current) = sign_ins.get_mut(&provider).filter(|current| {
            current.generation == entry.generation && current.phase == Phase::SigningIn
        }) else {
            return;
        };
        current.phase = Phase::Verifying;
        current.epoch = entry.epoch;
        current.login_id = entry.login_id.clone();
    }
    let owner = svc.clone();
    crate::async_runtime::spawn(async move {
        let read_owner = owner.clone();
        let read_provider = provider.clone();
        let read_entry = entry.clone();
        let result = codeterm_plugin::js_runtime::spawn_blocking(move || {
            verify(&read_owner, &read_provider, &read_entry)
        })
        .await
        .map_err(|_| "Account verification could not finish".into())
        .and_then(|result| result);
        let finish_owner = owner.clone();
        let _ = codeterm_plugin::js_runtime::spawn_blocking(move || {
            finish(&finish_owner, &provider, &entry, result)
        })
        .await;
    });
}

struct Verified {
    identity: Option<AccountIdentity>,
    duplicate: bool,
    adapter: Arc<dyn crate::agent::provider::AgentProvider>,
}

fn bound(svc: &ServiceContext, provider: &str, entry: &Entry) -> bool {
    if entry.epoch.is_none() || entry.nonce.is_none() {
        return false;
    }
    let Some(context) = &entry.context else {
        return false;
    };
    svc.mux.read().get_tab(&context.tab).is_some_and(|tab| {
        let state = tab.agent_runtime_state.as_ref();
        tab.agent_type.as_deref() == Some(provider)
            && super::selected_account(tab) == entry.context_account
            && tab.session_binding_nonce == entry.nonce
            && state.is_some_and(|state| {
                state["epoch"].as_u64() == entry.epoch
                    && state["state"] == "loaded"
                    && state["auth"] == "signedIn"
                    && state["login"]["status"] != "pending"
                    && state["login"]["loginId"].as_str() == entry.login_id.as_deref()
            })
    })
}

fn verify(svc: &ServiceContext, provider: &str, entry: &Entry) -> Result<Verified, String> {
    if !bound(svc, provider, entry) {
        return Err("The account sign-in binding changed".into());
    }
    let adapter = svc
        .agent_registry
        .provider_arc(provider)
        .ok_or("Harness is unavailable")?;
    let started = Instant::now();
    let (auth, identity) = roster::read_status(svc, provider, &entry.context_account, &adapter)?;
    if auth.is_some_and(|auth| !auth.logged_in) {
        return Err("The harness is still signed out".into());
    }
    if !entry.adding
        && entry.expected_identity.as_ref().is_some_and(|expected| {
            identity
                .as_ref()
                .is_none_or(|identity| identity.subject != expected.subject)
        })
    {
        return Err(
            "This sign-in belongs to a different account. Cancel and use Add account instead."
                .into(),
        );
    }
    if let Some(identity) = &identity {
        let policy = policy(svc, provider);
        for account in policy
            .order
            .iter()
            .filter(|account| *account != &entry.account)
        {
            if started.elapsed() > Duration::from_secs(60) {
                return Err("Account identity verification timed out".into());
            }
            let active = svc
                .ext_or_init::<State>(State::default)
                .sign_ins
                .lock()
                .get(provider)
                .is_some_and(|current| {
                    current.generation == entry.generation && current.phase == Phase::Verifying
                });
            if !active {
                return Err("Account verification was canceled".into());
            }
            let (_, existing) = roster::read_status(svc, provider, account, &adapter)?;
            if existing
                .as_ref()
                .is_some_and(|existing| existing.subject == identity.subject)
            {
                return Ok(Verified {
                    identity: Some(identity.clone()),
                    duplicate: true,
                    adapter,
                });
            }
        }
    }
    if !svc
        .agent_registry
        .provider_arc(provider)
        .is_some_and(|current| Arc::ptr_eq(&current, &adapter))
    {
        return Err("The harness changed during account verification".into());
    }
    if !bound(svc, provider, entry) {
        return Err("The account sign-in binding changed".into());
    }
    Ok(Verified {
        identity,
        duplicate: false,
        adapter,
    })
}

fn finish(svc: &ServiceContext, provider: &str, entry: &Entry, result: Result<Verified, String>) {
    let verified = match result {
        Ok(verified) => verified,
        Err(error) => {
            fail(svc, provider, entry, error);
            return;
        }
    };
    if !bound(svc, provider, entry)
        || !svc
            .agent_registry
            .provider_arc(provider)
            .is_some_and(|current| Arc::ptr_eq(&current, &verified.adapter))
    {
        fail(
            svc,
            provider,
            entry,
            "The account sign-in binding changed".into(),
        );
        return;
    }
    let mut sign_ins = svc.ext_or_init::<State>(State::default).sign_ins.lock();
    let Some(current) = sign_ins.get_mut(provider).filter(|current| {
        current.generation == entry.generation && current.phase == Phase::Verifying
    }) else {
        return;
    };
    current.phase = Phase::Finishing;
    drop(sign_ins);
    if let Err(error) = close_and_discard(
        svc,
        provider,
        entry,
        verified.duplicate && entry.context_account != IMPLICIT_ACCOUNT,
    ) {
        fail(svc, provider, entry, error);
        return;
    }
    if verified.duplicate {
        let mut sign_ins = svc.ext_or_init::<State>(State::default).sign_ins.lock();
        if let Some(current) = sign_ins
            .get_mut(provider)
            .filter(|current| current.generation == entry.generation)
        {
            current.phase = Phase::Duplicate;
            current.context = None;
            current.identity = verified.identity;
        }
        drop(sign_ins);
        emit(svc, provider);
        return;
    }
    let identity = verified.identity;
    let config = match svc.try_ext::<crate::config::ConfigState>() {
        Some(config) => config,
        None => {
            fail(svc, provider, entry, "Configuration is unavailable".into());
            return;
        }
    };
    let applied = crate::config::update_config_and_apply(config, |config| {
        let policy = config
            .agents
            .entry(provider.into())
            .or_default()
            .accounts
            .get_or_insert_with(Default::default);
        if entry.adding {
            if policy.profiles.contains_key(&entry.account) {
                return Err("The account was already added".into());
            }
            policy.profiles.insert(
                entry.account.clone(),
                ProviderAccountConfig {
                    name: None,
                    identity: identity.clone(),
                    ..Default::default()
                },
            );
            policy.order.push(entry.account.clone());
        } else if entry.account == IMPLICIT_ACCOUNT {
            policy.default_identity = identity.clone();
        } else {
            let profile = policy
                .profiles
                .get_mut(&entry.account)
                .ok_or("The selected harness account no longer exists")?;
            if profile.identity != entry.expected_identity {
                return Err("The account identity changed during sign-in".into());
            }
            if profile.retained_namespaces.len() >= MAX_PROVIDER_ACCOUNTS {
                return Err("Close old account tabs before replacing this sign-in again".into());
            }
            let previous = profile
                .credential_namespace
                .replace(entry.context_account.clone())
                .unwrap_or_else(|| entry.account.clone());
            profile.retained_namespaces.push(previous);
            profile.identity = identity.clone();
        }
        policy.validate()
    });
    match applied {
        Ok(applied) => {
            roster::record_authenticated(svc, provider, &entry.account, identity);
            if entry.adding {
                let position = policy(svc, provider).order.len().saturating_sub(1);
                svc.ext_or_init::<State>(State::default)
                    .feedback
                    .lock()
                    .insert(
                        provider.into(),
                        roster::Feedback {
                            id: uuid::Uuid::new_v4().to_string(),
                            kind: "added",
                            account_id: entry.account.clone(),
                            position,
                        },
                    );
            }
            svc.ext_or_init::<State>(State::default)
                .sign_ins
                .lock()
                .remove(provider);
            svc.ui.emit(
                "config-changed",
                serde_json::to_value(applied).unwrap_or(Value::Null),
            );
            emit(svc, provider);
        }
        Err(error) => fail(svc, provider, entry, error),
    }
}

fn fail(svc: &ServiceContext, provider: &str, entry: &Entry, details: String) {
    if let Some(current) = svc
        .ext_or_init::<State>(State::default)
        .sign_ins
        .lock()
        .get_mut(provider)
        .filter(|current| {
            current.generation == entry.generation
                && !matches!(current.phase, Phase::Closing | Phase::Duplicate)
        })
    {
        current.phase = Phase::Failed;
        current.error = Some("Couldn't add the account.".into());
        current.details = Some(details);
    }
    emit(svc, provider);
}

#[cfg(test)]
mod tests;

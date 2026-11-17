use super::{
    admission::{Admissions, Removal},
    capability, policy, sign_in, tabs_in_use, State, IMPLICIT_ACCOUNT,
};
use crate::service::ServiceContext;
use codeterm_types::config::provider_accounts::AccountRemovalConfig;
use serde::Serialize;
use serde_json::json;
use std::time::Duration;

pub(super) fn restore(svc: &ServiceContext, provider: &str) {
    let policy = policy(svc, provider);
    let state = svc.ext_or_init::<State>(State::default);
    let mut admissions = state.admissions.lock();
    for (account, profile) in policy.profiles {
        if let Some(removal) = profile.removal {
            admissions
                .removing
                .entry((provider.into(), account))
                .or_insert(Removal {
                    id: removal.id,
                    working: false,
                    error: Some(removal.failure),
                });
        }
    }
}

#[derive(Serialize)]
pub(super) struct Notice {
    pub id: String,
    pub kind: &'static str,
}

pub(super) fn notice(svc: &ServiceContext, provider: &str, account: &str) -> Option<Notice> {
    svc.ext_or_init::<State>(State::default)
        .admissions
        .lock()
        .notices
        .get(&(provider.into(), account.into()))
        .map(|id| Notice {
            id: id.clone(),
            kind: "inUse",
        })
}

pub(super) fn dismiss(svc: &ServiceContext, provider: &str, account: &str, id: &str) {
    let mut admissions = svc.ext_or_init::<State>(State::default).admissions.lock();
    let key = (provider.into(), account.into());
    if admissions
        .notices
        .get(&key)
        .is_some_and(|notice| notice == id)
    {
        admissions.notices.remove(&key);
    }
}

pub(super) fn request(svc: &ServiceContext, provider: &str, account: &str) -> Result<(), String> {
    restore(svc, provider);
    capability(svc, provider).ok_or("This harness does not support isolated accounts")?;
    if account == IMPLICIT_ACCOUNT {
        return Err("The usual sign-in must be removed in the harness itself".into());
    }
    if !policy(svc, provider).profiles.contains_key(account) {
        return Err("The selected harness account no longer exists".into());
    }
    if sign_in::active(svc, provider, account) {
        return Err("Finish or cancel this account's sign-in before removing it".into());
    }
    let state = svc.ext_or_init::<State>(State::default);
    let mut admissions = state.admissions.lock();
    let key = (provider.into(), account.into());
    let tabs = tabs_in_use(svc, provider, account);
    if tabs > 0
        && super::continuation(svc, provider)
            != codeterm_plugin::provider_account::storage::AccountContinuation::Proven
    {
        admissions
            .notices
            .insert(key, uuid::Uuid::new_v4().to_string());
        drop(admissions);
        emit(svc, provider);
        return Ok(());
    }
    let previous = admissions.removing.get(&key).cloned();
    if previous.as_ref().is_some_and(|removal| removal.working) {
        return Ok(());
    }
    let id = previous
        .as_ref()
        .map(|removal| removal.id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    admissions.removing.insert(
        key.clone(),
        Removal {
            id: id.clone(),
            working: false,
            error: None,
        },
    );
    drop(admissions);
    let persisted = (|| {
        super::mutate_config(svc, |config| {
            let profile = config
                .agents
                .get_mut(provider)
                .and_then(|provider| provider.accounts.as_mut())
                .and_then(|policy| policy.profiles.get_mut(account))
                .ok_or("The account no longer exists")?;
            profile.removal = Some(AccountRemovalConfig {
                id: id.clone(),
                failure: "Removal was interrupted. Try again.".into(),
            });
            Ok(())
        })
    })();
    let applied = match persisted {
        Ok(applied) => applied,
        Err(error) => {
            let mut admissions = state.admissions.lock();
            if let Some(previous) = previous {
                admissions.removing.insert(key, previous);
            } else {
                admissions.removing.remove(&key);
            }
            return Err(error);
        }
    };
    svc.ui.emit(
        "config-changed",
        serde_json::to_value(applied).unwrap_or_default(),
    );
    emit(svc, provider);
    schedule(svc, provider);
    Ok(())
}

fn reserved(admissions: &Admissions, provider: &str, account: &str) -> bool {
    admissions
        .reservations
        .values()
        .any(|(id, selected)| id == provider && selected == account)
}

pub(super) fn schedule(svc: &ServiceContext, provider: &str) {
    let Some(state) = svc.try_ext::<State>() else {
        return;
    };
    let waiting: Vec<_> = state
        .admissions
        .lock()
        .removing
        .keys()
        .filter(|(id, _)| id == provider)
        .map(|(_, account)| account.clone())
        .collect();
    for account in waiting {
        super::scheduler::evacuate(svc, provider, &account);
    }
    let candidates = {
        let mut admissions = state.admissions.lock();
        let candidates: Vec<_> = admissions
            .removing
            .iter()
            .filter(|((id, account), removal)| {
                id == provider
                    && !removal.working
                    && removal.error.is_none()
                    && !reserved(&admissions, id, account)
                    && tabs_in_use(svc, id, account) == 0
                    && !sign_in::active(svc, id, account)
            })
            .map(|(key, removal)| (key.clone(), removal.id.clone()))
            .collect();
        for (key, _) in &candidates {
            if let Some(removal) = admissions.removing.get_mut(key) {
                removal.working = true;
            }
        }
        candidates
    };
    for ((provider, account), id) in candidates {
        let owner = svc.clone();
        crate::async_runtime::spawn(async move {
            let worker = owner.clone();
            let provider_copy = provider.clone();
            let account_copy = account.clone();
            let id_copy = id.clone();
            let result = codeterm_plugin::js_runtime::spawn_blocking(move || {
                finish(&worker, &provider_copy, &account_copy, &id_copy)
            })
            .await
            .map_err(|_| "Account removal could not finish".to_owned())
            .and_then(|result| result);
            if let Err(error) = &result {
                let owner = owner.clone();
                let provider = provider.clone();
                let account = account.clone();
                let id = id.clone();
                let reason = error.clone();
                let _ = codeterm_plugin::js_runtime::spawn_blocking(move || {
                    remember_failure(&owner, &provider, &account, &id, &reason)
                })
                .await;
            }
            let state = owner.ext_or_init::<State>(State::default);
            let mut admissions = state.admissions.lock();
            let key = (provider.clone(), account);
            if admissions
                .removing
                .get(&key)
                .is_some_and(|removal| removal.id == id)
            {
                match result {
                    Ok(()) => {
                        admissions.removing.remove(&key);
                        admissions.notices.remove(&key);
                    }
                    Err(error) => {
                        if let Some(removal) = admissions.removing.get_mut(&key) {
                            removal.working = false;
                            removal.error = Some(error);
                        }
                    }
                }
            }
            drop(admissions);
            emit(&owner, &provider);
        });
    }
}

fn finish(svc: &ServiceContext, provider: &str, account: &str, id: &str) -> Result<(), String> {
    let operation = super::operation(svc, provider, account);
    let _operation = operation
        .try_lock_for(Duration::from_secs(30))
        .ok_or("The account is still finishing an operation; credentials were retained")?;
    let state = svc.ext_or_init::<State>(State::default);
    {
        let admissions = state.admissions.lock();
        if !admissions
            .removing
            .get(&(provider.into(), account.into()))
            .is_some_and(|removal| removal.id == id && removal.working)
            || reserved(&admissions, provider, account)
            || tabs_in_use(svc, provider, account) > 0
        {
            return Err("The account is still in use; credentials were retained".into());
        }
    }
    let before = policy(svc, provider);
    let profile = before
        .profiles
        .get(account)
        .ok_or("The account no longer exists")?;
    let position = before
        .order
        .iter()
        .position(|id| id == account)
        .unwrap_or_default();
    let identity = profile
        .name
        .clone()
        .or_else(|| {
            profile
                .identity
                .as_ref()
                .map(|identity| identity.label.clone())
        })
        .unwrap_or_else(|| "Account".into());
    {
        let _provisioning = state.provisioning.lock();
        for namespace in std::iter::once(profile.credential_namespace.as_deref().unwrap_or(account))
            .chain(profile.retained_namespaces.iter().map(String::as_str))
        {
            super::credentials::discard(svc, provider, account, namespace)?;
        }
    }
    let applied = super::mutate_config(svc, |config| {
        let policy = config
            .agents
            .get_mut(provider)
            .and_then(|provider| provider.accounts.as_mut())
            .ok_or("The account list changed")?;
        if policy.profiles.get(account) != Some(profile) {
            return Err("The account changed during removal".into());
        }
        policy.remove(account)?;
        policy.validate()
    })?;
    state.scheduler.lock().forget_account(provider, account);
    state
        .observations
        .lock()
        .remove(&(provider.into(), account.into()));
    state.feedback.lock().insert(
        provider.into(),
        super::roster::Feedback {
            id: uuid::Uuid::new_v4().to_string(),
            kind: "removed",
            account_id: account.into(),
            position: position.min(before.order.len().saturating_sub(2)),
            identity: Some(identity),
        },
    );
    svc.ui.emit(
        "config-changed",
        serde_json::to_value(applied).unwrap_or_default(),
    );
    Ok(())
}

fn remember_failure(svc: &ServiceContext, provider: &str, account: &str, id: &str, reason: &str) {
    let reason: String = reason
        .chars()
        .filter(|value| !value.is_control())
        .take(2048)
        .collect();
    let result = super::mutate_config(svc, |config| {
        if let Some(removal) = config
            .agents
            .get_mut(provider)
            .and_then(|provider| provider.accounts.as_mut())
            .and_then(|policy| policy.profiles.get_mut(account))
            .and_then(|profile| profile.removal.as_mut())
            .filter(|removal| removal.id == id)
        {
            removal.failure = if reason.is_empty() {
                "Account removal could not finish".into()
            } else {
                reason
            };
        }
        Ok(())
    });
    if let Ok(applied) = result {
        svc.ui.emit(
            "config-changed",
            serde_json::to_value(applied).unwrap_or_default(),
        );
    }
}

fn emit(svc: &ServiceContext, provider: &str) {
    svc.ui
        .emit("provider-accounts-changed", json!({"providerId":provider}));
}

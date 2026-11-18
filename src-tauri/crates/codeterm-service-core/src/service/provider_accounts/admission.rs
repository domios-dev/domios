use super::State;
use crate::service::agent_lifecycle::AccountRestartBinding;
use crate::service::ServiceContext;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Admissions {
    pub reservations: BTreeMap<String, (String, String)>,
    pub removing: BTreeMap<(String, String), Removal>,
    pub notices: BTreeMap<(String, String), String>,
}

#[derive(Clone)]
pub(super) struct Removal {
    pub id: String,
    pub working: bool,
    pub error: Option<String>,
}

pub(crate) struct Reservation {
    svc: ServiceContext,
    id: String,
    provider: String,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.svc
            .ext_or_init::<State>(State::default)
            .admissions
            .lock()
            .reservations
            .remove(&self.id);
        let owner = self.svc.clone();
        let provider = self.provider.clone();
        crate::async_runtime::spawn(async move {
            let _ = codeterm_plugin::js_runtime::spawn_blocking(move || {
                super::removal::schedule(&owner, &provider)
            })
            .await;
        });
    }
}

pub(crate) fn reserve(
    svc: &ServiceContext,
    provider: &str,
    account: Option<&str>,
) -> Result<Reservation, String> {
    let account = account.unwrap_or(super::IMPLICIT_ACCOUNT);
    let mut admissions = svc.ext_or_init::<State>(State::default).admissions.lock();
    let key = (provider.into(), account.into());
    if admissions.removing.contains_key(&key)
        || super::policy(svc, provider)
            .profiles
            .get(account)
            .is_some_and(|profile| profile.removal.is_some())
    {
        return Err("This account is being removed".into());
    }
    Ok(reserve_key(svc, &mut admissions, key))
}

pub(super) fn retain_pty_switch_source(
    svc: &ServiceContext,
    tab_id: &str,
    source: &AccountRestartBinding,
) -> Result<Reservation, String> {
    let mut admissions = svc.ext_or_init::<State>(State::default).admissions.lock();
    let mux = svc.mux.read();
    source.validate(mux.get_tab(tab_id).ok_or("The account tab closed")?, None)?;
    let account = source
        .account_id
        .as_deref()
        .unwrap_or(super::IMPLICIT_ACCOUNT);
    Ok(reserve_key(
        svc,
        &mut admissions,
        (source.provider.clone(), account.into()),
    ))
}

fn reserve_key(
    svc: &ServiceContext,
    admissions: &mut Admissions,
    key: (String, String),
) -> Reservation {
    let id = uuid::Uuid::new_v4().to_string();
    let provider = key.0.clone();
    admissions.reservations.insert(id.clone(), key);
    Reservation {
        svc: svc.clone(),
        id,
        provider,
    }
}

pub(super) fn available(svc: &ServiceContext, provider: &str, account: &str) -> bool {
    let blocked = svc
        .ext_or_init::<State>(State::default)
        .admissions
        .lock()
        .removing
        .contains_key(&(provider.into(), account.into()));
    !blocked
        && !super::policy(svc, provider)
            .profiles
            .get(account)
            .is_some_and(|profile| profile.removal.is_some())
}

#[cfg(test)]
mod tests;

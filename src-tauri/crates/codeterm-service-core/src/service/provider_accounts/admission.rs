use super::State;
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
    let id = uuid::Uuid::new_v4().to_string();
    admissions.reservations.insert(id.clone(), key);
    Ok(Reservation {
        svc: svc.clone(),
        id,
        provider: provider.into(),
    })
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

use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;

use codeterm_types::pty::PromptKind;
use parking_lot::Mutex;

use super::{subscription, Active, ResponseRoute, ServiceContext, ACTIVE};

#[derive(Clone, PartialEq, Eq)]
struct Identity {
    nonce: String,
    session: String,
    request: String,
}

type Attempts = HashMap<String, VecDeque<(Identity, bool)>>;
static ATTEMPTS: LazyLock<Mutex<Attempts>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) struct Reply {
    binding: subscription::Binding,
    route: ResponseRoute,
    identity: Identity,
}

fn enabled(svc: &ServiceContext, tab_id: &str) -> bool {
    svc.mux
        .read()
        .get_tab(tab_id)
        .is_some_and(|tab| tab.auto_approve || tab.permission_auto_confirm)
}

fn permission(route: &ResponseRoute) -> bool {
    route.prompt_kind() == PromptKind::Permission
}

pub(super) fn kind(entry: Option<&Active>) -> PromptKind {
    match entry {
        Some(entry)
            if permission(&entry.route) && entry.automatic_nonce.is_some() && entry.in_flight =>
        {
            PromptKind::AutomaticPermission
        }
        Some(entry) if permission(&entry.route) => PromptKind::Permission,
        _ => PromptKind::Structured,
    }
}

pub(super) fn claim(svc: &ServiceContext, tab_id: &str) -> Option<Reply> {
    if !enabled(svc, tab_id) {
        return None;
    }
    let binding = subscription::current(svc, tab_id)?;
    let mut active = ACTIVE.lock();
    let entry = active.get_mut(tab_id)?;
    if !permission(&entry.route)
        || entry.in_flight
        || entry.automatic_nonce.is_some()
        || entry.route.session_id != binding.session_id
    {
        return None;
    }
    let identity = Identity {
        nonce: binding.nonce.clone(),
        session: binding.session_id.clone(),
        request: entry.route.request_id.clone(),
    };
    let mut attempts = ATTEMPTS.lock();
    let attempts = attempts.entry(tab_id.into()).or_default();
    entry.automatic_nonce = Some(binding.nonce.clone());
    if let Some((_, hidden)) = attempts.iter().find(|(previous, _)| previous == &identity) {
        entry.in_flight = *hidden;
        return None;
    }
    attempts.push_back((identity.clone(), true));
    if attempts.len() > 64 {
        attempts.pop_front();
    }
    entry.in_flight = true;
    Some(Reply {
        binding,
        route: entry.route.clone(),
        identity,
    })
}

impl Reply {
    pub(crate) fn abandon(self, tab_id: &str) {
        let mut active = ACTIVE.lock();
        if active.get(tab_id).is_some_and(|entry| {
            entry.route == self.route
                && entry.automatic_nonce.as_deref() == Some(self.binding.nonce.as_str())
        }) {
            active.remove(tab_id);
        }
        if let Some(attempts) = ATTEMPTS.lock().get_mut(tab_id) {
            attempts.retain(|(identity, _)| identity != &self.identity);
        }
    }

    fn owns(&self, tab_id: &str) -> bool {
        ACTIVE.lock().get(tab_id).is_some_and(|entry| {
            entry.route == self.route
                && entry.in_flight
                && entry.automatic_nonce.as_deref() == Some(self.binding.nonce.as_str())
        })
    }

    pub(crate) fn dispatch(self, svc: ServiceContext, tab_id: String) {
        drop(crate::async_runtime::spawn_blocking(move || {
            self.run(&svc, &tab_id)
        }));
    }

    fn run(self, svc: &ServiceContext, tab_id: &str) {
        self.run_with(svc, tab_id, |binding, route| {
            super::send_reply(
                &binding.base_url,
                &binding.cwd,
                route,
                &serde_json::json!({"reply":"once"}),
            )
        });
    }

    fn run_with(
        self,
        svc: &ServiceContext,
        tab_id: &str,
        send: impl FnOnce(&subscription::Binding, &ResponseRoute) -> Result<(), String>,
    ) {
        let bound = subscription::current(svc, tab_id).as_ref() == Some(&self.binding);
        let owned = self.owns(tab_id);
        let current = enabled(svc, tab_id) && bound && owned;
        let result = if current {
            send(&self.binding, &self.route)
        } else {
            Err("automatic permission binding or mode changed before response".into())
        };
        if let Some(attempts) = ATTEMPTS.lock().get_mut(tab_id) {
            if let Some((_, hidden)) = attempts
                .iter_mut()
                .find(|(identity, _)| identity == &self.identity)
            {
                *hidden = result.is_ok();
            }
        }
        if let Err(error) = &result {
            log::warn!("[structured-permission] tab={tab_id} automatic response stopped: {error}");
        }
        if subscription::current(svc, tab_id).as_ref() != Some(&self.binding) {
            let removed = {
                let mut active = ACTIVE.lock();
                if active.get(tab_id).is_some_and(|entry| {
                    entry.route == self.route
                        && entry.automatic_nonce.as_deref() == Some(self.binding.nonce.as_str())
                }) {
                    active.remove(tab_id).is_some()
                } else {
                    false
                }
            };
            if removed {
                svc.push_structured_prompt(tab_id, super::pending_for_tab(tab_id));
            }
            return;
        }
        let _ = super::finish_response(
            svc,
            tab_id,
            &self.route,
            Some(&self.binding.nonce),
            result,
            0,
        );
    }
}

pub(super) fn clear(tab_id: &str) {
    ATTEMPTS.lock().remove(tab_id);
}

#[cfg(test)]
mod tests;

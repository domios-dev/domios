use crate::service::ServiceContext;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Weak},
};

#[derive(PartialEq, Eq, Hash)]
struct LaunchKey {
    host: usize,
    tab: String,
}
struct AuthorizedLaunch {
    owner: Weak<crate::lock_hold::TimedRwLock<crate::multiplexer::state::MultiplexerState>>,
    provider: String,
    binding_nonce: String,
}
static LAUNCHES: LazyLock<Mutex<HashMap<LaunchKey, AuthorizedLaunch>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn key(svc: &ServiceContext, tab: &str) -> LaunchKey {
    LaunchKey {
        host: Arc::as_ptr(&svc.mux) as usize,
        tab: tab.into(),
    }
}

pub(super) fn record_spawn(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    nonce: &str,
) -> Result<(), String> {
    let current = svc.mux.read().get_tab(tab).is_some_and(|target| {
        target.agent_type.as_deref() == Some(provider)
            && target.session_binding_nonce.as_deref() == Some(nonce)
            && matches!(&target.backing, crate::multiplexer::types::TabBacking::ChatBackend { plugin_id, .. } if plugin_id == provider)
    });
    if !current || !super::engine::requested(svc, provider) {
        return Err("Authorized runtime spawn binding changed".into());
    }
    LAUNCHES.lock().insert(
        key(svc, tab),
        AuthorizedLaunch {
            owner: Arc::downgrade(&svc.mux),
            provider: provider.into(),
            binding_nonce: nonce.into(),
        },
    );
    Ok(())
}

pub(crate) fn verifies(svc: &ServiceContext, tab: &str, provider: &str, nonce: &str) -> bool {
    let (exact, restored) = svc
        .mux
        .read()
        .get_tab(tab)
        .map(|target| {
            let exact = target.agent_type.as_deref() == Some(provider)
                && target.session_binding_nonce.as_deref() == Some(nonce);
            (
                exact,
                target.restored_from_snapshot
                    && target
                        .agent_session_id
                        .as_ref()
                        .is_some_and(|thread| !thread.is_empty()),
            )
        })
        .unwrap_or((false, false));
    let retained = LAUNCHES.lock().get(&key(svc, tab)).is_some_and(|launch| {
        launch.provider == provider
            && launch.binding_nonce == nonce
            && launch
                .owner
                .upgrade()
                .is_some_and(|owner| Arc::ptr_eq(&owner, &svc.mux))
    });
    exact && (retained || restored) && super::engine::requested(svc, provider)
}

pub(super) fn revoke(tab: &str) {
    LAUNCHES.lock().retain(|key, _| key.tab != tab);
}

use crate::service::ServiceContext;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock, Weak,
    },
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
    authorized: bool,
    epoch: Option<u64>,
    operation: Option<String>,
    generation: Option<Arc<NativeGeneration>>,
}

pub(crate) struct NativeGeneration {
    acquired: AtomicBool,
    unknown: AtomicBool,
}

impl NativeGeneration {
    pub(crate) fn mark_acquired(&self) {
        self.acquired.store(true, Ordering::Release);
    }
    pub(crate) fn unacquired(&self) -> bool {
        !self.acquired.load(Ordering::Acquire) && !self.unknown.load(Ordering::Acquire)
    }
}

const MAX_GENERATION_MARKERS: usize = 128;

// The caller holds the committing mux guard and has just minted this nonce.
pub(crate) fn seed_generation(
    svc: &ServiceContext,
    minted: crate::service::tab_session_binding::MintedGeneration<'_>,
) {
    let tab = minted.tab();
    let mut launches = LAUNCHES.lock();
    launches.retain(|_, launch| launch.owner.strong_count() > 0);
    let eligible = tab.agent_type.as_ref().zip(tab.session_binding_nonce.as_ref()).filter(|(provider, _)| {
        matches!(&tab.backing, crate::multiplexer::types::TabBacking::ChatBackend { plugin_id, .. } if plugin_id == *provider)
    });
    let Some((provider, nonce)) = eligible else {
        if let Some(previous) = launches.get_mut(&key(svc, &tab.id)) {
            previous.generation = None;
        }
        return;
    };
    if launches
        .get(&key(svc, &tab.id))
        .is_some_and(|launch| launch.binding_nonce == *nonce)
    {
        return;
    }
    let host = Arc::as_ptr(&svc.mux) as usize;
    let replaces_marker = launches
        .get(&key(svc, &tab.id))
        .is_some_and(|launch| launch.generation.is_some());
    if !replaces_marker
        && launches
            .iter()
            .filter(|(key, launch)| key.host == host && launch.generation.is_some())
            .count()
            >= MAX_GENERATION_MARKERS
    {
        let evicted = launches
            .iter()
            .find(|(key, launch)| key.host == host && launch.generation.is_some())
            .map(|(key, _)| LaunchKey {
                host: key.host,
                tab: key.tab.clone(),
            });
        if let Some(launch) = evicted.as_ref().and_then(|key| launches.get_mut(key)) {
            if let Some(generation) = launch.generation.take() {
                generation.unknown.store(true, Ordering::Release);
            }
        }
        if evicted
            .as_ref()
            .and_then(|key| launches.get(key))
            .is_some_and(|launch| !launch.authorized)
        {
            if let Some(key) = evicted {
                launches.remove(&key);
            }
        }
    }
    launches.insert(
        key(svc, &tab.id),
        AuthorizedLaunch {
            owner: Arc::downgrade(&svc.mux),
            provider: provider.clone(),
            binding_nonce: nonce.clone(),
            authorized: false,
            epoch: tab.lifecycle_epoch,
            operation: tab.lifecycle_operation_id.clone(),
            generation: Some(Arc::new(NativeGeneration {
                acquired: AtomicBool::new(false),
                unknown: AtomicBool::new(
                    minted.restored()
                        || tab.restored_from_snapshot
                        || tab.agent_session_id.is_some()
                        || tab.reported_session_binding_nonce.is_some()
                        || tab.agent_runtime_state.is_some(),
                ),
            })),
        },
    );
}

// Both callers hold mux; acquisition callers also hold SESSIONS before this map.
pub(super) fn generation(
    svc: &ServiceContext,
    tab: &crate::multiplexer::types::Tab,
) -> Option<Arc<NativeGeneration>> {
    let launches = LAUNCHES.lock();
    let launch = launches.get(&key(svc, &tab.id))?;
    (launch.authorized
        && Some(&launch.provider) == tab.agent_type.as_ref()
        && Some(&launch.binding_nonce) == tab.session_binding_nonce.as_ref()
        && launch.epoch == tab.lifecycle_epoch
        && launch.operation == tab.lifecycle_operation_id
        && launch
            .owner
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &svc.mux)))
    .then(|| launch.generation.clone())
    .flatten()
}

pub(super) fn mark_acquisition(svc: &ServiceContext, tab: &crate::multiplexer::types::Tab) {
    let launches = LAUNCHES.lock();
    if let Some(launch) = launches.get(&key(svc, &tab.id)).filter(|launch| {
        Some(&launch.provider) == tab.agent_type.as_ref()
            && Some(&launch.binding_nonce) == tab.session_binding_nonce.as_ref()
    }) {
        if let Some(generation) = &launch.generation {
            generation.mark_acquired();
        }
    }
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
    if !super::engine::requested(svc, provider) {
        return Err("Authorized runtime spawn capability changed".into());
    }
    #[cfg(any(test, feature = "test-support"))]
    super::engine::expected_launch_commit(svc, "record_spawn");
    let mux = svc.mux.read();
    let current = mux.get_tab(tab).is_some_and(|target| {
        target.agent_type.as_deref() == Some(provider)
            && target.session_binding_nonce.as_deref() == Some(nonce)
            && matches!(&target.backing, crate::multiplexer::types::TabBacking::ChatBackend { plugin_id, .. } if plugin_id == provider)
    });
    if !current {
        return Err("Authorized runtime spawn binding changed".into());
    }
    let mut launches = LAUNCHES.lock();
    let generation = launches.get(&key(svc, tab)).and_then(|previous| {
        if previous
            .owner
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &svc.mux))
            && previous.provider == provider
            && previous.binding_nonce == nonce
        {
            if !previous.authorized
                || mux.get_tab(tab).is_some_and(|tab| {
                    previous.epoch == tab.lifecycle_epoch
                        && previous.operation == tab.lifecycle_operation_id
                })
            {
                return previous.generation.clone();
            }
            if let Some(generation) = &previous.generation {
                generation.unknown.store(true, Ordering::Release);
            }
        }
        None
    });
    launches.insert(
        key(svc, tab),
        AuthorizedLaunch {
            owner: Arc::downgrade(&svc.mux),
            provider: provider.into(),
            binding_nonce: nonce.into(),
            authorized: true,
            epoch: mux.get_tab(tab).and_then(|tab| tab.lifecycle_epoch),
            operation: mux
                .get_tab(tab)
                .and_then(|tab| tab.lifecycle_operation_id.clone()),
            generation,
        },
    );
    Ok(())
}

pub fn verifies(svc: &ServiceContext, tab: &str, provider: &str, nonce: &str) -> bool {
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
        launch.authorized
            && launch.provider == provider
            && launch.binding_nonce == nonce
            && launch
                .owner
                .upgrade()
                .is_some_and(|owner| Arc::ptr_eq(&owner, &svc.mux))
    });
    exact && (retained || restored) && super::engine::requested(svc, provider)
}

#[cfg(any(test, feature = "test-support"))]
pub fn record_spawn_for_test(svc: &ServiceContext, tab: &str, provider: &str, nonce: &str) {
    record_spawn(svc, tab, provider, nonce).unwrap();
}

pub(super) fn revoke(tab: &str) {
    LAUNCHES.lock().retain(|key, launch| {
        if key.tab != tab {
            return true;
        }
        if let Some(generation) = &launch.generation {
            generation.unknown.store(true, Ordering::Release);
        }
        false
    });
}

pub fn rotate_restart(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    old_nonce: &str,
) -> Result<String, String> {
    if !verifies(svc, tab, provider, old_nonce) {
        return Err("Runtime restart authority changed".into());
    }
    let nonce = svc.rotate_session_binding_nonce(tab);
    svc.neighbors()
        .carry_neighbor_restore_grant(tab, old_nonce, &nonce)?;
    record_spawn(svc, tab, provider, &nonce)?;
    Ok(nonce)
}

pub fn authorize_failed_start_restart(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    nonce: &str,
) -> Result<(), String> {
    let failed = svc.mux.read().get_tab(tab).is_some_and(|target| {
        target.agent_type.as_deref() == Some(provider)
            && target.session_binding_nonce.as_deref() == Some(nonce)
            && super::start_failure::for_tab(target)
    });
    if !failed {
        return Err("Runtime refused-start binding changed".into());
    }
    record_spawn(svc, tab, provider, nonce)
}

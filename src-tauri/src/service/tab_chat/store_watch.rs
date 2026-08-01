//! Bounded watcher for agent sessions whose provider store is not available yet.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::agent::registry::AgentRegistry;

use super::{spawn_tailer, ChatRegistry, DeltaSink, TailConfig, TailerHandle};

pub const PENDING_STORE_WATCH_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub struct StoreWatchConfig {
    pub tab_id: String,
    pub provider_id: String,
    pub cwd: String,
    pub session_id: String,
    pub poll_interval: Duration,
}

/// A pending watcher can only promote after this registration succeeds.
pub struct PendingStoreWatch {
    handle: TailerHandle,
    tab_id: String,
    session_id: String,
    promotion_authority: Arc<AtomicBool>,
}

impl PendingStoreWatch {
    pub fn register(self, chat_registry: &ChatRegistry, subscription_id: String) -> bool {
        let Self {
            handle,
            tab_id,
            session_id,
            promotion_authority,
        } = self;
        let registered = chat_registry.insert(tab_id, session_id, subscription_id, handle);
        if registered {
            promotion_authority.store(true, Ordering::Release);
        }
        registered
    }
}

/// Promote this watch to a tailer once the provider store materializes.
/// The tailer starts at zero, so its first emission is the initial batch.
pub fn spawn_store_watch(
    cfg: StoreWatchConfig,
    agent_registry: Arc<AgentRegistry>,
    chat_registry: Arc<ChatRegistry>,
    sink: Arc<dyn DeltaSink>,
) -> PendingStoreWatch {
    spawn_store_watch_inner(cfg, agent_registry, chat_registry, sink, None)
}

fn spawn_store_watch_inner(
    cfg: StoreWatchConfig,
    agent_registry: Arc<AgentRegistry>,
    chat_registry: Arc<ChatRegistry>,
    sink: Arc<dyn DeltaSink>,
    #[cfg(test)] test_hook: Option<StoreWatchTestHook>,
    #[cfg(not(test))] _: Option<()>,
) -> PendingStoreWatch {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop = stop_flag.clone();
    let promotion_authority = Arc::new(AtomicBool::new(false));
    let watcher_authority = promotion_authority.clone();
    let tab_id = cfg.tab_id.clone();
    let session_id = cfg.session_id.clone();
    let join = thread::spawn(move || {
        #[cfg(test)]
        let mut test_hook = test_hook;
        while !stop.load(Ordering::Acquire) {
            let path = agent_registry
                .with_provider(&cfg.provider_id, |provider| {
                    provider.session_jsonl_path(&cfg.cwd, &cfg.session_id)
                })
                .flatten()
                .filter(|path| path.exists());
            if path.is_some() {
                #[cfg(test)]
                let first_store_seen = test_hook.take();
                #[cfg(test)]
                if let Some(hook) = first_store_seen.as_ref() {
                    let _ = hook.store_seen.send(());
                    hook.resume_after_store_seen.wait();
                }
                if !watcher_authority.load(Ordering::Acquire) {
                    #[cfg(test)]
                    if let Some(hook) = first_store_seen {
                        let _ = hook.authority_rejected.send(());
                    }
                    thread::sleep(cfg.poll_interval);
                    continue;
                }
                let tailer = spawn_tailer(
                    TailConfig {
                        tab_id: cfg.tab_id.clone(),
                        provider_id: cfg.provider_id.clone(),
                        cwd: cfg.cwd.clone(),
                        session_id: cfg.session_id.clone(),
                        start_offset: 0,
                        poll_interval: super::live_chat_poll_interval(),
                    },
                    agent_registry.clone(),
                    sink.clone(),
                );
                chat_registry.replace_if_current(&cfg.tab_id, &cfg.session_id, tailer);
                return;
            }
            thread::sleep(cfg.poll_interval);
        }
    });

    PendingStoreWatch {
        handle: TailerHandle::new(stop_flag, Arc::new(AtomicU64::new(0)), join),
        tab_id,
        session_id,
        promotion_authority,
    }
}

#[cfg(test)]
pub(crate) struct StoreWatchTestHook {
    pub store_seen: std::sync::mpsc::SyncSender<()>,
    pub resume_after_store_seen: Arc<std::sync::Barrier>,
    pub authority_rejected: std::sync::mpsc::SyncSender<()>,
}

#[cfg(test)]
pub(crate) fn spawn_store_watch_with_test_hook(
    cfg: StoreWatchConfig,
    agent_registry: Arc<AgentRegistry>,
    chat_registry: Arc<ChatRegistry>,
    sink: Arc<dyn DeltaSink>,
    test_hook: StoreWatchTestHook,
) -> PendingStoreWatch {
    spawn_store_watch_inner(cfg, agent_registry, chat_registry, sink, Some(test_hook))
}

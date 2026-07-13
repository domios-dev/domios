//! Bounded watcher for agent sessions whose provider store is not available yet.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::agent::registry::AgentRegistry;

use super::{spawn_tailer, ChatRegistry, DeltaSink, TailConfig, TailerHandle};

pub const PENDING_STORE_WATCH_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub struct StoreWatchConfig {
    pub pane_id: String,
    pub provider_id: String,
    pub cwd: String,
    pub session_id: String,
    pub poll_interval: Duration,
}

/// Promote this watch to a tailer once the provider store materializes.
/// The tailer starts at zero, so its first emission is the initial batch.
pub fn spawn_store_watch(
    cfg: StoreWatchConfig,
    agent_registry: Arc<AgentRegistry>,
    chat_registry: Arc<ChatRegistry>,
    sink: Arc<dyn DeltaSink>,
) -> TailerHandle {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop = stop_flag.clone();
    let join = thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            let path = agent_registry
                .with_provider(&cfg.provider_id, |provider| {
                    provider.session_jsonl_path(&cfg.cwd, &cfg.session_id)
                })
                .flatten()
                .filter(|path| path.exists());
            if path.is_some() {
                let tailer = spawn_tailer(
                    TailConfig {
                        pane_id: cfg.pane_id.clone(),
                        provider_id: cfg.provider_id.clone(),
                        cwd: cfg.cwd.clone(),
                        session_id: cfg.session_id.clone(),
                        start_offset: 0,
                        poll_interval: super::live_chat_poll_interval(),
                    },
                    agent_registry.clone(),
                    sink.clone(),
                );
                chat_registry.replace_if_current(&cfg.pane_id, &cfg.session_id, tailer);
                return;
            }
            thread::sleep(cfg.poll_interval);
        }
    });

    TailerHandle::new(stop_flag, Arc::new(AtomicU64::new(0)), join)
}

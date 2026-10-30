//! Re-probe a provider's model catalog when its CLI is rewritten.
//!
//! A CLI self-update is a file write, and a file write is an event — so the
//! catalog never has to guess with a timer whether the answer it cached still
//! describes the binary on disk.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use notify::event::{MetadataKind, ModifyKind};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use codeterm_plugin::chatbackend_provider::ChatBackendProvider;
use codeterm_plugin::provider::AgentProvider;

use crate::agent::model_catalog;

/// Keeps one watcher per provider alive for the daemon's lifetime.
// Lock order: `watchers` before `admitted`.
#[derive(Default)]
pub struct ProviderBinaryWatchers {
    watchers: Mutex<HashMap<String, InputWatch>>,
    admitted: Mutex<HashMap<String, u64>>,
}

type OnChange = Arc<dyn Fn() + Send + Sync>;
type StreamHandler = Box<dyn FnMut(notify::Result<Event>) + Send>;
type StartStream<'a> = &'a dyn Fn(&[(PathBuf, bool)], StreamHandler) -> Option<Box<dyn Send>>;

struct InputWatch {
    targets: Vec<PathBuf>,
    /// `None` until the install publishes, so a superseded stream never calls out.
    callback: Arc<Mutex<Option<OnChange>>>,
    _stream: Box<dyn Send>,
}

/// One install's place in its key's request order; only the newest publishes.
struct Admission {
    key: String,
    generation: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum WatchInstall {
    Installed(PathBuf),
    Reused,
    Unavailable,
    Superseded,
}

#[derive(Clone, PartialEq, Eq)]
struct InputStamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
    content_digest: Option<[u8; 32]>,
}

impl InputStamp {
    fn same_input(&self, other: &Self) -> bool {
        match (self.content_digest, other.content_digest) {
            (Some(left), Some(right)) => left == right,
            (None, None) => self.len == other.len && self.modified == other.modified,
            _ => false,
        }
    }
}

const MAX_INPUT_DIGEST_BYTES: u64 = 1024 * 1024;

fn bounded_content_digest(path: &std::path::Path, len: u64) -> Option<[u8; 32]> {
    if len > MAX_INPUT_DIGEST_BYTES {
        return None;
    }
    let mut reader = std::fs::File::open(path)
        .ok()?
        .take(MAX_INPUT_DIGEST_BYTES + 1);
    let mut contents = Vec::new();
    reader.read_to_end(&mut contents).ok()?;
    if contents.len() as u64 > MAX_INPUT_DIGEST_BYTES {
        return None;
    }
    Some(Sha256::digest(&contents).into())
}

fn input_stamps(targets: &[PathBuf]) -> Vec<Option<InputStamp>> {
    targets
        .iter()
        .map(|path| {
            std::fs::metadata(path).ok().map(|metadata| {
                let len = metadata.len();
                InputStamp {
                    len,
                    modified: metadata.modified().ok(),
                    content_digest: metadata
                        .is_file()
                        .then(|| bounded_content_digest(path, len))
                        .flatten(),
                }
            })
        })
        .collect()
}

fn same_input_stamps(left: &[Option<InputStamp>], right: &[Option<InputStamp>]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| match (left, right) {
                (Some(left), Some(right)) => left.same_input(right),
                (None, None) => true,
                _ => false,
            })
}

fn changes_input(event: &Event, targets: &[PathBuf]) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && !matches!(
        event.kind,
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
    ) && touches_any(event, targets)
}

fn accept_input_event(
    event: &Event,
    targets: &[PathBuf],
    stamps: &mut Vec<Option<InputStamp>>,
) -> bool {
    if !changes_input(event, targets) {
        return false;
    }
    let current = input_stamps(targets);
    if same_input_stamps(&current, stamps) {
        return false;
    }
    *stamps = current;
    true
}

impl ProviderBinaryWatchers {
    fn admit(&self, key: String) -> Admission {
        let mut admitted = self.admitted.lock();
        let generation = admitted.entry(key.clone()).or_default();
        *generation += 1;
        Admission {
            key,
            generation: *generation,
        }
    }

    /// Runs `publish` under the registry lock only while `admission` is still
    /// the newest request for its key; a superseded install changes nothing.
    fn publish<T>(
        &self,
        admission: &Admission,
        publish: impl FnOnce(&mut HashMap<String, InputWatch>) -> T,
    ) -> Option<T> {
        let mut watchers = self.watchers.lock();
        let newest = self.admitted.lock().get(&admission.key) == Some(&admission.generation);
        newest.then(|| publish(&mut watchers))
    }

    fn watch(
        &self,
        admission: Admission,
        targets: Vec<PathBuf>,
        on_change: OnChange,
    ) -> WatchInstall {
        self.watch_with(admission, targets, on_change, &start_stream)
    }

    fn watch_with(
        &self,
        admission: Admission,
        mut targets: Vec<PathBuf>,
        on_change: OnChange,
        start: StartStream<'_>,
    ) -> WatchInstall {
        targets.sort();
        targets.dedup();
        let reused = self.publish(&admission, |watchers| {
            let existing = watchers
                .get(&admission.key)
                .filter(|watch| watch.targets == targets)?;
            *existing.callback.lock() = Some(on_change.clone());
            Some(())
        });
        match reused {
            None => return WatchInstall::Superseded,
            Some(Some(())) => return WatchInstall::Reused,
            Some(None) => {}
        }
        let watch_dirs: Vec<(PathBuf, bool)> = targets
            .iter()
            .filter_map(|target| {
                let parent = target.parent()?;
                let existing = parent.ancestors().find(|path| path.is_dir())?;
                Some((existing.to_path_buf(), existing != parent))
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if watch_dirs.is_empty() {
            return match self.publish(&admission, |watchers| watchers.remove(&admission.key)) {
                Some(removed) => {
                    drop(removed);
                    WatchInstall::Unavailable
                }
                None => WatchInstall::Superseded,
            };
        }
        let watched_targets = targets.clone();
        let watch_key = admission.key.clone();
        let callback: Arc<Mutex<Option<OnChange>>> = Arc::new(Mutex::new(None));
        let event_callback = callback.clone();
        let role = format!("catalogue-watch:{}", admission.key);
        let mut stamps = input_stamps(&targets);
        let handler: StreamHandler = Box::new(move |event: notify::Result<Event>| {
            let _role = codeterm_util::named_thread::Scope::enter(&role);
            let Ok(event) = event else { return };
            let Some(on_change) = event_callback.lock().clone() else {
                return;
            };
            if !accept_input_event(&event, &watched_targets, &mut stamps) {
                if changes_input(&event, &watched_targets) {
                    log::debug!(
                        "[model_catalog] coalesced duplicate input event provider={} kind={:?}",
                        watch_key,
                        event.kind
                    );
                }
                return;
            }
            log::debug!(
                "[model_catalog] input watcher triggered refresh provider={} kind={:?} paths={:?}",
                watch_key,
                event.kind,
                event.paths
            );
            on_change();
        });
        let Some(stream) = start(&watch_dirs, handler) else {
            return WatchInstall::Unavailable;
        };
        let mut pending = Some(InputWatch {
            targets: targets.clone(),
            callback: callback.clone(),
            _stream: stream,
        });
        let replaced = self.publish(&admission, |watchers| {
            *callback.lock() = Some(on_change);
            watchers.insert(
                admission.key.clone(),
                pending.take().expect("published once"),
            )
        });
        let published = replaced.is_some();
        // Starting and stopping an FSEvents stream takes seconds under load, so
        // neither happens while other providers wait on this registry lock.
        drop((pending, replaced));
        if published {
            WatchInstall::Installed(targets.remove(0))
        } else {
            WatchInstall::Superseded
        }
    }
}

fn start_stream(dirs: &[(PathBuf, bool)], handler: StreamHandler) -> Option<Box<dyn Send>> {
    let mut watcher = notify::recommended_watcher(handler).ok()?;
    for (dir, recursive) in dirs {
        let mode = if *recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        watcher.watch(dir, mode).ok()?;
    }
    Some(Box::new(watcher))
}

#[cfg(test)]
#[path = "model_binary_watch/tests.rs"]
mod tests;

fn touches_any(event: &Event, targets: &[PathBuf]) -> bool {
    event.paths.iter().any(|path| {
        targets
            .iter()
            .any(|target| target == path || target.starts_with(path))
    })
}

/// Re-probe without the TTL after a declared catalogue input changes.
pub fn on_catalogue_input_changed(provider: Arc<dyn AgentProvider>) {
    log::debug!(
        "[model_catalog] refresh trigger source=input-watcher provider={}",
        provider.id()
    );
    crate::service::capability_probe::invalidate_and_rebuild(provider.id());
    model_catalog::invalidate_inputs(provider.id());
    refresh_agent_provider(provider, true);
}

pub fn refresh_agent_provider(provider: Arc<dyn AgentProvider>, force: bool) {
    install_in_background(provider.clone());
    if !refresh_requests().lock().begin(provider.id(), force) {
        log::debug!(
            "[model_catalog] coalesced provider refresh provider={} source=active-refresh",
            provider.id()
        );
        return;
    }
    log::debug!(
        "[model_catalog] starting provider refresh provider={} force={force}",
        provider.id()
    );
    crate::async_runtime::spawn(async move {
        let mut force = force;
        loop {
            log::debug!(
                "[model_catalog] running provider discovery provider={} force={force}",
                provider.id()
            );
            let _ = model_catalog::refresh_models(provider.clone(), force).await;
            let watched = provider.clone();
            let admission = watchers().admit(pty_key(watched.as_ref()));
            let _ =
                crate::async_runtime::spawn_blocking(move || install_admitted(watched, admission))
                    .await;
            let next = refresh_requests().lock().next(provider.id());
            let Some(queued) = next else { break };
            force = queued;
        }
    });
}

#[derive(Default)]
struct RefreshRequests(HashMap<String, Option<bool>>);

impl RefreshRequests {
    fn begin(&mut self, id: &str, force: bool) -> bool {
        if let Some(queued) = self.0.get_mut(id) {
            *queued = Some(queued.unwrap_or(false) || force);
            false
        } else {
            self.0.insert(id.to_owned(), None);
            true
        }
    }

    fn next(&mut self, id: &str) -> Option<bool> {
        match self.0.remove(id).flatten() {
            Some(force) => {
                self.0.insert(id.to_owned(), None);
                Some(force)
            }
            None => None,
        }
    }
}

fn refresh_requests() -> &'static Mutex<RefreshRequests> {
    static REQUESTS: std::sync::OnceLock<Mutex<RefreshRequests>> = std::sync::OnceLock::new();
    REQUESTS.get_or_init(Mutex::default)
}

fn watchers() -> &'static ProviderBinaryWatchers {
    static WATCHERS: std::sync::OnceLock<ProviderBinaryWatchers> = std::sync::OnceLock::new();
    WATCHERS.get_or_init(ProviderBinaryWatchers::default)
}

fn pty_key(provider: &dyn AgentProvider) -> String {
    format!("pty:{}", provider.id())
}

/// Install the watch and the refresh it triggers. A provider whose CLI is not
/// installed is simply not watched; the next forced refresh resolves it.
pub fn install(provider: Arc<dyn AgentProvider>) {
    let admission = watchers().admit(pty_key(provider.as_ref()));
    install_admitted(provider, admission);
}

fn install_admitted(provider: Arc<dyn AgentProvider>, admission: Admission) {
    let started = std::time::Instant::now();
    let mut targets = provider
        .model_catalogue_capability()
        .fingerprint_files(provider.model_credential_files());
    let fingerprinted = started.elapsed();
    let binary = provider.cli_binary().and_then(|binary| {
        match codeterm_plugin::js_runtime::probe_bin(binary).resolution {
            codeterm_plugin::js_runtime::BinResolution::Found(path) => Some(path),
            _ => None,
        }
    });
    let probed = started.elapsed();
    if let Some(binary) = &binary {
        targets.push(binary.clone());
    }
    let watched_provider = provider.clone();
    let on_change: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        on_catalogue_input_changed(watched_provider.clone());
    });
    let installed = watchers().watch(admission, targets, on_change);
    log::debug!(
        "[boot-operation] catalogue watch provider={} fingerprint_files_ms={} probe_bin_ms={} watch_ms={} outcome={:?}",
        provider.id(),
        fingerprinted.as_millis(),
        (probed - fingerprinted).as_millis(),
        (started.elapsed() - probed).as_millis(),
        installed
    );
    match installed {
        WatchInstall::Installed(path) => log::info!(
            "[model_catalog] watching catalogue input {}",
            path.display()
        ),
        WatchInstall::Unavailable => {
            log::debug!("[model_catalog] no declared catalogue inputs to watch")
        }
        WatchInstall::Reused | WatchInstall::Superseded => {}
    }
}

/// Install on the blocking pool: callers on the async runtime, an FSEvents run
/// loop or the startup path must never wait for a stream to start. Admission
/// happens here, so a later request wins however the pool orders the work.
pub fn install_in_background(provider: Arc<dyn AgentProvider>) {
    let admission = watchers().admit(pty_key(provider.as_ref()));
    crate::async_runtime::spawn_blocking(move || install_admitted(provider, admission));
}

pub fn install_chatbackend_in_background(
    provider: Arc<dyn ChatBackendProvider>,
    registry: crate::chatbackend::ChatBackendRegistryHandle,
) {
    let admission = watchers().admit(format!("chatbackend:{}", provider.id()));
    crate::async_runtime::spawn_blocking(move || {
        install_chatbackend_admitted(provider, registry, admission)
    });
}

fn install_chatbackend_admitted(
    provider: Arc<dyn ChatBackendProvider>,
    registry: crate::chatbackend::ChatBackendRegistryHandle,
    admission: Admission,
) {
    let id = provider.id().to_string();
    let targets = provider.model_catalogue_input_files();
    let refresh_registry = registry.clone();
    let refresh_id = id.clone();
    let on_change: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        model_catalog::invalidate(&refresh_id);
        let _ = refresh_registry.refresh_provider(&refresh_id);
    });
    match watchers().watch(admission, targets, on_change) {
        WatchInstall::Installed(path) => log::info!(
            "[model_catalog] watching catalogue input {}",
            path.display()
        ),
        WatchInstall::Unavailable => {
            log::debug!("[model_catalog] no declared chatBackend catalogue inputs to watch")
        }
        WatchInstall::Reused | WatchInstall::Superseded => {}
    }
}

pub fn refresh_after_settings_change(
    agents: &crate::agent::registry::AgentRegistry,
    chat_backends: &crate::chatbackend::ChatBackendRegistry,
    provider_ids: impl IntoIterator<Item = String>,
) {
    for provider_id in provider_ids {
        log::debug!("[model_catalog] refresh trigger source=settings provider={provider_id}");
        if chat_backends
            .get(&provider_id)
            .is_some_and(|backend| backend.remote_task_capability().is_none())
        {
            crate::service::capability_probe::invalidate_and_rebuild(&provider_id);
            chat_backends.refresh_after_input_change(&provider_id);
        } else {
            refresh_agent_after_settings_change(agents, &provider_id);
        }
    }
}

pub fn refresh_agent_after_settings_change(
    agents: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
) {
    crate::service::capability_probe::invalidate_and_rebuild(provider_id);
    if let Some(provider) = agents.provider_arc(provider_id) {
        model_catalog::invalidate_inputs(provider_id);
        refresh_agent_provider(provider, true);
    }
}

//! Centralized model catalog with plugin registration + user overrides.
//!
//! ## Lookup precedence
//!   1. `<data_dir>/models/<provider>.toml` — user override, replaces the
//!      registered list entirely. Persists across upgrades.
//!   2. Live discovery snapshot (when available), otherwise plugin-registered
//!      seed list.
//!
//! ## How plugins integrate
//!
//! At startup the plugin loader calls `register(provider_id, defaults)`
//! with its built-in catalog. The provider's `available_models()` /
//! `default_model()` then go through `resolve_models` / `resolve_default`
//! so the same precedence applies everywhere — settings UI, CLI listing,
//! mesh queries, etc.
//!
//! Plugin built-ins are pure data (no I/O), so registering once and
//! consulting the override at read time stays cheap.
//!
//! ## User override format
//!
//! See `essences/models.md`. README is auto-installed to
//! `<data_dir>/models/README.md` on first run.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::provider::{AgentProvider, ModelAvailability, ModelInfo, ReasoningEffortInfo};

/// One model entry as read from the TOML file. Matches `ModelInfo` plus an
/// optional alias field, kept ergonomic so the user file feels light.
#[derive(Debug, Deserialize)]
struct CatalogModel {
    id: String,
    display_name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    available: Option<bool>,
    #[serde(default)]
    featured: Option<bool>,
    #[serde(default)]
    badge: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CatalogEffort {
    id: String,
    display_name: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    /// Default model id. Optional — falls through to the registered default
    /// when absent.
    #[serde(default)]
    default: Option<String>,
    /// When `true`, the override's `[[models]]` replaces the built-in list
    /// entirely (no merge). When `false` (default), the built-in list is
    /// kept and the override merges by id: each user `[[models]]` entry
    /// either *replaces* a built-in with the same id, or *adds* a new id
    /// at the end. This is the friendlier default — adding one custom
    /// model doesn't freeze the rest of your list at install time.
    #[serde(default)]
    replace: bool,
    #[serde(default)]
    models: Vec<CatalogModel>,
    #[serde(default)]
    default_effort: Option<String>,
    #[serde(default)]
    efforts: Vec<CatalogEffort>,
}

/// A plugin's built-in catalog as registered at startup.
#[derive(Debug, Clone)]
pub struct RegisteredDefaults {
    pub models: Vec<ModelInfo>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct RegisteredEfforts {
    efforts: Vec<ReasoningEffortInfo>,
    default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub id: String,
    pub display: String,
    pub fell_back: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Seed,
    Live,
}

/// Identity of the provider CLI a snapshot was captured against. A self-update
/// rewrites the file, so size and mtime are the change signal the catalog needs
/// — no timer, just the write that actually happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryFingerprint {
    pub path: String,
    pub len: u64,
    pub modified_unix: u64,
}

/// `None` when the path does not resolve to a readable file, which is itself a
/// change worth noticing: the CLI a snapshot describes is gone.
pub fn fingerprint_path(path: &std::path::Path) -> Option<BinaryFingerprint> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or_default();
    Some(BinaryFingerprint {
        path: path.to_string_lossy().to_string(),
        len: meta.len(),
        modified_unix: modified,
    })
}

fn fingerprint_bin(bin: &str) -> Option<BinaryFingerprint> {
    fingerprint_path(&crate::js_runtime::resolved_bin_path(bin)?)
}

#[derive(Debug, Clone)]
struct ProviderModels {
    models: Vec<ModelInfo>,
    default: Option<String>,
    source: Source,
    /// Wall-clock fetch time (not `Instant`) so the TTL survives a restart via the on-disk cache.
    fetched_at: Option<SystemTime>,
    binary: Option<BinaryFingerprint>,
}

impl ProviderModels {
    fn seed(defaults: RegisteredDefaults) -> Self {
        Self {
            models: defaults.models,
            default: defaults.default,
            source: Source::Seed,
            fetched_at: None,
            binary: None,
        }
    }

    fn live(models: Vec<ModelInfo>, default: Option<String>, fetched_at: SystemTime) -> Self {
        Self {
            models,
            default,
            source: Source::Live,
            fetched_at: Some(fetched_at),
            binary: None,
        }
    }

    fn against_binary(mut self, binary: Option<BinaryFingerprint>) -> Self {
        self.binary = binary;
        self
    }
}

fn registry() -> &'static RwLock<HashMap<String, ProviderModels>> {
    static REG: OnceLock<RwLock<HashMap<String, ProviderModels>>> = OnceLock::new();
    REG.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Registered seeds survive live overlays so cache validation can always
/// compare against what the current build actually ships.
fn seed_registry() -> &'static RwLock<HashMap<String, RegisteredDefaults>> {
    static REG: OnceLock<RwLock<HashMap<String, RegisteredDefaults>>> = OnceLock::new();
    REG.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Deterministic hash of the registered seed (ids, display, descriptions,
/// availability, order, default); no registered seed hashes `null`.
fn seed_fingerprint(provider_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let seed = seed_registry().read().get(provider_id).cloned();
    let canonical = match seed {
        Some(defaults) => serde_json::json!({
            "default": defaults.default,
            "models": defaults.models,
        }),
        None => serde_json::Value::Null,
    };
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn effort_registry() -> &'static RwLock<HashMap<String, RegisteredEfforts>> {
    static REG: OnceLock<RwLock<HashMap<String, RegisteredEfforts>>> = OnceLock::new();
    REG.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Plugin entry point: register the built-in catalog for `provider_id`.
/// Idempotent — calling twice replaces the prior registration (handy for
/// hot-reloaded plugin updates and for tests). The data here is what CodeTerm
/// ships with for the current build; bump it alongside upstream releases.
pub fn register(provider_id: &str, defaults: RegisteredDefaults) {
    let unchanged = seed_registry()
        .read()
        .get(provider_id)
        .is_some_and(|prior| prior.models == defaults.models && prior.default == defaults.default);
    seed_registry()
        .write()
        .insert(provider_id.to_string(), defaults.clone());
    let mut reg = registry().write();
    // Re-registering an UNCHANGED seed (plugin rescan/hot-reload) must not drop
    // a genuine live overlay; a changed seed invalidates it by definition.
    if unchanged && reg.get(provider_id).map(|s| s.source) == Some(Source::Live) {
        return;
    }
    reg.insert(provider_id.to_string(), ProviderModels::seed(defaults));
}

/// Replace one provider's registered catalog without preserving a live overlay.
/// Capability lifecycles use this when the provider implementation itself was
/// replaced, so models from the prior implementation cannot survive a refresh.
pub fn replace(provider_id: &str, defaults: RegisteredDefaults) {
    seed_registry()
        .write()
        .insert(provider_id.to_string(), defaults.clone());
    registry()
        .write()
        .insert(provider_id.to_string(), ProviderModels::seed(defaults));
}

/// Replace one provider, resolve user override precedence once, then retain the
/// resolved in-memory snapshot for pure descriptor reads.
pub fn replace_resolved(provider_id: &str, defaults: RegisteredDefaults) -> RegisteredDefaults {
    replace(provider_id, defaults);
    let resolved = RegisteredDefaults {
        models: resolve_models(provider_id),
        default: resolve_default(provider_id),
    };
    registry().write().insert(
        provider_id.to_string(),
        ProviderModels::seed(resolved.clone()),
    );
    resolved
}

/// Remove only `provider_id` from every in-memory catalog registry.
pub fn unregister(provider_id: &str) {
    registry().write().remove(provider_id);
    seed_registry().write().remove(provider_id);
    effort_registry().write().remove(provider_id);
    override_cache().write().remove(provider_id);
}

/// Resolved snapshot used by descriptor hot paths.
pub fn registered_snapshot(provider_id: &str) -> RegisteredDefaults {
    RegisteredDefaults {
        models: resolve_models(provider_id),
        default: resolve_default(provider_id),
    }
}

pub fn register_reasoning_efforts(
    provider_id: &str,
    efforts: Vec<ReasoningEffortInfo>,
    default: Option<String>,
) {
    effort_registry().write().insert(
        provider_id.to_string(),
        RegisteredEfforts { efforts, default },
    );
}

/// Default freshness window for live discovery. A snapshot younger than this
/// is reused; older (or absent) triggers a background refresh.
pub const DEFAULT_TTL: Duration = Duration::from_secs(6 * 3600);

/// Hard cap on a single `discover_models` probe — it may shell out / hit the
/// network, so it must never hang the refresh task.
pub const DISCOVER_TIMEOUT: Duration = Duration::from_secs(8);

/// Publish a live discovery snapshot for `provider_id` and persist it to the
/// on-disk cache so it survives a restart.
pub fn put_live(provider_id: &str, models: Vec<ModelInfo>, default: Option<String>) {
    put_live_at(provider_id, models, default, None, SystemTime::now());
}

/// Publish a snapshot together with the identity of the CLI it was captured
/// against, so a later self-update can invalidate it.
pub fn put_live_against(
    provider_id: &str,
    models: Vec<ModelInfo>,
    default: Option<String>,
    binary: Option<BinaryFingerprint>,
) {
    put_live_at(provider_id, models, default, binary, SystemTime::now());
}

fn put_live_at(
    provider_id: &str,
    models: Vec<ModelInfo>,
    default: Option<String>,
    binary: Option<BinaryFingerprint>,
    fetched_at: SystemTime,
) {
    let snapshot =
        ProviderModels::live(honest_verdicts(&models), default, fetched_at).against_binary(binary);
    persist_cache(provider_id, &snapshot);
    registry().write().insert(provider_id.to_string(), snapshot);
}

/// A probe that could not run is not a capability answer. An unknown verdict
/// keeps its own name and never travels as a definite negative.
fn honest_verdicts(models: &[ModelInfo]) -> Vec<ModelInfo> {
    models
        .iter()
        .map(|model| {
            if !model.is_unknown() {
                return model.clone();
            }
            ModelInfo {
                available: None,
                availability: Some(ModelAvailability::Unknown),
                ..model.clone()
            }
        })
        .collect()
}

/// Load a persisted live snapshot into the registry at startup so the picker shows
/// the last-known list instantly. Won't overwrite an already-live snapshot; no-op without a cache file.
pub fn prime_from_cache(provider_id: &str) {
    let Some(cached) = read_cache(provider_id) else {
        return;
    };
    let Some(fetched_at) = cached.fetched_at_system() else {
        return;
    };
    let current = seed_fingerprint(provider_id);
    if cached.seed_fingerprint.as_deref() != Some(current.as_str()) {
        log::warn!(
            "[model_catalog] {provider_id} cache was captured against a different seed \
             (or predates seed fingerprinting) — keeping the registered seed"
        );
        return;
    }
    if binary_moved(cached.binary.as_ref()) {
        log::info!(
            "[model_catalog] {provider_id} cache describes a provider binary that changed underneath it — keeping the registered seed until a fresh probe"
        );
        return;
    }
    {
        let reg = registry().read();
        if reg.get(provider_id).map(|s| s.source) == Some(Source::Live) {
            return;
        }
    }
    registry().write().insert(
        provider_id.to_string(),
        ProviderModels::live(cached.models, cached.default, fetched_at)
            .against_binary(cached.binary),
    );
}

/// True when the recorded CLI is no longer the file the snapshot described. A
/// snapshot that named no binary describes nothing that can change.
fn binary_moved(recorded: Option<&BinaryFingerprint>) -> bool {
    let Some(recorded) = recorded else {
        return false;
    };
    fingerprint_path(std::path::Path::new(&recorded.path)).as_ref() != Some(recorded)
}

/// Whether the live snapshot in memory still describes the CLI on disk. An
/// answer about a binary stops being an answer when that binary is replaced.
pub fn provider_binary_changed(provider_id: &str) -> bool {
    let recorded = registry()
        .read()
        .get(provider_id)
        .and_then(|snapshot| snapshot.binary.clone());
    binary_moved(recorded.as_ref())
}

/// Returns true when the provider has no live snapshot or the snapshot is older
/// than `ttl` at the caller-provided time.
pub fn is_stale(provider_id: &str, ttl: Duration, now: SystemTime) -> bool {
    let registered = registry().read();
    let Some(snapshot) = registered.get(provider_id) else {
        return true;
    };
    if snapshot.source != Source::Live {
        return true;
    }
    let Some(fetched_at) = snapshot.fetched_at else {
        return true;
    };
    now.duration_since(fetched_at).is_ok_and(|age| age >= ttl)
}

/// TTL-gated refresh: skip when a fresh snapshot exists unless `force`. Returns the
/// new list on success, `None` when skipped or failed (the prior snapshot/seed stays active).
pub async fn refresh_models(
    provider: Arc<dyn AgentProvider>,
    force: bool,
) -> Option<Vec<ModelInfo>> {
    let provider_id = provider.id().to_string();
    if !force && !is_stale(&provider_id, DEFAULT_TTL, SystemTime::now()) {
        log::debug!("[model_catalog] {provider_id} live cache fresh — skipping refresh");
        return None;
    }
    refresh_live(provider, DISCOVER_TIMEOUT).await
}

/// The event-driven refresh: a resolved provider binary was rewritten, so the
/// answer captured against the old one is void. Forced, because the TTL knows
/// nothing about a file write.
pub async fn refresh_after_binary_change(
    provider: Arc<dyn AgentProvider>,
) -> Option<Vec<ModelInfo>> {
    log::info!(
        "[model_catalog] {} provider binary changed - re-probing",
        provider.id()
    );
    refresh_models(provider, true).await
}

/// The user opened the picker, which is an explicit action rather than a poll.
/// TTL-gated so an open costs at most one probe per window, except when the
/// binary itself changed or the last probe could not run at all.
pub async fn refresh_for_reader(provider: Arc<dyn AgentProvider>) -> Option<Vec<ModelInfo>> {
    let provider_id = provider.id().to_string();
    let must_retry = provider_binary_changed(&provider_id) || has_unknown_verdict(&provider_id);
    if !must_retry && !is_stale(&provider_id, DEFAULT_TTL, SystemTime::now()) {
        return None;
    }
    refresh_models(provider, true).await
}

/// True when the active snapshot still carries a verdict a probe could not
/// reach. An unknown answer is retried, never settled by waiting.
pub fn has_unknown_verdict(provider_id: &str) -> bool {
    registry()
        .read()
        .get(provider_id)
        .is_some_and(|snapshot| snapshot.models.iter().any(ModelInfo::is_unknown))
}

/// Refresh live models off the async runtime. On timeout / opt-out / failure — or an
/// empty post-sanitize result — the previous snapshot stays active (the list never goes empty).
pub async fn refresh_live(
    provider: Arc<dyn AgentProvider>,
    timeout_duration: Duration,
) -> Option<Vec<ModelInfo>> {
    let provider_id = provider.id().to_string();
    let default = provider.default_model();
    // blocking pool. Use an independent executor, not `Handle::block_on` (deadlocks nested in spawn_blocking).
    let task = tokio::task::spawn_blocking(move || {
        // A refresh must observe current filesystem and PATH state before live discovery.
        crate::js_runtime::forget_resolved_bins();
        let models = futures_executor::block_on(provider.discover_models())?;
        let models = sanitize_live(models);
        if models.is_empty() {
            log::warn!(
                "[model_catalog] {provider_id} discovery returned no usable models — keeping seed"
            );
            return None;
        }
        let default = resolve_live_default(&models, default);
        let binary = provider.cli_binary().and_then(fingerprint_bin);
        put_live_against(&provider_id, models.clone(), default, binary);
        Some(models)
    });
    match tokio::time::timeout(timeout_duration, task).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) | Err(_) => None,
    }
}

/// Provider-agnostic cleanup of a discovery list: drop empty ids and non-chat
/// models, dedupe by id. Purely content heuristics, no provider-id branching.
fn sanitize_live(models: Vec<ModelInfo>) -> Vec<ModelInfo> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(models.len());
    for m in models {
        let id = m.id.trim();
        if id.is_empty() || is_non_chat_id(id) {
            continue;
        }
        let key = id.to_lowercase();
        if seen.insert(key) {
            out.push(m);
        }
    }
    honest_verdicts(&out)
}

/// Heuristic: ids that name an embedding / non-chat model. Conservative — only
/// well-known non-chat markers, so a real chat model is never dropped.
fn is_non_chat_id(id: &str) -> bool {
    let lower = id.to_lowercase();
    lower.contains("embed") || lower.contains("embedding") || lower.contains("text-moderation")
}

/// If the seed default still names a model present in the live list, keep it;
/// otherwise fall back to the first live model so a vanished hardcoded default
/// doesn't dangle. `None` when the list is empty.
fn resolve_live_default(models: &[ModelInfo], seed_default: Option<String>) -> Option<String> {
    if let Some(d) = seed_default {
        if models.iter().any(|m| m.id.eq_ignore_ascii_case(&d)) {
            return Some(d);
        }
    }
    models.first().map(|m| m.id.clone())
}

/// Read-side: returns the resolved model list using these rules —
///   * No override file        → registered defaults
///   * Override + `replace=true`  → override list verbatim (no merge)
///   * Override + `replace=false` (default) → registered defaults merged
///     with the user's `[[models]]` by id: same id replaces the built-in
///     (keeps position), new id is appended. This is the friendlier
///     default — built-in upgrades flow in automatically without
///     stomping user additions.
///
/// Unregistered provider returns the override list as-is (or empty).
pub fn resolve_models(provider_id: &str) -> Vec<ModelInfo> {
    let registered = registered_models(provider_id);
    let mut models = match load_override(provider_id) {
        None => registered,
        Some(o) if o.replace => o.models,
        Some(o) => merge_models(&registered, &o.models),
    };
    // Featured models lead, but the catalog's own order survives within each
    // group — `sort_by_key` is stable, so nothing else is reordered.
    models.sort_by_key(|model| !model.is_featured());
    models
}

fn registered_models(provider_id: &str) -> Vec<ModelInfo> {
    let snapshot = registry().read().get(provider_id).cloned();
    let Some(snapshot) = snapshot else {
        return Vec::new();
    };
    if snapshot.source != Source::Live {
        return snapshot.models;
    }
    let seed = seed_registry()
        .read()
        .get(provider_id)
        .map(|defaults| defaults.models.clone());
    match seed {
        Some(seed) => merge_models(&seed, &snapshot.models),
        None => snapshot.models,
    }
}

/// Same precedence for the default model id.
pub fn resolve_default(provider_id: &str) -> Option<String> {
    if let Some(o) = load_override(provider_id) {
        if let Some(d) = o.default {
            return Some(d);
        }
    }
    registry()
        .read()
        .get(provider_id)
        .and_then(|d| d.default.clone())
}

pub fn resolve_reasoning_efforts(provider_id: &str) -> Vec<ReasoningEffortInfo> {
    let registered = effort_registry()
        .read()
        .get(provider_id)
        .map(|defaults| defaults.efforts.clone())
        .unwrap_or_default();
    match load_override(provider_id) {
        None => registered,
        Some(override_file) if override_file.replace => override_file.efforts,
        Some(override_file) => merge_efforts(&registered, &override_file.efforts),
    }
}

pub fn resolve_default_reasoning_effort(provider_id: &str) -> Option<String> {
    if let Some(default) = load_override(provider_id).and_then(|o| o.default_effort) {
        return Some(default);
    }
    effort_registry()
        .read()
        .get(provider_id)
        .and_then(|defaults| defaults.default.clone())
}

pub fn resolve_model(provider_id: &str, requested: Option<&str>) -> Resolved {
    let models = resolve_models(provider_id);
    let requested = requested.map(str::trim).filter(|m| !m.is_empty());
    let requested_is_declared =
        requested.is_some_and(|needle| models.iter().any(|model| model_matches(model, needle)));

    if let Some(requested) = requested {
        if let Some(model) = find_model(&models, requested) {
            return resolved_from_model(model, false);
        }
    }

    if let Some(default) = resolve_default(provider_id).and_then(|id| non_empty(id.as_str())) {
        if let Some(model) = find_model(&models, &default) {
            return resolved_from_model(model, requested.is_some());
        }
    }

    if let Some(model) = models.iter().find(|model| model.available != Some(false)) {
        return resolved_from_model(model, requested.is_some());
    }

    if let Some(requested) = requested.filter(|_| !requested_is_declared) {
        return Resolved {
            id: requested.to_string(),
            display: requested.to_string(),
            fell_back: false,
        };
    }

    Resolved {
        id: String::new(),
        display: String::new(),
        fell_back: false,
    }
}

pub fn resolve_family_model(provider_id: &str, family: &str) -> Option<String> {
    let family = family.trim();
    if family.is_empty() {
        return None;
    }
    let family_lower = family.to_lowercase();
    let models = resolve_models(provider_id);
    let matches: Vec<&ModelInfo> = models
        .iter()
        .filter(|m| m.id.to_lowercase().contains(&family_lower))
        .collect();
    match matches.as_slice() {
        [] => None,
        [model] => Some(model.id.clone()),
        many => resolve_default(provider_id)
            .and_then(|default| {
                many.iter()
                    .find(|m| m.id.eq_ignore_ascii_case(&default))
                    .map(|m| m.id.clone())
            })
            .or_else(|| Some(family.to_string())),
    }
}

/// Like `resolve_models` but returns `None` when no user override file is
/// present. Use this when the plugin has custom default-resolution logic
/// (e.g. dynamic CLI probing) that should keep running unless the user
/// explicitly pinned a list. NOTE: this skips merge — calling code that
/// uses `try_override` opts out of automatic built-in updates.
pub fn try_override(provider_id: &str) -> Option<Vec<ModelInfo>> {
    load_override(provider_id).map(|o| o.models)
}

/// Same as `try_override` but for the default model id.
pub fn try_override_default(provider_id: &str) -> Option<String> {
    load_override(provider_id).and_then(|o| o.default)
}

/// Result of loading a provider override.
struct ProviderOverride {
    default: Option<String>,
    replace: bool,
    models: Vec<ModelInfo>,
    default_effort: Option<String>,
    efforts: Vec<ReasoningEffortInfo>,
}

pub fn models_dir() -> PathBuf {
    // NOTE: `codeterm_paths::data_dir()` must be provided by codeterm-plugin consumers.
    // This references a `paths` module that needs to be available in this crate's scope.
    let dir = codeterm_paths::data_dir().join("models");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn override_path(provider_id: &str) -> PathBuf {
    models_dir().join(format!("{}.toml", provider_id))
}

fn cache_path(provider_id: &str) -> PathBuf {
    models_dir().join(format!("{}.cache.json", provider_id))
}

/// On-disk persisted live snapshot. `fetched_at_unix` is wall-clock seconds so
/// the TTL survives a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedSnapshot {
    fetched_at_unix: u64,
    #[serde(default)]
    default: Option<String>,
    models: Vec<ModelInfo>,
    /// Fingerprint of the registered seed this snapshot was captured against.
    /// Absent (legacy cache) or mismatched → the snapshot never primes.
    #[serde(default)]
    seed_fingerprint: Option<String>,
    /// The provider CLI this snapshot describes. Changed underneath → the
    /// snapshot never primes, because it answers about a file that is gone.
    #[serde(default)]
    binary: Option<BinaryFingerprint>,
}

impl CachedSnapshot {
    fn fetched_at_system(&self) -> Option<SystemTime> {
        Some(UNIX_EPOCH + Duration::from_secs(self.fetched_at_unix))
    }
}

/// Write a live snapshot to disk (best-effort — a failed write only loses the
/// next-boot warm start, never the in-memory list).
fn persist_cache(provider_id: &str, snapshot: &ProviderModels) {
    let Some(fetched_at) = snapshot.fetched_at else {
        return;
    };
    let Ok(secs) = fetched_at.duration_since(UNIX_EPOCH) else {
        return;
    };
    let payload = CachedSnapshot {
        fetched_at_unix: secs.as_secs(),
        default: snapshot.default.clone(),
        models: honest_verdicts(&snapshot.models),
        seed_fingerprint: Some(seed_fingerprint(provider_id)),
        binary: snapshot.binary.clone(),
    };
    let path = cache_path(provider_id);
    match serde_json::to_string_pretty(&payload) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::warn!("[model_catalog] could not write {}: {}", path.display(), e);
            }
        }
        Err(e) => log::warn!(
            "[model_catalog] could not serialize {} cache: {}",
            provider_id,
            e
        ),
    }
}

fn read_cache(provider_id: &str) -> Option<CachedSnapshot> {
    let path = cache_path(provider_id);
    let raw = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<CachedSnapshot>(&raw) {
        Ok(c) => Some(c),
        Err(e) => {
            log::warn!(
                "[model_catalog] {} parse error — ignoring cache: {}",
                path.display(),
                e
            );
            None
        }
    }
}

/// In-memory cache of parsed override files, keyed by provider id. Each
/// entry remembers the file's last-modified time so we re-read only when
/// the user actually edits the file. Crucial because `resolve_models` is
/// called from `describe()`, which is documented as I/O-free and runs
/// under the agent registry lock — uncached `read_to_string` per call
/// pegs disk on any settings-panel render burst.
#[derive(Clone)]
struct CachedOverride {
    /// `None` = we verified the file does not exist (no override active).
    /// `Some` = parsed file body.
    parsed: Option<std::sync::Arc<ProviderOverrideOwned>>,
    /// `None` = file did not exist when we checked.
    mtime: Option<std::time::SystemTime>,
}

#[derive(Clone)]
struct ProviderOverrideOwned {
    default: Option<String>,
    replace: bool,
    models: Vec<ModelInfo>,
    default_effort: Option<String>,
    efforts: Vec<ReasoningEffortInfo>,
}

fn override_cache() -> &'static RwLock<HashMap<String, CachedOverride>> {
    static CACHE: OnceLock<RwLock<HashMap<String, CachedOverride>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn load_override(provider_id: &str) -> Option<ProviderOverride> {
    let path = override_path(provider_id);
    let current_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

    // Fast path: cached entry valid against current mtime.
    if let Some(cached) = override_cache().read().get(provider_id) {
        if cached.mtime == current_mtime {
            return cached.parsed.as_ref().map(|o| ProviderOverride {
                default: o.default.clone(),
                replace: o.replace,
                models: o.models.clone(),
                default_effort: o.default_effort.clone(),
                efforts: o.efforts.clone(),
            });
        }
    }

    // Slow path: file changed (or first read) — parse and update the cache.
    let parsed_arc: Option<std::sync::Arc<ProviderOverrideOwned>> = {
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => None.into_iter().next().unwrap_or_else(String::new),
        };
        if raw.is_empty() {
            None
        } else {
            match toml::from_str::<CatalogFile>(&raw) {
                Err(e) => {
                    log::warn!(
                        "[model_catalog] {} parse error — falling back to plugin defaults: {}",
                        path.display(),
                        e
                    );
                    None
                }
                Ok(p) if p.models.is_empty() && p.efforts.is_empty() && !p.replace => {
                    log::warn!(
                        "[model_catalog] {} has no `[[models]]` or `[[efforts]]` entries — falling back to plugin defaults",
                        path.display()
                    );
                    None
                }
                Ok(p) => Some(std::sync::Arc::new(ProviderOverrideOwned {
                    default: p.default,
                    replace: p.replace,
                    models: p
                        .models
                        .into_iter()
                        .map(|m| ModelInfo {
                            id: m.id,
                            display_name: m.display_name,
                            description: m.description,
                            available: m.available,
                            availability: None,
                            featured: m.featured,
                            badge: m.badge,
                        })
                        .collect(),
                    default_effort: p.default_effort,
                    efforts: p
                        .efforts
                        .into_iter()
                        .map(|effort| ReasoningEffortInfo {
                            id: effort.id,
                            display_name: effort.display_name,
                            description: effort.description,
                        })
                        .collect(),
                })),
            }
        }
    };

    // Re-stat after the read so the cached mtime reflects the file content we parsed — not the mtime we sampled before reading.
    let cached_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    override_cache().write().insert(
        provider_id.to_string(),
        CachedOverride {
            parsed: parsed_arc.clone(),
            mtime: cached_mtime,
        },
    );

    parsed_arc.map(|o| ProviderOverride {
        default: o.default.clone(),
        replace: o.replace,
        models: o.models.clone(),
        default_effort: o.default_effort.clone(),
        efforts: o.efforts.clone(),
    })
}

/// Merge built-in models with user overrides, by id. Built-in order is
/// preserved; each user entry either replaces an entry with the same id
/// (keeping its original position) or, if the id is new, gets appended at
/// the end. Result: built-in updates flow in automatically, user additions
/// stick around, user re-titles of built-ins win.
fn merge_models(builtin: &[ModelInfo], overrides: &[ModelInfo]) -> Vec<ModelInfo> {
    merge_by_id(builtin, overrides, |model| model.id.as_str())
}

fn merge_efforts(
    builtin: &[ReasoningEffortInfo],
    overrides: &[ReasoningEffortInfo],
) -> Vec<ReasoningEffortInfo> {
    merge_by_id(builtin, overrides, |effort| effort.id.as_str())
}

fn merge_by_id<T, F>(builtin: &[T], overrides: &[T], id: F) -> Vec<T>
where
    T: Clone,
    F: for<'a> Fn(&'a T) -> &'a str,
{
    let mut merged: Vec<T> = builtin
        .iter()
        .map(|entry| {
            overrides
                .iter()
                .find(|override_entry| id(override_entry) == id(entry))
                .cloned()
                .unwrap_or_else(|| entry.clone())
        })
        .collect();
    for override_entry in overrides {
        if !merged.iter().any(|entry| id(entry) == id(override_entry)) {
            merged.push(override_entry.clone());
        }
    }
    merged
}

fn find_model<'a>(models: &'a [ModelInfo], needle: &str) -> Option<&'a ModelInfo> {
    models
        .iter()
        .find(|model| model.available != Some(false) && model_matches(model, needle))
}

fn model_matches(model: &ModelInfo, needle: &str) -> bool {
    model.id.eq_ignore_ascii_case(needle) || model.display_name.eq_ignore_ascii_case(needle)
}

fn resolved_from_model(model: &ModelInfo, fell_back: bool) -> Resolved {
    Resolved {
        id: model.id.clone(),
        display: model.display_name.clone(),
        fell_back,
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Auto-install the README documenting the catalog format. Called once on
/// startup so the user discovers the directory naturally.
pub fn ensure_readme_installed() {
    let dir = models_dir();
    let readme = dir.join("README.md");
    if readme.exists() {
        return;
    }
    if let Err(e) = std::fs::write(&readme, README_BODY) {
        log::warn!(
            "[model_catalog] could not write {}: {}",
            readme.display(),
            e
        );
    }
}

/// Refresh `<id>.example.toml` for every registered provider. The .example
/// files are ALWAYS overwritten so they reflect the current build's
/// defaults — that's their whole point. The user's `<id>.toml` (no
/// `.example`) is the override and is never touched here. To enable an
/// override the user copies `claude.example.toml` → `claude.toml`,
/// edits, and saves.
pub fn refresh_example_files() {
    let dir = models_dir();
    let registered_efforts = effort_registry().read().clone();
    let snapshot: Vec<(String, RegisteredDefaults, RegisteredEfforts)> = seed_registry()
        .read()
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.clone(),
                registered_efforts.get(k).cloned().unwrap_or_default(),
            )
        })
        .collect();
    for (id, defaults, efforts) in snapshot {
        let path = dir.join(format!("{}.example.toml", id));
        let body = render_example(&id, &defaults, &efforts);
        if let Err(e) = std::fs::write(&path, body) {
            log::warn!("[model_catalog] could not write {}: {}", path.display(), e);
        }
    }
}

fn render_example(
    provider_id: &str,
    defaults: &RegisteredDefaults,
    effort_defaults: &RegisteredEfforts,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Example model catalog for `{0}`.\n\
         #\n\
         # To enable an override, copy this file to `{0}.toml` (drop the\n\
         # `.example` suffix). CodeTerm picks up `{0}.toml` on the next read\n\
         # — no restart. Delete it to fall back to built-in defaults.\n\
         #\n\
         # ## Merge semantics (default)\n\
         #\n\
         # By default, `[[models]]` and `[[efforts]]` MERGE with built-ins by id:\n\
         #   - Same id          → your entry replaces the built-in (keeps order).\n\
         #   - New id           → your entry is APPENDED at the end.\n\
         #   - Built-in id you didn't mention → stays, auto-updates on release.\n\
         #\n\
         # That means: adding one custom model doesn't freeze the rest of\n\
         # your list — when a new CodeTerm release ships an updated built-in\n\
         # catalog, it arrives automatically alongside your customizations.\n\
         #\n\
         # ## Full replacement\n\
         #\n\
         # If you want absolute control and no built-in updates ever, set\n\
         # `replace = true` at the top. Then this file is the entire list,\n\
         # and built-ins are ignored.\n\
         #\n\
         # This `.example` file is regenerated on every CodeTerm start so you\n\
         # can always see what the current build ships with.\n\n",
        provider_id
    ));
    out.push_str("# replace = true   # uncomment to disable merge with built-ins\n\n");
    if let Some(d) = &defaults.default {
        out.push_str(&format!("default = \"{}\"\n\n", d));
    }
    if let Some(default) = &effort_defaults.default {
        out.push_str(&format!("default_effort = \"{}\"\n\n", default));
    }
    for m in &defaults.models {
        out.push_str("[[models]]\n");
        out.push_str(&format!("id = \"{}\"\n", m.id));
        out.push_str(&format!(
            "display_name = \"{}\"\n",
            m.display_name.replace('"', "\\\"")
        ));
        if let Some(desc) = &m.description {
            out.push_str(&format!(
                "description = \"{}\"\n",
                desc.replace('"', "\\\"")
            ));
        }
        if m.is_featured() {
            out.push_str("featured = true\n");
        }
        if let Some(badge) = &m.badge {
            out.push_str(&format!("badge = \"{}\"\n", badge.replace('"', "\\\"")));
        }
        out.push('\n');
    }
    for effort in &effort_defaults.efforts {
        out.push_str("[[efforts]]\n");
        out.push_str(&format!("id = \"{}\"\n", effort.id));
        out.push_str(&format!(
            "display_name = \"{}\"\n",
            effort.display_name.replace('"', "\\\"")
        ));
        if let Some(description) = &effort.description {
            out.push_str(&format!(
                "description = \"{}\"\n",
                description.replace('"', "\\\"")
            ));
        }
        out.push('\n');
    }
    out
}

const README_BODY: &str = r#"# CodeTerm Model Catalog

This folder lets you override the model list for any agent provider without
rebuilding CodeTerm. Useful when upstream ships new models faster than we cut
releases.

## Quick start

For every provider with a model picker, CodeTerm drops a
`<provider>.example.toml` here on each startup. To enable an override:

```bash
cd ~/.codeterm/models/
cp claude.example.toml claude.toml
# edit claude.toml — change models or reasoning efforts, set defaults
```

CodeTerm picks `claude.toml` up immediately (no restart). Static provider
catalogs merge overrides by default; dynamic providers may treat an override as
a pinned list. Delete `claude.toml` (the override; NOT the `.example` file) to
fall back to provider defaults.

## Files

- `claude.toml`, `codex.toml`, `opencode.toml` — **your overrides** (only
  present if you copied the example). Never touched by CodeTerm.
- `claude.example.toml`, etc. — **always-current built-ins**, rewritten on
  every startup so you can see what ships with this build.
- `README.md` — this file. Auto-installed on first run.

## Format

```toml
# Optional: which model is selected by default for fresh sessions.
default = "example-model"
default_effort = "medium"

[[models]]
id = "example-model"
display_name = "Example Model (extended context)"
description = "Best for everyday, complex tasks."
featured = true
badge = "own"

[[models]]
id = "example-model-mini"
display_name = "Example Model Mini"
description = "Efficient for routine tasks."

[[efforts]]
id = "xhigh"
display_name = "Extra high"
description = "Maximum reasoning depth."
```

- `id` MUST match what the underlying CLI accepts (`claude --model <id>`,
  `codex -m <id>`, `opencode --model <provider>/<model>`). Wrong ids cause
  launch failures.
- `display_name` is what the picker UI shows.
- `description` is optional secondary text.
- `featured = true` lists this model ahead of the rest, preserving catalog
  order within each group.
- `badge` is an optional short chip shown beside the model in spawn pickers.
- `[[efforts]]` entries extend or replace accepted reasoning-effort ids using
  the same merge rules as models.

## Merge vs replace

By default, your override **merges** with built-ins by id:
- Same id      → your entry replaces the built-in (keeps original order).
- New id       → your entry is appended at the end.
- Built-in id you didn't mention → stays. Auto-updates on each CodeTerm
  release.

That's the default because it's what most people want: add one custom
model, keep getting upstream updates for everything else.

If you want absolute control and no built-in updates ever, set
`replace = true` at the top of your `<provider>.toml`. Then your file is
the entire list, period.

## Lookup precedence

1. `<this dir>/<provider>.toml` (override) merged with built-ins, OR replacing
   them entirely if `replace = true`.
2. Provider defaults: either the provider's live CLI catalog when available,
   or plugin defaults shipped with the current CodeTerm build.

Syntax error or no `[[models]]` / `[[efforts]]` entries? CodeTerm logs a warning
and falls back to built-ins — the picker never goes empty.

## Full docs

See `essences/models.md` in the CodeTerm repository for the long-form reference
(plugin contract, how to add a new provider, etc.).
"#;

#[cfg(test)]
mod tests;

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
//! See `docs/MODELS.md`. README is auto-installed to
//! `<data_dir>/models/README.md` on first run.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::provider::{AgentProvider, ModelInfo, ReasoningEffortInfo};

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

#[derive(Debug, Clone)]
struct ProviderModels {
    models: Vec<ModelInfo>,
    default: Option<String>,
    source: Source,
    /// Wall-clock fetch time (not `Instant`) so the TTL survives a restart via the on-disk cache.
    fetched_at: Option<SystemTime>,
}

impl ProviderModels {
    fn seed(defaults: RegisteredDefaults) -> Self {
        Self {
            models: defaults.models,
            default: defaults.default,
            source: Source::Seed,
            fetched_at: None,
        }
    }

    fn live(models: Vec<ModelInfo>, default: Option<String>, fetched_at: SystemTime) -> Self {
        Self {
            models,
            default,
            source: Source::Live,
            fetched_at: Some(fetched_at),
        }
    }

    fn as_defaults(&self) -> RegisteredDefaults {
        RegisteredDefaults {
            models: self.models.clone(),
            default: self.default.clone(),
        }
    }
}

fn registry() -> &'static RwLock<HashMap<String, ProviderModels>> {
    static REG: OnceLock<RwLock<HashMap<String, ProviderModels>>> = OnceLock::new();
    REG.get_or_init(|| RwLock::new(HashMap::new()))
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
    registry()
        .write()
        .insert(provider_id.to_string(), ProviderModels::seed(defaults));
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
    put_live_at(provider_id, models, default, SystemTime::now());
}

fn put_live_at(
    provider_id: &str,
    models: Vec<ModelInfo>,
    default: Option<String>,
    fetched_at: SystemTime,
) {
    let snapshot = ProviderModels::live(models, default, fetched_at);
    persist_cache(provider_id, &snapshot);
    registry().write().insert(provider_id.to_string(), snapshot);
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
    {
        let reg = registry().read();
        if reg.get(provider_id).map(|s| s.source) == Some(Source::Live) {
            return;
        }
    }
    registry().write().insert(
        provider_id.to_string(),
        ProviderModels::live(cached.models, cached.default, fetched_at),
    );
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
        let models = futures_executor::block_on(provider.discover_models())?;
        let models = sanitize_live(models);
        if models.is_empty() {
            log::warn!(
                "[model_catalog] {provider_id} discovery returned no usable models — keeping seed"
            );
            return None;
        }
        let default = resolve_live_default(&models, default);
        put_live(&provider_id, models.clone(), default);
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
    out
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
    let registered = registry()
        .read()
        .get(provider_id)
        .map(|d| d.models.clone())
        .unwrap_or_default();
    match load_override(provider_id) {
        None => registered,
        Some(o) if o.replace => o.models,
        Some(o) => merge_models(&registered, &o.models),
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

    if let Some(requested) = requested {
        if let Some(model) = find_model(&models, requested) {
            return resolved_from_model(model, false);
        }
    }

    if let Some(default) = resolve_default(provider_id).and_then(|id| non_empty(id.as_str())) {
        if let Some(model) = find_model(&models, &default) {
            return resolved_from_model(model, requested.is_some());
        }
        return Resolved {
            id: default.clone(),
            display: default,
            fell_back: requested.is_some(),
        };
    }

    if let Some(requested) = requested {
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
        models: snapshot.models.clone(),
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
        .find(|m| m.id.eq_ignore_ascii_case(needle) || m.display_name.eq_ignore_ascii_case(needle))
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
    let snapshot: Vec<(String, RegisteredDefaults, RegisteredEfforts)> = registry()
        .read()
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.as_defaults(),
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
         # your list — when a new CodeTerm release ships `opus-4.8`, it\n\
         # arrives automatically alongside your customizations.\n\
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
default = "opus"
default_effort = "medium"

[[models]]
id = "opus"
display_name = "Claude Opus 4.8 (1M context)"
description = "Best for everyday, complex tasks."

[[models]]
id = "sonnet"
display_name = "Claude Sonnet 4.6"
description = "Best for everyday tasks."

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

See `docs/MODELS.md` in the CodeTerm repository for the long-form reference
(plugin contract, how to add a new provider, etc.).
"#;

#[cfg(test)]
mod tests;

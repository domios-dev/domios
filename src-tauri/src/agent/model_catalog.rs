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

use crate::agent::provider::{AgentProvider, ModelInfo};

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
}

/// A plugin's built-in catalog as registered at startup.
#[derive(Debug, Clone)]
pub struct RegisteredDefaults {
    pub models: Vec<ModelInfo>,
    pub default: Option<String>,
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

/// Plugin entry point: register the built-in catalog for `provider_id`.
/// Idempotent — calling twice replaces the prior registration (handy for
/// hot-reloaded plugin updates and for tests). The data here is what CodeTerm
/// ships with for the current build; bump it alongside upstream releases.
pub fn register(provider_id: &str, defaults: RegisteredDefaults) {
    registry()
        .write()
        .insert(provider_id.to_string(), ProviderModels::seed(defaults));
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
    // `discover_models` may run synchronous JS under the VM lock, so run it all on the
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
}

pub fn models_dir() -> PathBuf {
    let dir = crate::paths::data_dir().join("models");
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
                Ok(p) if p.models.is_empty() && !p.replace => {
                    log::warn!(
                        "[model_catalog] {} has no `[[models]]` entries — falling back to plugin defaults",
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
    })
}

/// Merge built-in models with user overrides, by id. Built-in order is
/// preserved; each user entry either replaces an entry with the same id
/// (keeping its original position) or, if the id is new, gets appended at
/// the end. Result: built-in updates flow in automatically, user additions
/// stick around, user re-titles of built-ins win.
fn merge_models(builtin: &[ModelInfo], overrides: &[ModelInfo]) -> Vec<ModelInfo> {
    let mut merged: Vec<ModelInfo> = builtin
        .iter()
        .map(|b| {
            overrides
                .iter()
                .find(|o| o.id == b.id)
                .cloned()
                .unwrap_or_else(|| b.clone())
        })
        .collect();
    for o in overrides {
        if !merged.iter().any(|m| m.id == o.id) {
            merged.push(o.clone());
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
    let snapshot: Vec<(String, RegisteredDefaults)> = registry()
        .read()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_defaults()))
        .collect();
    for (id, defaults) in snapshot {
        let path = dir.join(format!("{}.example.toml", id));
        let body = render_example(&id, &defaults);
        if let Err(e) = std::fs::write(&path, body) {
            log::warn!("[model_catalog] could not write {}: {}", path.display(), e);
        }
    }
}

fn render_example(provider_id: &str, defaults: &RegisteredDefaults) -> String {
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
         # By default, your `[[models]]` MERGE with built-ins by id:\n\
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
# edit claude.toml — change display names, add models, set default
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

[[models]]
id = "opus"
display_name = "Claude Opus 4.8 (1M context)"
description = "Best for everyday, complex tasks."

[[models]]
id = "sonnet"
display_name = "Claude Sonnet 4.6"
description = "Best for everyday tasks."
```

- `id` MUST match what the underlying CLI accepts (`claude --model <id>`,
  `codex -m <id>`, `opencode --model <provider>/<model>`). Wrong ids cause
  launch failures.
- `display_name` is what the picker UI shows.
- `description` is optional secondary text.

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

Syntax error or empty `[[models]]`? CodeTerm logs a warning and falls back
to built-ins — the picker never goes empty.

## Full docs

See `docs/MODELS.md` in the CodeTerm repository for the long-form reference
(plugin contract, how to add a new provider, etc.).
"#;

#[cfg(test)]
mod tests {
    use super::*;

    // The registry is a process-global static — tests run in parallel against it. Use a unique provider id per test (and clean up after) so no
    // test depends on the registry being empty or seeing another test's writes.

    #[test]
    fn register_and_read_back() {
        let id = "test-register-read-back";
        register(
            id,
            RegisteredDefaults {
                models: vec![ModelInfo {
                    id: "m1".into(),
                    display_name: "Model One".into(),
                    description: None,
                    available: None,
                }],
                default: Some("m1".into()),
            },
        );
        let models = resolve_models(id);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "m1");
        assert_eq!(resolve_default(id).as_deref(), Some("m1"));
        registry().write().remove(id);
    }

    #[test]
    fn unregistered_provider_returns_empty() {
        assert!(resolve_models("ghost-never-registered").is_empty());
        assert!(resolve_default("ghost-never-registered").is_none());
    }

    #[test]
    fn merge_replaces_same_id_keeps_position_and_appends_new() {
        // Built-ins: opus, sonnet, haiku in that order. User override: re-titles sonnet, adds custom finetune. Expectation: order [opus, sonnet-renamed, haiku, my-finetune].
        let builtin = vec![
            ModelInfo {
                id: "opus".into(),
                display_name: "Opus 4.7".into(),
                description: None,
                available: None,
            },
            ModelInfo {
                id: "sonnet".into(),
                display_name: "Sonnet 4.6".into(),
                description: None,
                available: None,
            },
            ModelInfo {
                id: "haiku".into(),
                display_name: "Haiku 4.5".into(),
                description: None,
                available: None,
            },
        ];
        let overrides = vec![
            ModelInfo {
                id: "sonnet".into(),
                display_name: "My Sonnet".into(),
                description: None,
                available: None,
            },
            ModelInfo {
                id: "my-finetune".into(),
                display_name: "Mine".into(),
                description: None,
                available: None,
            },
        ];
        let merged = merge_models(&builtin, &overrides);
        assert_eq!(merged.len(), 4);
        assert_eq!(merged[0].id, "opus");
        assert_eq!(merged[1].id, "sonnet");
        assert_eq!(merged[1].display_name, "My Sonnet"); // override won
        assert_eq!(merged[2].id, "haiku");
        assert_eq!(merged[3].id, "my-finetune");
    }

    #[test]
    fn merge_with_empty_override_is_builtin_verbatim() {
        let builtin = vec![ModelInfo {
            id: "a".into(),
            display_name: "A".into(),
            description: None,
            available: None,
        }];
        let merged = merge_models(&builtin, &[]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "a");
    }

    #[test]
    fn merge_with_empty_builtin_is_override_verbatim() {
        let overrides = vec![ModelInfo {
            id: "x".into(),
            display_name: "X".into(),
            description: None,
            available: None,
        }];
        let merged = merge_models(&[], &overrides);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "x");
    }

    #[test]
    fn register_twice_replaces() {
        let id = "test-register-twice-replaces";
        register(
            id,
            RegisteredDefaults {
                models: vec![ModelInfo {
                    id: "a".into(),
                    display_name: "A".into(),
                    description: None,
                    available: None,
                }],
                default: Some("a".into()),
            },
        );
        register(
            id,
            RegisteredDefaults {
                models: vec![ModelInfo {
                    id: "b".into(),
                    display_name: "B".into(),
                    description: None,
                    available: None,
                }],
                default: Some("b".into()),
            },
        );
        let models = resolve_models(id);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "b");
        registry().write().remove(id);
    }

    #[test]
    fn put_live_overrides_seed_for_models_and_default() {
        let id = "test-put-live-overrides-seed";
        register(
            id,
            RegisteredDefaults {
                models: vec![ModelInfo {
                    id: "seed".into(),
                    display_name: "Seed".into(),
                    description: None,
                    available: None,
                }],
                default: Some("seed".into()),
            },
        );

        put_live(
            id,
            vec![ModelInfo {
                id: "live".into(),
                display_name: "Live".into(),
                description: None,
                available: None,
            }],
            Some("live".into()),
        );

        let models = resolve_models(id);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "live");
        assert_eq!(resolve_default(id).as_deref(), Some("live"));
        registry().write().remove(id);
    }

    #[test]
    fn is_stale_uses_injected_now() {
        let id = "test-is-stale-injected-now";
        register(
            id,
            RegisteredDefaults {
                models: vec![ModelInfo {
                    id: "seed".into(),
                    display_name: "Seed".into(),
                    description: None,
                    available: None,
                }],
                default: Some("seed".into()),
            },
        );

        put_live(
            id,
            vec![ModelInfo {
                id: "live".into(),
                display_name: "Live".into(),
                description: None,
                available: None,
            }],
            Some("live".into()),
        );
        let fetched_at = test_live_fetched_at(id);

        assert!(!is_stale(
            id,
            Duration::from_secs(60),
            fetched_at + Duration::from_secs(59)
        ));
        assert!(is_stale(
            id,
            Duration::from_secs(60),
            fetched_at + Duration::from_secs(60)
        ));
        registry().write().remove(id);
    }

    #[test]
    fn resolve_model_matches_exact_id() {
        let id = "test-resolve-exact-id";
        register(
            id,
            RegisteredDefaults {
                models: vec![
                    ModelInfo {
                        id: "gpt-5.5".into(),
                        display_name: "GPT-5.5".into(),
                        description: None,
                        available: None,
                    },
                    ModelInfo {
                        id: "gpt-5.5-mini".into(),
                        display_name: "GPT-5.5 Mini".into(),
                        description: None,
                        available: None,
                    },
                ],
                default: Some("gpt-5.5-mini".into()),
            },
        );

        let resolved = resolve_model(id, Some("gpt-5.5"));

        assert_eq!(resolved.id, "gpt-5.5");
        assert_eq!(resolved.display, "GPT-5.5");
        assert!(!resolved.fell_back);
        registry().write().remove(id);
    }

    #[test]
    fn resolve_model_matches_display_case_insensitively() {
        let id = "test-resolve-display-case";
        register(
            id,
            RegisteredDefaults {
                models: vec![ModelInfo {
                    id: "gpt-5.5".into(),
                    display_name: "GPT-5.5".into(),
                    description: None,
                    available: None,
                }],
                default: Some("gpt-5.5".into()),
            },
        );

        let upper = resolve_model(id, Some("GPT-5.5"));
        let lower = resolve_model(id, Some("gpt-5.5"));

        assert_eq!(upper.id, "gpt-5.5");
        assert_eq!(lower.id, "gpt-5.5");
        assert!(!upper.fell_back);
        assert!(!lower.fell_back);
        registry().write().remove(id);
    }

    #[test]
    fn resolve_model_unknown_falls_back_to_default() {
        let id = "test-resolve-unknown-default";
        register(
            id,
            RegisteredDefaults {
                models: vec![
                    ModelInfo {
                        id: "gpt-5.5".into(),
                        display_name: "GPT-5.5".into(),
                        description: None,
                        available: None,
                    },
                    ModelInfo {
                        id: "gpt-5.5-mini".into(),
                        display_name: "GPT-5.5 Mini".into(),
                        description: None,
                        available: None,
                    },
                ],
                default: Some("gpt-5.5-mini".into()),
            },
        );

        let resolved = resolve_model(id, Some("stale-model"));

        assert_eq!(resolved.id, "gpt-5.5-mini");
        assert_eq!(resolved.display, "GPT-5.5 Mini");
        assert!(resolved.fell_back);
        registry().write().remove(id);
    }

    fn test_live_fetched_at(provider_id: &str) -> SystemTime {
        registry()
            .read()
            .get(provider_id)
            .and_then(|snapshot| snapshot.fetched_at)
            .expect("live snapshot fetched_at")
    }

    fn model(id: &str, display: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            display_name: display.into(),
            description: None,
            available: None,
        }
    }

    /// Minimal provider whose `discover_models` returns a canned result, so the
    /// refresh path can be driven deterministically (Some = online, None =
    /// offline/opt-out).
    struct DiscoverStub {
        id: String,
        default: Option<String>,
        discovery: Option<Vec<ModelInfo>>,
    }

    #[async_trait::async_trait]
    impl AgentProvider for DiscoverStub {
        fn id(&self) -> &str {
            &self.id
        }
        fn display_name(&self) -> &str {
            &self.id
        }
        fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
            String::new()
        }
        fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
            crate::agent::provider::ReadinessStrategy::FixedDelay {
                duration: Duration::ZERO,
            }
        }
        fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
            crate::agent::provider::TaskDelivery::PtyInput
        }
        fn detect_from_title(&self, _: &str) -> bool {
            false
        }
        fn detect_from_output(&self, _: &str) -> bool {
            false
        }
        fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
            None
        }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            None
        }
        fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
            None
        }
        fn default_model(&self) -> Option<String> {
            self.default.clone()
        }
        async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
            self.discovery.clone()
        }
    }

    fn cleanup(id: &str) {
        registry().write().remove(id);
        let _ = std::fs::remove_file(cache_path(id));
    }

    #[test]
    fn put_live_writes_cache_that_survives_restart() {
        let id = "test-cache-survives-restart";
        let _ = std::fs::remove_file(cache_path(id));
        put_live(id, vec![model("live", "Live One")], Some("live".into()));

        // Cache file written…
        let cached = read_cache(id).expect("cache file written");
        assert_eq!(cached.models.len(), 1);
        assert_eq!(cached.models[0].id, "live");
        assert_eq!(cached.default.as_deref(), Some("live"));

        // …and survives a "restart": drop the in-memory snapshot, prime from disk.
        registry().write().remove(id);
        assert!(resolve_models(id).is_empty(), "snapshot cleared");
        prime_from_cache(id);
        let models = resolve_models(id);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "live");
        assert_eq!(resolve_default(id).as_deref(), Some("live"));
        cleanup(id);
    }

    #[test]
    fn prime_does_not_clobber_a_live_snapshot() {
        let id = "test-prime-no-clobber";
        let _ = std::fs::remove_file(cache_path(id));
        // Persist an OLD cache to disk, then publish a NEWER in-memory snapshot.
        let payload = CachedSnapshot {
            fetched_at_unix: 1,
            default: Some("old".into()),
            models: vec![model("old", "Old")],
        };
        std::fs::write(cache_path(id), serde_json::to_string(&payload).unwrap()).unwrap();
        put_live(id, vec![model("new", "New")], Some("new".into()));
        prime_from_cache(id); // must NOT replace the live snapshot with stale disk
        assert_eq!(resolve_models(id)[0].id, "new");
        cleanup(id);
    }

    #[tokio::test]
    async fn refresh_models_skips_when_fresh_refreshes_when_forced() {
        let id = "test-refresh-ttl-gate";
        cleanup(id);
        register(
            id,
            RegisteredDefaults {
                models: vec![model("seed", "Seed")],
                default: Some("seed".into()),
            },
        );
        put_live(id, vec![model("old", "Old")], Some("old".into()));

        let stub = Arc::new(DiscoverStub {
            id: id.into(),
            default: Some("new".into()),
            discovery: Some(vec![model("new", "New")]),
        });

        // Fresh snapshot → refresh skipped, list unchanged.
        assert!(refresh_models(stub.clone(), false).await.is_none());
        assert_eq!(resolve_models(id)[0].id, "old");

        // force=true → refreshes regardless of TTL.
        assert!(refresh_models(stub, true).await.is_some());
        assert_eq!(resolve_models(id)[0].id, "new");
        cleanup(id);
    }

    #[tokio::test]
    async fn refresh_live_offline_falls_back_to_seed() {
        let id = "test-refresh-offline-fallback";
        cleanup(id);
        register(
            id,
            RegisteredDefaults {
                models: vec![model("seed", "Seed")],
                default: Some("seed".into()),
            },
        );
        let stub = Arc::new(DiscoverStub {
            id: id.into(),
            default: Some("seed".into()),
            discovery: None, // offline / opt-out
        });
        assert!(refresh_live(stub, Duration::from_secs(1)).await.is_none());
        // Seed remains — list never goes empty.
        let models = resolve_models(id);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "seed");
        cleanup(id);
    }

    #[tokio::test]
    async fn refresh_live_empty_after_sanitize_keeps_seed() {
        let id = "test-refresh-empty-keeps-seed";
        cleanup(id);
        register(
            id,
            RegisteredDefaults {
                models: vec![model("seed", "Seed")],
                default: Some("seed".into()),
            },
        );
        // Discovery returns only a non-chat (embedding) model → sanitizes to empty.
        let stub = Arc::new(DiscoverStub {
            id: id.into(),
            default: None,
            discovery: Some(vec![model("text-embedding-3", "Embedding")]),
        });
        assert!(refresh_live(stub, Duration::from_secs(1)).await.is_none());
        assert_eq!(resolve_models(id)[0].id, "seed");
        cleanup(id);
    }

    #[test]
    fn live_discovery_corrects_stale_seed_label() {
        let id = "test-live-label-correction";
        cleanup(id);
        register(
            id,
            RegisteredDefaults {
                models: vec![model("opus", "Claude Opus 4.7")],
                default: Some("opus".into()),
            },
        );
        // Live discovery surfaces a fresher label for the same alias id.
        put_live(
            id,
            vec![model("opus", "Claude Opus 4.8")],
            Some("opus".into()),
        );
        assert_eq!(resolve_models(id)[0].display_name, "Claude Opus 4.8");
        cleanup(id);
    }

    #[test]
    fn availability_flag_is_carried_through_resolve() {
        let id = "test-availability-flag";
        cleanup(id);
        register(
            id,
            RegisteredDefaults {
                models: vec![
                    model("opus", "Opus"),
                    ModelInfo {
                        id: "fable".into(),
                        display_name: "Fable".into(),
                        description: None,
                        available: Some(false),
                    },
                ],
                default: Some("opus".into()),
            },
        );
        let fable = resolve_models(id)
            .into_iter()
            .find(|m| m.id == "fable")
            .expect("fable present");
        assert_eq!(fable.available, Some(false));
        cleanup(id);
    }

    #[test]
    fn sanitize_live_drops_non_chat_and_dedups() {
        let cleaned = sanitize_live(vec![
            model("gpt-chat", "Chat"),
            model("text-embedding-3-large", "Embedding"),
            model("", "Empty"),
            model("gpt-chat", "Dup"),
        ]);
        assert_eq!(cleaned.len(), 1);
        assert_eq!(cleaned[0].id, "gpt-chat");
    }

    #[test]
    fn resolve_live_default_recovers_when_default_vanished() {
        let models = vec![model("b", "B"), model("c", "C")];
        // Hardcoded default "a" gone from live → fall back to first live id.
        assert_eq!(
            resolve_live_default(&models, Some("a".into())).as_deref(),
            Some("b")
        );
        // Default still present → kept.
        assert_eq!(
            resolve_live_default(&models, Some("c".into())).as_deref(),
            Some("c")
        );
    }
}

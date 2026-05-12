//! Centralized model catalog with plugin registration + user overrides.
//!
//! ## Lookup precedence
//!   1. `<data_dir>/models/<provider>.toml` — user override, replaces the
//!      registered list entirely. Persists across upgrades.
//!   2. Plugin-registered list (set once at startup via `register`) — what
//!      ships with the current CodeTerm build. Bumped with each release.
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
use std::sync::OnceLock;

use parking_lot::RwLock;
use serde::Deserialize;

use crate::agent::provider::ModelInfo;

/// One model entry as read from the TOML file. Matches `ModelInfo` plus an
/// optional alias field, kept ergonomic so the user file feels light.
#[derive(Debug, Deserialize)]
struct CatalogModel {
    id: String,
    display_name: String,
    #[serde(default)]
    description: Option<String>,
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

fn registry() -> &'static RwLock<HashMap<String, RegisteredDefaults>> {
    static REG: OnceLock<RwLock<HashMap<String, RegisteredDefaults>>> = OnceLock::new();
    REG.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Plugin entry point: register the built-in catalog for `provider_id`.
/// Idempotent — calling twice replaces the prior registration (handy for
/// hot-reloaded plugin updates and for tests). The data here is what CodeTerm
/// ships with for the current build; bump it alongside upstream releases.
pub fn register(provider_id: &str, defaults: RegisteredDefaults) {
    registry()
        .write()
        .insert(provider_id.to_string(), defaults);
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
            Err(_) => None.into_iter().next().unwrap_or_else(|| String::new()),
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
                        })
                        .collect(),
                })),
            }
        }
    };

    // Re-stat after the read so the cached mtime reflects the file content we
    // actually parsed — not the mtime we sampled before reading.
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

/// Auto-install the README documenting the catalog format. Called once on
/// startup so the user discovers the directory naturally.
pub fn ensure_readme_installed() {
    let dir = models_dir();
    let readme = dir.join("README.md");
    if readme.exists() {
        return;
    }
    if let Err(e) = std::fs::write(&readme, README_BODY) {
        log::warn!("[model_catalog] could not write {}: {}", readme.display(), e);
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
        .map(|(k, v)| (k.clone(), v.clone()))
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

CodeTerm picks `claude.toml` up immediately (no restart) and **replaces** the
built-in list. Delete `claude.toml` (the override; NOT the `.example` file)
to fall back to built-in defaults.

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
display_name = "Claude Opus 4.7 (1M context)"
description = "Most capable for complex reasoning."

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
2. Plugin defaults shipped with the current CodeTerm build (auto-updated
   on upgrade).

Syntax error or empty `[[models]]`? CodeTerm logs a warning and falls back
to built-ins — the picker never goes empty.

## Full docs

See `docs/MODELS.md` in the CodeTerm repository for the long-form reference
(plugin contract, how to add a new provider, etc.).
"#;

#[cfg(test)]
mod tests {
    use super::*;

    // The registry is a process-global static — tests run in parallel
    // against it. Use a unique provider id per test (and clean up after)
    // so no test depends on the registry being empty or seeing another
    // test's writes.

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
        // Built-ins: opus, sonnet, haiku in that order.
        // User override: re-titles sonnet, adds custom finetune.
        // Expectation: order [opus, sonnet-renamed, haiku, my-finetune].
        let builtin = vec![
            ModelInfo { id: "opus".into(), display_name: "Opus 4.7".into(), description: None },
            ModelInfo { id: "sonnet".into(), display_name: "Sonnet 4.6".into(), description: None },
            ModelInfo { id: "haiku".into(), display_name: "Haiku 4.5".into(), description: None },
        ];
        let overrides = vec![
            ModelInfo { id: "sonnet".into(), display_name: "My Sonnet".into(), description: None },
            ModelInfo { id: "my-finetune".into(), display_name: "Mine".into(), description: None },
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
            id: "a".into(), display_name: "A".into(), description: None,
        }];
        let merged = merge_models(&builtin, &[]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].id, "a");
    }

    #[test]
    fn merge_with_empty_builtin_is_override_verbatim() {
        let overrides = vec![ModelInfo {
            id: "x".into(), display_name: "X".into(), description: None,
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
                }],
                default: Some("b".into()),
            },
        );
        let models = resolve_models(id);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "b");
        registry().write().remove(id);
    }
}

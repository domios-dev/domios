//! Discovery + load pipeline for `plugin.json` bundles.
//!
//! Two roots are scanned by default:
//!
//! * `<repo>/plugins/` — workspace plugins shipped with the source tree
//!   (only useful in dev runs).
//! * `<data-dir>/plugins/` — user-installed plugins. This is where
//!   third-party drop-ins live.
//!
//! For each `<root>/<plugin-id>/plugin.json` we parse the manifest, read
//! the bundled `dist/plugin.js`, spin up a `PluginVm`, and yield a
//! `JsProvider`. Failures on a single plugin are logged and skipped —
//! the rest of the boot continues normally.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{JsProvider, PluginManifest, PluginVm};

/// One result of scanning a root directory.
#[derive(Debug)]
pub enum LoadOutcome {
    Loaded {
        plugin_id: String,
        manifest_path: PathBuf,
    },
    Skipped {
        path: PathBuf,
        reason: String,
    },
}

/// Discover and load every plugin reachable from the supplied roots.
///
/// `roots` are scanned in order; if two roots contain the same plugin
/// `id`, the LATER occurrence wins (intentional — user plugins override
/// workspace defaults of the same name).
pub fn scan_roots(roots: &[PathBuf]) -> (Vec<JsProvider>, Vec<LoadOutcome>) {
    let mut providers: Vec<JsProvider> = Vec::new();
    let mut report: Vec<LoadOutcome> = Vec::new();
    for root in roots {
        scan_one(root, &mut providers, &mut report);
    }
    (providers, report)
}

fn scan_one(root: &Path, providers: &mut Vec<JsProvider>, report: &mut Vec<LoadOutcome>) {
    if !root.exists() {
        return;
    }
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(e) => {
            report.push(LoadOutcome::Skipped {
                path: root.to_path_buf(),
                reason: format!("read_dir: {e}"),
            });
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let manifest_path = path.join("plugin.json");
        if !manifest_path.exists() {
            continue;
        }
        match load_one(&manifest_path) {
            Ok(p) => {
                // Overwrite earlier registration of the same id.
                let id = p.manifest().id.clone();
                providers.retain(|existing| existing.manifest().id != id);
                providers.push(p);
                report.push(LoadOutcome::Loaded {
                    plugin_id: id,
                    manifest_path,
                });
            }
            Err(reason) => {
                report.push(LoadOutcome::Skipped {
                    path: manifest_path,
                    reason,
                });
            }
        }
    }
}

/// Parse a `plugin.json`, load the bundled JS, return a ready provider.
pub fn load_one(manifest_path: &Path) -> Result<JsProvider, String> {
    load_one_with_installation(manifest_path, crate::bundled::installed())
}

pub(crate) fn load_one_with_installation(
    manifest_path: &Path,
    installation: Result<&crate::bundled::BundledPlugins, String>,
) -> Result<JsProvider, String> {
    installation?;
    let manifest = PluginManifest::load(manifest_path)?;
    let entry = manifest
        .entry_path()
        .ok_or_else(|| "manifest source_path missing".to_string())?;
    if !entry.exists() {
        return Err(format!(
            "entry script {} does not exist (build the plugin first)",
            entry.display()
        ));
    }
    let manifest_json = serde_json::to_string(&manifest)
        .map_err(|e| format!("serialize manifest for {}: {e}", manifest.id))?;

    // is treated as third-party in dev so live edits don't trip the verifier.
    let bundle_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest has no parent dir".to_string())?;
    let trust = compute_trust(&manifest.id, bundle_dir);

    if let Err(reason) = validate_conformance(&manifest, &trust) {
        log::warn!("[plugin/{}] {reason}", manifest.id);
        return Err(reason);
    }

    let sandbox = Arc::new(compute_sandbox(&trust, bundle_dir, &manifest));
    let vm = PluginVm::load_with_manifest_sandboxed(&manifest.id, &entry, manifest_json, sandbox)
        .map_err(|e| format!("VM init for {}: {e}", manifest.id))?;

    Ok(JsProvider::new_with_trust(manifest, Arc::new(vm), trust))
}

/// FS/env policy for a plugin. Bundled/in-workspace plugins (anything NOT under
/// the user plugins dir `~/.codeterm/plugins/`) and verified `System` plugins
/// get full host access. A user-dropped plugin is confined to its own dir + the
/// session root it declares; `envGet` of secret-named vars is blocked. (Trust
/// alone can't gate this: `CODETERM_DEV=1` forces every plugin to `ThirdParty`,
/// so the dev-edited bundled plugins must stay unrestricted — hence the
/// location check.)
///
/// A provider only gets the roots it DECLARES: a confined plugin whose manifest
/// names no `sessions.rootDir` cannot reach its own provider home, which is how
/// the codex usage panel went quiet for a day. Declared credential files are
/// carved back out of those roots — the host reads them instead.
fn compute_sandbox(
    trust: &crate::provider::ProviderTrust,
    bundle_dir: &Path,
    manifest: &PluginManifest,
) -> crate::js_runtime::host_api::PluginSandbox {
    use crate::js_runtime::host_api::PluginSandbox;
    use crate::provider::ProviderTrust;

    let dev = std::env::var("CODETERM_DEV").as_deref() == Ok("1");

    let base = if manifest.permissions.project_files.is_empty()
        && (dev || matches!(trust, ProviderTrust::System) || !is_user_dropin(bundle_dir))
    {
        PluginSandbox::unrestricted()
    } else {
        let _ = std::fs::create_dir_all(plugin_temp_dir(&manifest.id));
        let (read_roots, write_roots) = confined_roots(
            manifest,
            bundle_dir,
            &codeterm_paths::data_dir().join(&manifest.id),
        );
        PluginSandbox::restricted(read_roots, write_roots)
    };
    let base = base.with_credentials(&manifest.credentials);

    let settings_json =
        crate::js_runtime::plugin_config::resolve_settings_json(bundle_dir, &manifest.id);

    let mut perms = manifest.permissions.clone();
    crate::js_runtime::grants::apply_persisted(&mut perms, bundle_dir);
    let roots = serde_json::from_str::<serde_json::Value>(&settings_json)
        .ok()
        .and_then(|settings| {
            codeterm_util::project_roots::configured(&settings, &manifest.permissions.project_files)
                .ok()
        })
        .unwrap_or_default();
    let grants = crate::js_runtime::grants::load_grants(bundle_dir);
    let approved = if grants.permissions.get("project_write") == Some(&true) {
        grants
            .project_write_roots
            .iter()
            .filter_map(|root| codeterm_util::project_roots::canonical_root(root).ok())
            .collect()
    } else {
        Vec::new()
    };

    // onto every path, including unrestricted/dev.
    base.with_identity_and_perms(&manifest.id, &perms)
        .with_receiver_status_query(manifest.capabilities.receiver_status_query.clone())
        .with_project_roots(
            roots,
            approved,
            crate::js_runtime::grants::grants_path(bundle_dir),
        )
        .with_token_mode(manifest.capabilities.decision_model.token_mode)
        .with_settings(settings_json)
}

/// A bundle under the user plugins dir `~/.codeterm/plugins/`, as opposed to a
/// bundled or in-workspace one.
fn is_user_dropin(bundle_dir: &Path) -> bool {
    let user_plugins = codeterm_paths::data_dir().join("plugins");
    std::fs::canonicalize(bundle_dir)
        .ok()
        .zip(std::fs::canonicalize(&user_plugins).ok())
        .map(|(b, u)| b.starts_with(&u))
        .unwrap_or_else(|| bundle_dir.starts_with(&user_plugins))
}

/// The per-plugin scratch directory under the system temp root. Confined
/// plugins are granted this instead of the bare `std::env::temp_dir()`, which
/// on Unix is world-writable — a blanket grant there let a confined plugin
/// read/write every other process's (and every other plugin's) scratch files.
/// Pure path arithmetic; the caller creates the directory.
pub(crate) fn plugin_temp_dir(plugin_id: &str) -> PathBuf {
    std::env::temp_dir()
        .join("codeterm-plugins")
        .join(plugin_id)
}

/// The filesystem a confined plugin gets: its own bundle, its own data dir,
/// its own scoped temp subdir — and the provider home it DECLARES. A provider
/// that declares nothing cannot reach its own files, which is not a safe
/// default but a silent one. Pure so the declared-root grant is provable
/// without a real install layout.
pub(crate) fn confined_roots(
    manifest: &PluginManifest,
    bundle_dir: &Path,
    data_dir: &Path,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let scoped_temp = plugin_temp_dir(&manifest.id);
    let mut read_roots = vec![
        bundle_dir.to_path_buf(),
        data_dir.to_path_buf(),
        scoped_temp.clone(),
    ];
    if let Some(sessions) = manifest.sessions.as_ref() {
        if let Some(root) = PluginManifest::expand_home(&sessions.root_dir) {
            read_roots.push(root);
        }
    }
    let write_roots = vec![
        bundle_dir.to_path_buf(),
        data_dir.to_path_buf(),
        scoped_temp,
    ];
    (read_roots, write_roots)
}

/// Gate a third-party drop-in against the host contract before it loads.
///
/// First-party ids (anything in the system-plugin table — including dev and
/// hash-mismatched copies, which surface as `System`/`Unverified`) ship with
/// the build and move in lockstep with the host, so they're exempt. A genuine
/// `ThirdParty` plugin must declare a `hostApi` this build understands; if it's
/// missing or unsupported the plugin is skipped loudly rather than registered
/// against an interface it doesn't conform to (#16). Pure + env-free so the
/// decision is deterministic and unit-testable.
fn validate_conformance(
    manifest: &PluginManifest,
    trust: &crate::provider::ProviderTrust,
) -> Result<(), String> {
    use crate::js_runtime::host_api::HOST_API_VERSION;
    use crate::provider::ProviderTrust;

    if !matches!(trust, ProviderTrust::ThirdParty) {
        return Ok(());
    }
    if crate::system_plugins::lookup(&manifest.id).is_some() {
        return Ok(());
    }
    match manifest.host_api {
        Some(v) if v == HOST_API_VERSION => Ok(()),
        got => Err(format!(
            "incompatible (trust=ThirdParty): hostApi {got:?} unsupported \
             — this build requires hostApi {HOST_API_VERSION} — skipping load"
        )),
    }
}

fn compute_trust(id: &str, bundle_dir: &Path) -> crate::provider::ProviderTrust {
    use crate::provider::ProviderTrust;
    use crate::{bundle_hash, system_plugins};

    if std::env::var("CODETERM_DEV").as_deref() == Ok("1") {
        return ProviderTrust::ThirdParty;
    }
    let Some(expected) = system_plugins::lookup(id) else {
        return ProviderTrust::ThirdParty;
    };
    match bundle_hash::hash_bundle(bundle_dir) {
        Ok(actual) if actual == expected => ProviderTrust::System,
        Ok(actual) => {
            if is_official_system_channel_install(id, bundle_dir) {
                log::info!(
                    "[plugin/{id}] hash differs from embedded table but install record is official system channel — marking system"
                );
                return ProviderTrust::System;
            }
            log::warn!(
                "[plugin/{id}] hash mismatch: expected {expected:?}, got {actual:?} — marking unverified"
            );
            ProviderTrust::Unverified
        }
        Err(e) => {
            log::warn!("[plugin/{id}] hash computation failed: {e} — marking unverified");
            ProviderTrust::Unverified
        }
    }
}

fn is_official_system_channel_install(id: &str, bundle_dir: &Path) -> bool {
    const SYSTEM_CHANNEL_NAME: &str = "codeterm-core";
    let Ok(text) = std::fs::read_to_string(bundle_dir.join(".codeterm-install.json")) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    official_system_channel_record(id, &value, SYSTEM_CHANNEL_NAME)
}

fn official_system_channel_record(id: &str, record: &serde_json::Value, channel: &str) -> bool {
    if record.get("id").and_then(|v| v.as_str()) != Some(id) {
        return false;
    }
    let Some(source) = record.get("source") else {
        return false;
    };
    let path = source.get("path").and_then(|v| v.as_str());
    // Core non-provider plugins sit at the channel root under their bare id.
    let path_matches = path == Some(format!("provider-{id}").as_str())
        || (path == Some(id) && crate::system_plugins::is_system_id(id));
    source.get("kind").and_then(|v| v.as_str()) == Some("system_channel")
        && source.get("channel").and_then(|v| v.as_str()) == Some(channel)
        && path_matches
}

/// Core plugins speak to every agent (prompt line, `codeterm docs` guide). Dev runs
/// force ThirdParty trust, so there a bundled system id outside the user plugins dir counts.
pub fn is_core_plugin(manifest: &PluginManifest, trust: crate::provider::ProviderTrust) -> bool {
    let dev = std::env::var("CODETERM_DEV").as_deref() == Ok("1");
    core_eligibility(
        trust,
        dev,
        crate::system_plugins::is_system_id(&manifest.id),
        || {
            manifest
                .source_path
                .as_deref()
                .and_then(Path::parent)
                .map_or(true, is_user_dropin)
        },
    )
}

fn core_eligibility(
    trust: crate::provider::ProviderTrust,
    dev: bool,
    system_id: bool,
    user_dropin: impl FnOnce() -> bool,
) -> bool {
    trust == crate::provider::ProviderTrust::System || (dev && system_id && !user_dropin())
}

pub fn default_roots(workspace_root: Option<&Path>) -> Vec<PathBuf> {
    let dev = std::env::var("CODETERM_DEV").as_deref() == Ok("1");
    let workspace = selected_workspace_root(
        workspace_root,
        std::env::var_os("CODETERM_DEV_WORKSPACE_ROOT").map(PathBuf::from),
        dev,
    );
    ordered_roots(
        workspace.as_deref(),
        codeterm_paths::data_dir().join("plugins"),
        dev,
    )
}

fn selected_workspace_root(
    compiled: Option<&Path>,
    explicit: Option<PathBuf>,
    dev: bool,
) -> Option<PathBuf> {
    explicit
        .filter(|_| dev)
        .or_else(|| compiled.map(Path::to_path_buf))
}

fn ordered_roots(
    workspace_root: Option<&Path>,
    user_plugins: PathBuf,
    workspace_authoritative: bool,
) -> Vec<PathBuf> {
    let workspace_plugins = workspace_root.map(|ws| ws.join("plugins"));
    let mut out = Vec::new();
    if workspace_authoritative {
        out.push(user_plugins);
        if let Some(path) = workspace_plugins {
            out.push(path);
        }
    } else {
        out.push(user_plugins);
    }
    out
}

#[cfg(test)]
mod tests;

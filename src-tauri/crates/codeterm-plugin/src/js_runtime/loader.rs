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
fn compute_sandbox(
    trust: &crate::provider::ProviderTrust,
    bundle_dir: &Path,
    manifest: &PluginManifest,
) -> crate::js_runtime::host_api::PluginSandbox {
    use crate::js_runtime::host_api::PluginSandbox;
    use crate::provider::ProviderTrust;

    let dev = std::env::var("CODETERM_DEV").as_deref() == Ok("1");
    let user_plugins = codeterm_paths::data_dir().join("plugins");
    let is_user_dropin = std::fs::canonicalize(bundle_dir)
        .ok()
        .zip(std::fs::canonicalize(&user_plugins).ok())
        .map(|(b, u)| b.starts_with(&u))
        .unwrap_or_else(|| bundle_dir.starts_with(&user_plugins));

    let base = if dev || matches!(trust, ProviderTrust::System) || !is_user_dropin {
        PluginSandbox::unrestricted()
    } else {
        let data_dir = codeterm_paths::data_dir().join(&manifest.id);
        let mut read_roots = vec![
            bundle_dir.to_path_buf(),
            data_dir.clone(),
            std::env::temp_dir(),
        ];
        if let Some(sessions) = manifest.sessions.as_ref() {
            if let Some(root) = PluginManifest::expand_home(&sessions.root_dir) {
                read_roots.push(root);
            }
        }
        let write_roots = vec![bundle_dir.to_path_buf(), data_dir, std::env::temp_dir()];
        PluginSandbox::restricted(read_roots, write_roots)
    };

    let settings_json =
        crate::js_runtime::plugin_config::resolve_settings_json(bundle_dir, &manifest.id);

    let mut perms = manifest.permissions.clone();
    crate::js_runtime::grants::apply_persisted(&mut perms, bundle_dir);

    // onto every path, including unrestricted/dev.
    base.with_identity_and_perms(&manifest.id, &perms)
        .with_settings(settings_json)
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
    if value.get("id").and_then(|v| v.as_str()) != Some(id) {
        return false;
    }
    let Some(source) = value.get("source") else {
        return false;
    };
    let expected_path = format!("provider-{id}");
    source.get("kind").and_then(|v| v.as_str()) == Some("system_channel")
        && source.get("channel").and_then(|v| v.as_str()) == Some(SYSTEM_CHANNEL_NAME)
        && source.get("path").and_then(|v| v.as_str()) == Some(expected_path.as_str())
}

pub fn default_roots(workspace_root: Option<&Path>) -> Vec<PathBuf> {
    ordered_roots(
        workspace_root,
        codeterm_paths::data_dir().join("plugins"),
        std::env::var("CODETERM_DEV").as_deref() == Ok("1"),
    )
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
mod tests {
    use super::*;
    use crate::provider::AgentProvider;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    fn min_manifest(id: &str) -> String {
        use crate::js_runtime::host_api::HOST_API_VERSION;
        format!(
            r#"{{
                "id": "{id}",
                "version": "0.1.0",
                "displayName": "{id} Test",
                "hostApi": {HOST_API_VERSION},
                "spawn": {{
                    "readinessStrategy": {{ "kind": "fixed_delay", "durationMs": 0 }},
                    "taskDelivery": "pty_input",
                    "verifySubmit": {{ "kind": "none" }}
                }},
                "entry": "plugin.js"
            }}"#
        )
    }

    fn cap_manifest(id: &str, version: &str, host_api: u32) -> String {
        // Capability-only plugin (transcriber): no spawn block, declares its capability.
        format!(
            r#"{{
                "id": "{id}",
                "version": "{version}",
                "displayName": "{id} Test",
                "hostApi": {host_api},
                "entry": "plugin.js",
                "capabilities": {{ "transcriber": true }}
            }}"#
        )
    }

    /// Validates the install→update→work→compat-gate lifecycle for a capability
    /// plugin: a new version replaces the old, the capability still resolves and
    /// runs the new behaviour, and an incompatible version is gated (daemon
    /// survives). This is the loader-level core of the plugin-update flow.
    #[tokio::test]
    async fn capability_plugin_version_update_and_compat_gate() {
        use crate::js_runtime::host_api::HOST_API_VERSION;
        use crate::js_runtime::JsTranscriber;
        use crate::transcribe::Transcriber;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let dir = root.join("transcriber");

        write(
            &dir.join("plugin.json"),
            &cap_manifest("transcriber", "0.1.0", HOST_API_VERSION),
        );
        write(
            &dir.join("plugin.js"),
            r#"module.exports.default = { transcribe() { return { text: "v0.1.0" }; } }"#,
        );
        let (providers, _) = scan_roots(&[root.clone()]);
        let p = providers
            .iter()
            .find(|p| p.manifest().id == "transcriber")
            .expect("v0.1.0 loaded");
        assert_eq!(p.manifest().version, "0.1.0");
        assert!(p.manifest().capabilities.transcriber);
        let t = JsTranscriber::new(p.vm().clone(), "transcriber");
        assert_eq!(
            t.transcribe(Path::new("/a"), "").await.unwrap().text,
            "v0.1.0"
        );

        write(
            &dir.join("plugin.json"),
            &cap_manifest("transcriber", "0.1.1", HOST_API_VERSION),
        );
        write(
            &dir.join("plugin.js"),
            r#"module.exports.default = { transcribe() { return { text: "v0.1.1-updated" }; } }"#,
        );
        let (providers2, _) = scan_roots(&[root.clone()]);
        let p2 = providers2
            .iter()
            .find(|p| p.manifest().id == "transcriber")
            .expect("v0.1.1 loaded");
        assert_eq!(p2.manifest().version, "0.1.1", "version bumped");
        assert!(
            p2.manifest().capabilities.transcriber,
            "capability survives update"
        );
        let t2 = JsTranscriber::new(p2.vm().clone(), "transcriber");
        assert_eq!(
            t2.transcribe(Path::new("/a"), "").await.unwrap().text,
            "v0.1.1-updated",
            "the updated version's behaviour is active"
        );

        let bad = root.join("badplug");
        write(
            &bad.join("plugin.json"),
            &cap_manifest("badplug", "1.0.0", 999),
        );
        write(&bad.join("plugin.js"), "module.exports.default = {};");
        let (providers3, report3) = scan_roots(&[root.clone()]);
        assert!(
            !providers3.iter().any(|p| p.manifest().id == "badplug"),
            "incompatible plugin must not load"
        );
        assert!(
            report3
                .iter()
                .any(|o| matches!(o, LoadOutcome::Skipped { .. })),
            "incompatible load is recorded as Skipped"
        );
    }

    fn legacy_manifest_without_host_api(id: &str) -> String {
        format!(
            r#"{{
                "id": "{id}",
                "version": "0.1.0",
                "displayName": "{id} Test",
                "spawn": {{
                    "readinessStrategy": {{ "kind": "fixed_delay", "durationMs": 0 }},
                    "taskDelivery": "pty_input",
                    "verifySubmit": {{ "kind": "none" }}
                }},
                "entry": "plugin.js"
            }}"#
        )
    }

    fn min_plugin_js(id: &str) -> String {
        format!(
            r#"
            module.exports.default = {{
                detectFromTitle(t) {{ return t.includes('{id}'); }}
            }};
            "#
        )
    }

    fn manifest_with_host_api(id: &str, host_api: u32) -> String {
        format!(
            r#"{{
                "id": "{id}",
                "version": "0.1.0",
                "displayName": "{id} Test",
                "hostApi": {host_api},
                "spawn": {{
                    "readinessStrategy": {{ "kind": "fixed_delay", "durationMs": 0 }},
                    "taskDelivery": "pty_input",
                    "verifySubmit": {{ "kind": "none" }}
                }},
                "entry": "plugin.js"
            }}"#
        )
    }

    fn system_channel_install_record(id: &str) -> String {
        format!(
            r#"{{
                "id": "{id}",
                "version": "999.0.0",
                "source": {{
                    "kind": "system_channel",
                    "channel": "codeterm-core",
                    "path": "provider-{id}"
                }},
                "installed_at": "2026-06-15T00:00:00Z"
            }}"#
        )
    }

    /// #16 Layer B: a genuine third-party drop-in with no declared `hostApi`
    /// is skipped, not loaded — the daemon must boot past an incompatible
    /// custom plugin instead of registering it against a contract it doesn't meet.
    #[test]
    fn validate_conformance_rejects_thirdparty_without_host_api() {
        use crate::provider::ProviderTrust;
        let m: PluginManifest =
            serde_json::from_str(&legacy_manifest_without_host_api("totally-unknown-xyz")).unwrap();
        let err = validate_conformance(&m, &ProviderTrust::ThirdParty)
            .expect_err("missing hostApi must be rejected");
        assert!(
            err.contains("incompatible (trust=ThirdParty)"),
            "got: {err}"
        );
        assert!(err.contains("skipping load"), "got: {err}");
    }

    #[test]
    fn validate_conformance_accepts_thirdparty_with_matching_host_api() {
        use crate::js_runtime::host_api::HOST_API_VERSION;
        use crate::provider::ProviderTrust;
        let json = manifest_with_host_api("totally-unknown-xyz2", HOST_API_VERSION);
        let m: PluginManifest = serde_json::from_str(&json).unwrap();
        assert!(validate_conformance(&m, &ProviderTrust::ThirdParty).is_ok());
    }

    #[test]
    fn validate_conformance_rejects_thirdparty_with_wrong_host_api() {
        use crate::js_runtime::host_api::HOST_API_VERSION;
        use crate::provider::ProviderTrust;
        let json = manifest_with_host_api("totally-unknown-xyz3", HOST_API_VERSION + 99);
        let m: PluginManifest = serde_json::from_str(&json).unwrap();
        assert!(validate_conformance(&m, &ProviderTrust::ThirdParty).is_err());
    }

    /// First-party trust levels are never gated — they ship with the build.
    #[test]
    fn validate_conformance_exempts_first_party_trust() {
        use crate::provider::ProviderTrust;
        let m: PluginManifest = serde_json::from_str(&min_manifest("totally-unknown-xyz")).unwrap();
        assert!(validate_conformance(&m, &ProviderTrust::System).is_ok());
        assert!(validate_conformance(&m, &ProviderTrust::Unverified).is_ok());
        assert!(validate_conformance(&m, &ProviderTrust::Unknown).is_ok());
    }

    #[test]
    fn scan_loads_one_well_formed_plugin() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("demo");
        write(&dir.join("plugin.json"), &min_manifest("demo"));
        write(&dir.join("plugin.js"), &min_plugin_js("demo"));

        let (providers, report) = scan_roots(&[root.path().to_path_buf()]);
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id(), "demo");
        assert!(providers[0].detect_from_title("demo cli"));
        assert!(matches!(report[0], LoadOutcome::Loaded { .. }));
    }

    #[test]
    fn scan_skips_dir_without_manifest() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("not-a-plugin")).unwrap();
        let (providers, report) = scan_roots(&[root.path().to_path_buf()]);
        assert!(providers.is_empty());
        assert!(report.is_empty(), "no manifest = no report entry");
    }

    #[test]
    fn scan_reports_bad_manifest_as_skipped() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("broken");
        write(&dir.join("plugin.json"), "{ this is not json }");
        write(&dir.join("plugin.js"), "module.exports.default = {};");
        let (providers, report) = scan_roots(&[root.path().to_path_buf()]);
        assert!(providers.is_empty());
        assert!(matches!(report[0], LoadOutcome::Skipped { .. }));
    }

    #[test]
    fn scan_reports_missing_entry_as_skipped() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("noscript");
        write(&dir.join("plugin.json"), &min_manifest("noscript"));
        let (providers, report) = scan_roots(&[root.path().to_path_buf()]);
        assert!(providers.is_empty());
        match &report[0] {
            LoadOutcome::Skipped { reason, .. } => {
                assert!(reason.contains("does not exist"), "got: {reason}");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn later_root_overrides_earlier_with_same_id() {
        let r1 = tempfile::tempdir().unwrap();
        let r2 = tempfile::tempdir().unwrap();
        // Both roots contain a "shared" plugin — only the second should win.
        let d1 = r1.path().join("shared");
        let d2 = r2.path().join("shared");
        write(&d1.join("plugin.json"), &min_manifest("shared"));
        write(&d1.join("plugin.js"), &min_plugin_js("v1"));
        write(&d2.join("plugin.json"), &min_manifest("shared"));
        write(&d2.join("plugin.js"), &min_plugin_js("v2"));

        let (providers, _) = scan_roots(&[r1.path().to_path_buf(), r2.path().to_path_buf()]);
        assert_eq!(providers.len(), 1);
        assert!(
            providers[0].detect_from_title("v2 something"),
            "later root must win"
        );
        assert!(
            !providers[0].detect_from_title("v1 something"),
            "earlier root must be evicted"
        );
    }

    #[test]
    fn scan_continues_past_a_broken_plugin() {
        let root = tempfile::tempdir().unwrap();
        let good = root.path().join("good");
        let bad = root.path().join("bad");
        write(&good.join("plugin.json"), &min_manifest("good"));
        write(&good.join("plugin.js"), &min_plugin_js("good"));
        write(&bad.join("plugin.json"), "{ broken");
        let (providers, report) = scan_roots(&[root.path().to_path_buf()]);
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id(), "good");
        assert!(report
            .iter()
            .any(|r| matches!(r, LoadOutcome::Skipped { .. })));
    }

    #[test]
    fn missing_root_returns_empty_no_error() {
        let (providers, report) = scan_roots(&[PathBuf::from(
            "/definitely/does/not/exist/codeterm-plugins-xyz",
        )]);
        assert!(providers.is_empty());
        assert!(report.is_empty());
    }

    #[test]
    fn default_roots_includes_user_dir() {
        let roots = default_roots(Some(Path::new("/repo")));
        assert!(
            roots.iter().any(|p| p.ends_with("plugins")),
            "workspace root"
        );
        assert!(roots
            .iter()
            .any(|p| p == &codeterm_paths::data_dir().join("plugins")));
    }

    #[test]
    fn dev_root_order_keeps_workspace_plugins_authoritative() {
        let user = PathBuf::from("/data/plugins");
        let roots = ordered_roots(Some(Path::new("/repo")), user.clone(), true);
        assert_eq!(roots, vec![user, PathBuf::from("/repo/plugins")]);
    }

    #[test]
    fn production_root_order_excludes_workspace_plugins() {
        let user = PathBuf::from("/data/plugins");
        let roots = ordered_roots(Some(Path::new("/repo")), user.clone(), false);
        assert_eq!(roots, vec![user]);
    }

    #[test]
    fn root_order_without_workspace_root_uses_user_plugins() {
        let user = PathBuf::from("/data/plugins");

        assert_eq!(ordered_roots(None, user.clone(), true), vec![user.clone()]);
        assert_eq!(ordered_roots(None, user.clone(), false), vec![user]);
    }

    #[test]
    fn load_one_explicit_path_works() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("direct");
        write(&dir.join("plugin.json"), &min_manifest("direct"));
        write(&dir.join("plugin.js"), &min_plugin_js("direct"));
        let p = load_one(&dir.join("plugin.json")).unwrap();
        assert_eq!(p.id(), "direct");
    }

    /// Trust matrix: unknown id → ThirdParty, tampered system id →
    /// Unverified, CODETERM_DEV=1 → always ThirdParty.
    ///
    /// One sequential test because cargo runs `#[test]`s in parallel
    /// and CODETERM_DEV is process-global — running these as
    /// separate tests gives flaky cross-test interference.
    #[test]
    fn trust_matrix() {
        use crate::provider::ProviderTrust;

        let prev_dev = std::env::var("CODETERM_DEV").ok();

        // Case 1: unknown id, no dev mode → ThirdParty.
        unsafe { std::env::remove_var("CODETERM_DEV") };
        {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("nobody-knows-this");
            write(&dir.join("plugin.json"), &min_manifest("nobody-knows-this"));
            write(&dir.join("plugin.js"), &min_plugin_js("x"));
            let p = load_one(&dir.join("plugin.json")).unwrap();
            assert_eq!(p.trust(), ProviderTrust::ThirdParty, "case 1");
        }

        // Case 2: system id with custom content → Unverified. Skipped if the table is empty (cargo-publish edge case).
        if let Some((sys_id, _)) = crate::system_plugins::SYSTEM_PLUGIN_HASHES.first() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(*sys_id);
            write(&dir.join("plugin.json"), &min_manifest(sys_id));
            write(
                &dir.join("plugin.js"),
                "module.exports.default = { tampered: true };",
            );
            let p = load_one(&dir.join("plugin.json")).unwrap();
            assert_eq!(p.trust(), ProviderTrust::Unverified, "case 2");
        }

        // Case 3: system id with custom content from the official system channel stays trusted.
        if let Some((sys_id, _)) = crate::system_plugins::SYSTEM_PLUGIN_HASHES.first() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(*sys_id);
            write(&dir.join("plugin.json"), &min_manifest(sys_id));
            write(
                &dir.join("plugin.js"),
                "module.exports.default = { channel_update: true };",
            );
            write(
                &dir.join(".codeterm-install.json"),
                &system_channel_install_record(sys_id),
            );
            let p = load_one(&dir.join("plugin.json")).unwrap();
            assert_eq!(p.trust(), ProviderTrust::System, "case 3");
        }

        // Case 4: CODETERM_DEV=1 forces ThirdParty even for system id.
        unsafe { std::env::set_var("CODETERM_DEV", "1") };
        if let Some((sys_id, _)) = crate::system_plugins::SYSTEM_PLUGIN_HASHES.first() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(*sys_id);
            write(&dir.join("plugin.json"), &min_manifest(sys_id));
            write(&dir.join("plugin.js"), "module.exports.default = {};");
            let p = load_one(&dir.join("plugin.json")).unwrap();
            assert_eq!(p.trust(), ProviderTrust::ThirdParty, "case 4");
        }

        unsafe {
            match prev_dev {
                Some(v) => std::env::set_var("CODETERM_DEV", v),
                None => std::env::remove_var("CODETERM_DEV"),
            }
        }
    }
}

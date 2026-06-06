//! Discovery + load pipeline for `plugin.json` bundles.
//!
//! Two roots are scanned by default:
//!
//! * `<repo>/plugins/` — workspace plugins shipped with the source tree
//!   (only useful in dev runs).
//! * `~/.codeterm/plugins/` — user-installed plugins. This is where
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
    let vm = PluginVm::load_with_manifest(&manifest.id, &entry, manifest_json)
        .map_err(|e| format!("VM init for {}: {e}", manifest.id))?;

    // Trust check: compute the bundle hash and compare against the
    // build-time SYSTEM_PLUGIN_HASHES table. CODETERM_DEV=1 bypasses
    // — every plugin is treated as third-party in dev so live edits
    // don't trip the verifier.
    let bundle_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest has no parent dir".to_string())?;
    let trust = compute_trust(&manifest.id, bundle_dir);

    Ok(JsProvider::new_with_trust(manifest, Arc::new(vm), trust))
}

fn compute_trust(id: &str, bundle_dir: &Path) -> crate::agent::provider::ProviderTrust {
    use crate::agent::provider::ProviderTrust;
    use crate::plugin::{bundle_hash, system_plugins};

    if std::env::var("CODETERM_DEV").as_deref() == Ok("1") {
        return ProviderTrust::ThirdParty;
    }
    let Some(expected) = system_plugins::lookup(id) else {
        return ProviderTrust::ThirdParty;
    };
    match bundle_hash::hash_bundle(bundle_dir) {
        Ok(actual) if actual == expected => ProviderTrust::System,
        Ok(actual) => {
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

/// Standard discovery roots for a running CodeTerm instance. Order
/// matters — later roots override earlier ones, so user plugins shadow
/// workspace defaults.
pub fn default_roots(workspace_root: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(ws) = workspace_root {
        out.push(ws.join("plugins"));
    }
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".codeterm").join("plugins"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::provider::AgentProvider;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }

    fn min_manifest(id: &str) -> String {
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
        // intentionally no plugin.js
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
        if let Some(home) = dirs::home_dir() {
            assert!(roots
                .iter()
                .any(|p| p == &home.join(".codeterm").join("plugins")));
        }
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
        use crate::agent::provider::ProviderTrust;

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

        // Case 2: system id with custom content → Unverified. Skipped
        // if the table is empty (cargo-publish edge case).
        if let Some((sys_id, _)) = crate::plugin::system_plugins::SYSTEM_PLUGIN_HASHES.first() {
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

        // Case 3: CODETERM_DEV=1 forces ThirdParty even for system id.
        unsafe { std::env::set_var("CODETERM_DEV", "1") };
        if let Some((sys_id, _)) = crate::plugin::system_plugins::SYSTEM_PLUGIN_HASHES.first() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(*sys_id);
            write(&dir.join("plugin.json"), &min_manifest(sys_id));
            write(&dir.join("plugin.js"), "module.exports.default = {};");
            let p = load_one(&dir.join("plugin.json")).unwrap();
            assert_eq!(p.trust(), ProviderTrust::ThirdParty, "case 3");
        }

        unsafe {
            match prev_dev {
                Some(v) => std::env::set_var("CODETERM_DEV", v),
                None => std::env::remove_var("CODETERM_DEV"),
            }
        }
    }
}

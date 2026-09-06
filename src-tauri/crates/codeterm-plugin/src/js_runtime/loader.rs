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
    let user_plugins = codeterm_paths::data_dir().join("plugins");
    let is_user_dropin = std::fs::canonicalize(bundle_dir)
        .ok()
        .zip(std::fs::canonicalize(&user_plugins).ok())
        .map(|(b, u)| b.starts_with(&u))
        .unwrap_or_else(|| bundle_dir.starts_with(&user_plugins));

    let base = if dev || matches!(trust, ProviderTrust::System) || !is_user_dropin {
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

    // onto every path, including unrestricted/dev.
    base.with_identity_and_perms(&manifest.id, &perms)
        .with_settings(settings_json)
}

/// The per-plugin scratch directory under the system temp root. Confined
/// plugins are granted this instead of the bare `std::env::temp_dir()`, which
/// on Unix is world-writable — a blanket grant there let a confined plugin
/// read/write every other process's (and every other plugin's) scratch files.
/// Pure path arithmetic; the caller creates the directory.
pub(crate) fn plugin_temp_dir(plugin_id: &str) -> PathBuf {
    std::env::temp_dir().join("codeterm-plugins").join(plugin_id)
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
    let write_roots = vec![bundle_dir.to_path_buf(), data_dir.to_path_buf(), scoped_temp];
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

    /// The real `plugins/provider-codex/plugin.json`, so this test fails if the
    /// shipped manifest ever loses the declaration that keeps a confined codex
    /// plugin able to reach its own home.
    fn shipped_codex_manifest() -> PluginManifest {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../plugins/provider-codex/plugin.json");
        PluginManifest::load(&path).expect("the shipped codex manifest parses")
    }

    /// The defect this whole change exists for: a confined codex plugin whose
    /// manifest declared no root could not read `~/.codex`, its usage fetch
    /// returned nothing, and the panel froze. The shipped manifest must keep
    /// declaring that home, and must keep the credential inside it declared so
    /// the host — not the plugin — is what reads the OAuth token.
    #[test]
    fn the_shipped_codex_manifest_declares_the_home_and_credential_it_needs() {
        let manifest = shipped_codex_manifest();
        let sessions = manifest
            .sessions
            .as_ref()
            .expect("codex must declare the home its provider logic needs");
        assert_eq!(sessions.root_dir, "~/.codex");
        let credential = manifest
            .credentials
            .iter()
            .find(|c| c.file == "~/.codex/auth.json")
            .expect("the codex OAuth file must be host-read, not plugin-read");
        assert!(
            credential
                .secret
                .values()
                .any(|p| p == "tokens.access_token"),
            "the access token must be declared secret so it never enters the VM"
        );
        assert!(
            !credential
                .public
                .values()
                .any(|p| p == "tokens.access_token"),
            "the access token must never be exposed as a public field"
        );
    }

    /// Where the grant comes from, proved on the SHIPPED manifest without
    /// touching a filesystem: the confined read set is exactly bundle + data +
    /// the plugin's OWN scoped temp subdir + the ONE home codex declares —
    /// never the bare `std::env::temp_dir()`, which every process and every
    /// other plugin can also reach. Set equality is what makes "declared root,
    /// never a blanket widening" a real claim — an extra entry appearing here
    /// is a widening, and the codex home disappearing is the original bug.
    #[test]
    fn the_confined_read_set_is_exactly_the_base_roots_plus_the_one_declared_home() {
        let bundle = PathBuf::from("/bundle");
        let data = PathBuf::from("/data");
        let codex_home = dirs::home_dir().expect("home dir").join(".codex");
        let codex_manifest = shipped_codex_manifest();
        let scoped_temp = plugin_temp_dir(&codex_manifest.id);

        let (with_root, write_roots) = confined_roots(&codex_manifest, &bundle, &data);
        assert_eq!(
            with_root,
            vec![
                bundle.clone(),
                data.clone(),
                scoped_temp.clone(),
                codex_home.clone()
            ],
            "codex gets its declared home and nothing else"
        );
        assert_eq!(
            write_roots,
            vec![bundle.clone(), data.clone(), scoped_temp.clone()],
            "a declared session root is read-only — it is never a write root"
        );
        assert!(
            !with_root.contains(&std::env::temp_dir()),
            "the bare system temp dir must never be a member of the confined set"
        );
        assert!(
            !write_roots.contains(&std::env::temp_dir()),
            "the bare system temp dir must never be a member of the confined set"
        );

        let rootless: PluginManifest =
            serde_json::from_str(&min_manifest("rootless")).expect("manifest parses");
        let rootless_scoped_temp = plugin_temp_dir(&rootless.id);
        let (without_root, _) = confined_roots(&rootless, &bundle, &data);
        assert_eq!(
            without_root,
            vec![bundle, data, rootless_scoped_temp],
            "no declaration, no extra root — the state that produced the bug"
        );
        assert_eq!(
            with_root.len(),
            without_root.len() + 1,
            "the declaration adds exactly one root"
        );
        assert!(!without_root.contains(&codex_home));
    }

    /// RED-BEFORE-GREEN companion to the test above: this is the exact bug the
    /// gate exists to catch. On the pre-fix code, `confined_roots` granted the
    /// bare `std::env::temp_dir()` for every plugin with no declaration
    /// required — this assertion is what turns red on that code and green on
    /// the scoped fix. Set equality, not membership: an extra root (the bare
    /// temp dir surviving alongside the scoped one) must fail this too.
    #[test]
    fn confined_roots_never_contain_the_bare_system_temp_dir() {
        use std::collections::HashSet;

        let bundle = PathBuf::from("/bundle");
        let data = PathBuf::from("/data");
        let rootless: PluginManifest =
            serde_json::from_str(&min_manifest("no-declared-root")).expect("manifest parses");
        let (read_roots, write_roots) = confined_roots(&rootless, &bundle, &data);

        let expected: HashSet<PathBuf> = [
            bundle.clone(),
            data.clone(),
            plugin_temp_dir(&rootless.id),
        ]
        .into_iter()
        .collect();
        let read_set: HashSet<PathBuf> = read_roots.into_iter().collect();
        let write_set: HashSet<PathBuf> = write_roots.into_iter().collect();

        assert_eq!(
            read_set, expected,
            "the confined read set must equal exactly bundle + data + scoped temp"
        );
        assert_eq!(
            write_set, expected,
            "the confined write set must equal exactly bundle + data + scoped temp"
        );
        assert!(!read_set.contains(&std::env::temp_dir()));
        assert!(!write_set.contains(&std::env::temp_dir()));
    }

    /// Enforcement, with the base roots deliberately left out.
    ///
    /// Every `tempfile::tempdir()` lives INSIDE `std::env::temp_dir()` — so a
    /// fixture that builds its "outside" path from a tempdir while the
    /// sandbox is also granted the bare system temp dir proves nothing in
    /// either direction. The sandbox here is granted the declared home ALONE,
    /// which is what makes both the allow and the deny meaningful.
    #[test]
    fn a_declared_root_is_granted_while_outside_and_the_credential_stay_denied() {
        use crate::js_runtime::host_api::PluginSandbox;

        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("codex-home");
        let elsewhere = tmp.path().join("not-the-codex-home");
        for dir in [&home, &elsewhere] {
            std::fs::create_dir_all(dir).unwrap();
        }

        let mut manifest = shipped_codex_manifest();
        manifest.sessions.as_mut().unwrap().root_dir = home.to_string_lossy().into_owned();
        for credential in &mut manifest.credentials {
            credential.file = home.join("auth.json").to_string_lossy().into_owned();
        }

        let index = home.join("session_index.jsonl");
        std::fs::write(&index, "").unwrap();
        std::fs::write(home.join("auth.json"), "{}").unwrap();
        let outside = elsewhere.join("secrets.json");
        std::fs::write(&outside, "{}").unwrap();

        // The granted root comes from the manifest declaration, not from the
        // fixture — only the base roots are left out.
        let declared = PluginManifest::expand_home(&manifest.sessions.as_ref().unwrap().root_dir)
            .expect("declared root expands");
        assert_eq!(declared, home, "the sandbox is granted what codex declared");
        let sandbox = PluginSandbox::restricted(vec![declared], vec![])
            .with_credentials(&manifest.credentials);

        assert!(
            sandbox.can_read(index.to_str().unwrap()),
            "the declared home is readable, so session detection and usage work confined"
        );
        assert!(
            !sandbox.can_read(outside.to_str().unwrap()),
            "a sibling of the declared home is outside the grant"
        );
        assert!(
            !sandbox.can_read(home.join("auth.json").to_str().unwrap()),
            "the declared credential is carved back out of the declared root"
        );
        assert!(
            !sandbox.can_write(index.to_str().unwrap()),
            "a declared session root is read-only"
        );
    }

    /// The non-vacuous negative control this whole change exists to make
    /// real: a path inside the SYSTEM temp dir but OUTSIDE the plugin's own
    /// scoped subdir must be refused for both read and write, using the REAL
    /// `confined_roots` output (not a hand-picked root set). The "outside"
    /// fixture is built directly under `std::env::temp_dir()` — deliberately
    /// NOT via `tempfile::tempdir()`, because a tempdir fixture itself lives
    /// inside `std::env::temp_dir()` and, back when the bare system temp dir
    /// was a granted root, an "outside" path built that way was silently
    /// INSIDE the grant — which is exactly why the original assertion here
    /// was vacuous (always allowed, whether the code was right or not). This
    /// fixture is a sibling of the plugin's scoped subdir, directly under the
    /// real system temp root, so it is genuinely outside every granted root.
    #[test]
    fn a_path_inside_system_temp_but_outside_the_scoped_subdir_is_refused() {
        use crate::js_runtime::host_api::PluginSandbox;

        let plugin_id = format!(
            "confinement-negctl-{}",
            uuid::Uuid::new_v4().simple()
        );
        let manifest: PluginManifest =
            serde_json::from_str(&min_manifest(&plugin_id)).expect("manifest parses");

        let bundle = PathBuf::from("/bundle");
        let data = PathBuf::from("/data");
        let (read_roots, write_roots) = confined_roots(&manifest, &bundle, &data);

        let scoped_temp = plugin_temp_dir(&plugin_id);
        std::fs::create_dir_all(&scoped_temp).unwrap();
        let inside = scoped_temp.join("scratch.txt");
        std::fs::write(&inside, "mine").unwrap();

        // Sibling of the scoped subdir, still directly under the real system
        // temp root — the exact shape the old bare-temp_dir grant exposed.
        let outside_dir = std::env::temp_dir().join(format!(
            "codeterm-confinement-negctl-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&outside_dir).unwrap();
        let outside = outside_dir.join("someone-elses-scratch.txt");
        std::fs::write(&outside, "not mine").unwrap();

        let sandbox = PluginSandbox::restricted(read_roots, write_roots);

        assert!(
            sandbox.can_read(inside.to_str().unwrap()),
            "the plugin's own scoped temp file is readable"
        );
        assert!(
            sandbox.can_write(inside.to_str().unwrap()),
            "the plugin's own scoped temp file is writable"
        );
        assert!(
            !sandbox.can_read(outside.to_str().unwrap()),
            "a sibling scratch file elsewhere in system temp must be refused for reads"
        );
        assert!(
            !sandbox.can_write(outside.to_str().unwrap()),
            "a sibling scratch file elsewhere in system temp must be refused for writes"
        );

        let _ = std::fs::remove_dir_all(&scoped_temp);
        let _ = std::fs::remove_dir_all(&outside_dir);
    }

    /// The carve-out has to beat EVERY root, not just the declared one. Here
    /// the credential sits under two INDEPENDENTLY declared grants at once —
    /// `bundle_dir` (the home's own parent) and the declared session root
    /// itself — and must still be refused, or a credential would be readable
    /// through a root that has nothing to do with it. `bundle_dir` is used as
    /// the second root rather than the ambient system temp dir specifically
    /// so this double-coverage is not an artifact of `tempfile::tempdir()`
    /// landing inside `std::env::temp_dir()` (the scoped-temp fix means the
    /// plugin's temp grant no longer covers an arbitrary tempdir fixture at
    /// all, so that trap can't even be walked into here by accident).
    #[test]
    fn the_credential_carve_out_survives_a_second_root_that_also_covers_it() {
        use crate::js_runtime::host_api::PluginSandbox;

        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("codex-home");
        std::fs::create_dir_all(&home).unwrap();
        let auth = home.join("auth.json");
        std::fs::write(&auth, "{}").unwrap();

        let mut manifest = shipped_codex_manifest();
        manifest.sessions.as_mut().unwrap().root_dir = home.to_string_lossy().into_owned();
        for credential in &mut manifest.credentials {
            credential.file = auth.to_string_lossy().into_owned();
        }

        let (read_roots, write_roots) =
            confined_roots(&manifest, tmp.path(), &tmp.path().join("data"));
        let covering_roots = read_roots.iter().filter(|r| auth.starts_with(r)).count();
        assert!(
            covering_roots >= 2,
            "fixture precondition: the credential must be covered by at least \
             two independent roots, got {covering_roots}"
        );

        let sandbox = PluginSandbox::restricted(read_roots, write_roots)
            .with_credentials(&manifest.credentials);
        assert!(
            !sandbox.can_read(auth.to_str().unwrap()),
            "the deny-list is checked before every root, not just the declared one"
        );
    }

    /// `path_under` canonicalizes both sides, and falls back to a lexical path
    /// when a target and its parent do not exist. On Windows a canonicalized
    /// root carries the `\\?\` verbatim prefix and a lexical target does not,
    /// so the two shapes cannot match — the mismatch must fail CLOSED. A root
    /// that fails to canonicalize grants nothing either. Pinned for all three
    /// target shapes so neither direction can drift into an accidental match.
    #[test]
    fn a_path_outside_every_root_is_denied_whether_or_not_it_exists() {
        use crate::js_runtime::host_api::PluginSandbox;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("granted");
        let outside = tmp.path().join("ungranted");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let existing = outside.join("present.json");
        std::fs::write(&existing, "{}").unwrap();

        let sandbox = PluginSandbox::restricted(vec![root], vec![]);
        for target in [
            existing,
            outside.join("absent.json"),
            outside.join("missing-parent").join("absent.json"),
        ] {
            assert!(
                !sandbox.can_read(target.to_str().unwrap()),
                "outside every root must be refused: {}",
                target.display()
            );
        }
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

        let mut env = codeterm_util::test_env::EnvGuard::lock();

        // Case 1: unknown id, no dev mode → ThirdParty.
        env.remove("CODETERM_DEV");
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
        env.set("CODETERM_DEV", "1");
        if let Some((sys_id, _)) = crate::system_plugins::SYSTEM_PLUGIN_HASHES.first() {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join(*sys_id);
            write(&dir.join("plugin.json"), &min_manifest(sys_id));
            write(&dir.join("plugin.js"), "module.exports.default = {};");
            let p = load_one(&dir.join("plugin.json")).unwrap();
            assert_eq!(p.trust(), ProviderTrust::ThirdParty, "case 4");
        }
    }
}

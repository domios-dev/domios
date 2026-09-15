//! Bundled-plugin bytes embedded into the binary at compile time.
//!
//! `build.rs` mirrors `<repo>/plugins/provider-*/` into
//! `$OUT_DIR/bundled_plugins/<id>/` (filtered — same skip set as the
//! hash function), and `include_dir!` walks that mirror to produce a
//! compile-time `Dir`. This is the fallback used when the first-launch
//! channel fetch fails (offline install) or before the channel infra
//! lands.

use include_dir::{include_dir, Dir};

pub static EMBEDDED: Dir<'static> = include_dir!("$OUT_DIR/bundled_plugins");

/// List the plugin ids the binary ships with as an embedded fallback.
/// Order is deterministic (alphabetical by directory name from
/// include_dir's walk).
pub fn ids() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = EMBEDDED
        .dirs()
        .filter_map(|d| d.path().file_name().and_then(|n| n.to_str()))
        .collect();
    out.sort_unstable();
    out
}

/// Extract every system plugin embedded in the binary into
/// `<user_plugins_dir>/<id>/`, creating it when absent and **upgrading
/// it in place when the bundled version is strictly newer** than the
/// installed one (the CodeTerm-app-upgrade path). Returns the ids
/// actually written or upgraded. A channel update that is already ahead
/// of the bundle is left untouched — see `sync_to`. Intended for
/// first-launch / post-upgrade bootstrap; channel-based updates take
/// over from there.
pub fn bootstrap_into(user_plugins_dir: &std::path::Path) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(user_plugins_dir)?;
    let mut written = Vec::new();
    for id in ids() {
        if sync_to(id, user_plugins_dir)? {
            written.push(id.to_string());
        }
    }
    Ok(written)
}

/// Extract one plugin's tree into `<dst_root>/<id>/`. Idempotent — if
/// the destination already exists, it is **not** overwritten (we never
/// stomp user-modified bundles; that's the loader's job after a hash
/// mismatch).
pub fn extract_to(id: &str, dst_root: &std::path::Path) -> std::io::Result<bool> {
    let Some(plugin) = EMBEDDED.get_dir(id) else {
        return Ok(false);
    };
    let dst = dst_root.join(id);
    if dst.exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(&dst)?;
    write_dir(plugin, &dst)?;
    Ok(true)
}

/// Create-or-upgrade variant of `extract_to`: writes when the target
/// is absent, overwrites when the embedded bundle is a strictly newer
/// semver than what is installed, and otherwise leaves it alone (never
/// downgrades a channel-updated bundle). Returns whether bytes changed.
pub fn sync_to(id: &str, dst_root: &std::path::Path) -> std::io::Result<bool> {
    let Some(plugin) = EMBEDDED.get_dir(id) else {
        return Ok(false);
    };
    let dst = dst_root.join(id);
    if !should_replace_installed(id, &dst) {
        return Ok(false);
    }
    if dst.exists() {
        std::fs::remove_dir_all(&dst)?;
    }
    std::fs::create_dir_all(&dst)?;
    write_dir(plugin, &dst)?;
    Ok(true)
}

fn should_replace_installed(id: &str, dst: &std::path::Path) -> bool {
    if !dst.exists() {
        return true;
    }
    match (embedded_version(id), installed_version(dst)) {
        (Some(e), Some(i)) if embedded_is_newer(&e, &i) => return true,
        (Some(e), Some(i)) if installed_is_newer(&i, &e) => return false,
        _ => {}
    }
    installed_hash_differs_from_current_system(id, dst)
}

fn installed_hash_differs_from_current_system(id: &str, dst: &std::path::Path) -> bool {
    let Some(expected) = crate::system_plugins::lookup(id) else {
        return false;
    };
    crate::bundle_hash::hash_bundle(dst)
        .map(|actual| actual != expected)
        .unwrap_or(true)
}

fn manifest_version(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    value.get("version")?.as_str().map(String::from)
}

fn embedded_version(id: &str) -> Option<String> {
    let plugin = EMBEDDED.get_dir(id)?;
    let file = plugin
        .files()
        .find(|f| f.path().file_name().and_then(|n| n.to_str()) == Some("plugin.json"))?;
    manifest_version(file.contents())
}

fn installed_version(dir: &std::path::Path) -> Option<String> {
    manifest_version(&std::fs::read(dir.join("plugin.json")).ok()?)
}

fn embedded_is_newer(embedded: &str, installed: &str) -> bool {
    match (
        semver::Version::parse(embedded),
        semver::Version::parse(installed),
    ) {
        (Ok(e), Ok(i)) => e > i,
        _ => false,
    }
}

fn installed_is_newer(installed: &str, embedded: &str) -> bool {
    match (
        semver::Version::parse(installed),
        semver::Version::parse(embedded),
    ) {
        (Ok(i), Ok(e)) => i > e,
        _ => false,
    }
}

fn write_dir(dir: &Dir<'_>, dst: &std::path::Path) -> std::io::Result<()> {
    for sub in dir.dirs() {
        let name = sub
            .path()
            .file_name()
            .ok_or_else(|| std::io::Error::other("dir has no name"))?;
        let target = dst.join(name);
        std::fs::create_dir_all(&target)?;
        write_dir(sub, &target)?;
    }
    for f in dir.files() {
        let name = f
            .path()
            .file_name()
            .ok_or_else(|| std::io::Error::other("file has no name"))?;
        let target = dst.join(name);
        std::fs::write(&target, f.contents())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bundle_hash, system_plugins};

    /// Every system id has a matching embedded tree.
    #[test]
    fn embedded_covers_system_table() {
        for (id, _) in system_plugins::SYSTEM_PLUGIN_HASHES {
            assert!(
                EMBEDDED.get_dir(*id).is_some(),
                "no embedded bytes for system plugin {id:?}",
            );
        }
    }

    /// The Analytics view is a default core plugin: embedded with its UI and in the trust table.
    #[test]
    fn embedded_ships_core_analytics_view() {
        let plugin = EMBEDDED
            .get_dir("analytics")
            .expect("analytics core plugin is embedded");
        assert!(plugin.get_file("analytics/plugin.json").is_some());
        assert!(plugin.get_file("analytics/plugin.js").is_some());
        assert!(plugin.get_file("analytics/ui/index.html").is_some());
        assert!(system_plugins::lookup("analytics").is_some());
    }

    /// Extracting an embedded plugin produces bytes whose canonical
    /// hash matches the SYSTEM_PLUGIN_HASHES entry — the loader's
    /// trust check works against the embedded fallback by construction.
    #[test]
    fn extracted_hash_matches_table() {
        let tmp = tempfile::tempdir().unwrap();
        for (id, expected) in system_plugins::SYSTEM_PLUGIN_HASHES {
            let written = extract_to(id, tmp.path()).unwrap();
            assert!(written, "extract_to should write a fresh dir");
            let dest = tmp.path().join(id);
            let actual = bundle_hash::hash_bundle(&dest).unwrap();
            assert_eq!(
                actual, *expected,
                "embedded {id:?} canonical hash differs from build.rs table",
            );
        }
    }

    /// Idempotent: second extract is a no-op (return false).
    #[test]
    fn extract_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let id = match system_plugins::SYSTEM_PLUGIN_HASHES.first() {
            Some((id, _)) => *id,
            None => return,
        };
        assert!(extract_to(id, tmp.path()).unwrap());
        assert!(!extract_to(id, tmp.path()).unwrap());
    }

    /// bootstrap_into writes every system plugin into an empty dir,
    /// then the second call is a no-op (returns []).
    #[test]
    fn bootstrap_writes_all_then_idle() {
        let tmp = tempfile::tempdir().unwrap();
        let dst = tmp.path().join("plugins");
        let first = bootstrap_into(&dst).unwrap();
        let mut expected: Vec<String> = ids().into_iter().map(String::from).collect();
        expected.sort();
        let mut got = first.clone();
        got.sort();
        assert_eq!(got, expected);

        let second = bootstrap_into(&dst).unwrap();
        assert!(
            second.is_empty(),
            "second bootstrap should be a no-op, got {second:?}"
        );
    }

    /// An installed bundle older than the embedded one is upgraded in place.
    #[test]
    fn sync_upgrades_when_embedded_newer() {
        let tmp = tempfile::tempdir().unwrap();
        let Some(id) = ids().first().copied() else {
            return;
        };
        let dst = tmp.path().join(id);
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(
            dst.join("plugin.json"),
            br#"{"id":"stub","version":"0.0.1"}"#,
        )
        .unwrap();
        assert!(
            sync_to(id, tmp.path()).unwrap(),
            "older install must upgrade"
        );
        assert_ne!(installed_version(&dst).as_deref(), Some("0.0.1"));
    }

    /// A channel-updated bundle newer than the embedded one is never downgraded.
    #[test]
    fn sync_skips_when_installed_newer() {
        let tmp = tempfile::tempdir().unwrap();
        let Some(id) = ids().first().copied() else {
            return;
        };
        let dst = tmp.path().join(id);
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(
            dst.join("plugin.json"),
            br#"{"id":"stub","version":"9999.0.0"}"#,
        )
        .unwrap();
        assert!(
            !sync_to(id, tmp.path()).unwrap(),
            "newer install must not downgrade"
        );
        assert_eq!(installed_version(&dst).as_deref(), Some("9999.0.0"));
    }

    /// A same-version system plugin whose bytes no longer match this binary's
    /// build-time hash table is repaired.
    #[test]
    fn sync_repairs_same_version_hash_drift() {
        let tmp = tempfile::tempdir().unwrap();
        let Some((id, expected_hash)) = system_plugins::SYSTEM_PLUGIN_HASHES.first().copied()
        else {
            return;
        };
        let Some(version) = embedded_version(id) else {
            return;
        };
        let dst = tmp.path().join(id);
        std::fs::create_dir_all(&dst).unwrap();
        std::fs::write(
            dst.join("plugin.json"),
            format!(r#"{{"id":"{id}","version":"{version}","entry":"plugin.js"}}"#),
        )
        .unwrap();
        std::fs::write(
            dst.join("plugin.js"),
            "module.exports.default = { stale: true };",
        )
        .unwrap();

        assert!(
            sync_to(id, tmp.path()).unwrap(),
            "same-version hash drift must be repaired"
        );
        let actual = bundle_hash::hash_bundle(&dst).unwrap();
        assert_eq!(actual, expected_hash);
    }
}

//! Bundled-plugin bytes embedded into the binary at compile time.
//!
//! `build.rs` mirrors `<repo>/plugins/provider-*/` into
//! `$OUT_DIR/bundled_plugins/<id>/` (filtered — same skip set as the
//! hash function), and `include_dir!` walks that mirror to produce a
//! compile-time `Dir`. This is the fallback used when the first-launch
//! channel fetch fails (offline install) or before the channel infra
//! lands.

use include_dir::{include_dir, Dir};

pub static EMBEDDED: Dir<'static> =
    include_dir!("$OUT_DIR/bundled_plugins");

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
/// `<user_plugins_dir>/<id>/` if not already present. Returns the
/// list of ids actually written. **Never** overwrites existing
/// directories — see `extract_to`. Intended for first-launch
/// bootstrap; channel-based updates take over from there.
///
/// Quiet-no-op behaviours:
///   - `<user_plugins_dir>/<id>/` already exists → skipped.
///   - `dirs::home_dir()` returned None → caller passes whatever it
///     resolved; this function just writes wherever told.
pub fn bootstrap_into(user_plugins_dir: &std::path::Path) -> std::io::Result<Vec<String>> {
    std::fs::create_dir_all(user_plugins_dir)?;
    let mut written = Vec::new();
    for id in ids() {
        if extract_to(id, user_plugins_dir)? {
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
    use crate::plugin::{bundle_hash, system_plugins};

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
        assert!(second.is_empty(), "second bootstrap should be a no-op, got {second:?}");
    }
}

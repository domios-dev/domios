//! System plugin hash table.
//!
//! Generated at build time from `plugins/provider-*/` bundle contents
//! (see `src-tauri/build.rs` → `generate_system_plugin_hashes`). The
//! runtime loader looks each installed plugin's bundle hash up here:
//! match → grant system status (verified badge, uninstall-blocked).
//! Hash drift means somebody tampered with the bundle OR the user is
//! running a locally-modified copy (treat as third-party unverified).
//!
//! The hash algorithm lives in `bundle_hash.rs` and is reused by both
//! `build.rs` and the runtime so the two values cannot diverge.

include!(concat!(env!("OUT_DIR"), "/system_plugin_hashes.rs"));

/// Lookup helper.
pub fn lookup(id: &str) -> Option<&'static str> {
    SYSTEM_PLUGIN_HASHES
        .iter()
        .find(|(pid, _)| *pid == id)
        .map(|(_, h)| *h)
}

/// `true` when the given id is one of our shipped system plugins.
pub fn is_system_id(id: &str) -> bool {
    SYSTEM_PLUGIN_HASHES.iter().any(|(pid, _)| *pid == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: the build-generated table is non-empty in the repo
    /// checkout (we ship at least one bundled plugin). Skip if it
    /// happens to be empty (cargo-publish scenarios).
    #[test]
    fn table_has_expected_shape() {
        for (id, h) in SYSTEM_PLUGIN_HASHES {
            assert!(!id.is_empty(), "empty id in hash table");
            assert_eq!(h.len(), 64, "hash for {id:?} is not 64 hex chars");
            assert!(
                h.chars().all(
                    |c| c.is_ascii_hexdigit() && (c.is_ascii_digit() || c.is_ascii_lowercase())
                ),
                "hash for {id:?} contains non-lowercase-hex chars: {h:?}",
            );
        }
    }

    #[test]
    fn lookup_returns_table_value() {
        for (id, h) in SYSTEM_PLUGIN_HASHES {
            assert_eq!(lookup(id), Some(*h));
        }
        assert_eq!(lookup("definitely-not-a-real-id"), None);
    }
}

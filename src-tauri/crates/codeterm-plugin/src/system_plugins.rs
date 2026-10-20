pub fn hashes() -> &'static [(&'static str, &'static str)] {
    crate::bundled::installed()
        .expect("system plugin trust requires bundle installation")
        .hashes
}

/// Lookup helper.
pub fn lookup(id: &str) -> Option<&'static str> {
    hashes().iter().find(|(pid, _)| *pid == id).map(|(_, h)| *h)
}

/// `true` when the given id is one of our shipped system plugins.
pub fn is_system_id(id: &str) -> bool {
    hashes().iter().any(|(pid, _)| *pid == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: the build-generated table is non-empty in the repo
    /// checkout (we ship at least one bundled plugin). Skip if it
    /// happens to be empty (cargo-publish scenarios).
    #[test]
    fn table_has_expected_shape() {
        for (id, h) in hashes() {
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
        for (id, h) in hashes() {
            assert_eq!(lookup(id), Some(*h));
        }
        assert_eq!(lookup("definitely-not-a-real-id"), None);
    }
}

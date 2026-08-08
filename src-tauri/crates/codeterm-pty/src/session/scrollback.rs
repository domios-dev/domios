//! Per-tab bounded scrollback sidecars under `sessions/scrollback/`.
//! Additive files: restore skips missing/corrupt sidecars, so disabling
//! capture needs zero migration in either direction.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use super::store::SessionStore;

pub const DEFAULT_SCROLLBACK_TAIL_BYTES: usize = 64 * 1024;

const SIDECAR_HEADER: &str = "CTSCROLLBACK1";
const SIDECAR_SUFFIX: &str = ".scrollback";

impl SessionStore {
    fn scrollback_dir(&self) -> PathBuf {
        self.session_dir.join("scrollback")
    }

    fn scrollback_path(&self, tab_id: &str) -> PathBuf {
        self.scrollback_dir().join(sidecar_file_name(tab_id))
    }

    /// Persist the last `limit` bytes of `output` as this tab's sidecar.
    /// Empty output is a no-op so a restored-but-idle tab never truncates
    /// the sidecar it has not replayed yet.
    pub fn save_scrollback_tail(
        &self,
        tab_id: &str,
        output: &[u8],
        limit: usize,
    ) -> Result<(), String> {
        let tail = &output[output.len().saturating_sub(limit)..];
        if tail.is_empty() {
            return Ok(());
        }
        let dir = self.scrollback_dir();
        fs::create_dir_all(&dir).map_err(|e| format!("create {}: {}", dir.display(), e))?;
        let path = self.scrollback_path(tab_id);
        let tmp = dir.join(format!("{}.tmp", sidecar_file_name(tab_id)));
        let mut payload = format!("{} {}\n", SIDECAR_HEADER, tail.len()).into_bytes();
        payload.extend_from_slice(tail);
        fs::write(&tmp, &payload).map_err(|e| format!("write {}: {}", tmp.display(), e))?;
        fs::rename(&tmp, &path).map_err(|e| format!("rename → {}: {}", path.display(), e))
    }

    /// Bounded tail for this tab, or `None` when the sidecar is missing or
    /// unparseable — restore then leaves the tab blank (today's behavior).
    pub fn load_scrollback_tail(&self, tab_id: &str) -> Option<Vec<u8>> {
        let raw = fs::read(self.scrollback_path(tab_id)).ok()?;
        parse_scrollback_sidecar(&raw)
    }

    pub fn delete_scrollback_tail(&self, tab_id: &str) {
        let _ = fs::remove_file(self.scrollback_path(tab_id));
    }

    /// Remove sidecars whose tab is no longer a live scrollback-eligible tab.
    pub fn prune_scrollback_tails(&self, live_tab_ids: &HashSet<String>) {
        let keep: HashSet<String> = live_tab_ids.iter().map(|id| sidecar_file_name(id)).collect();
        let Ok(entries) = fs::read_dir(self.scrollback_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(SIDECAR_SUFFIX) && !keep.contains(&name) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

fn sidecar_file_name(tab_id: &str) -> String {
    let safe = tab_id.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    format!("{safe}{SIDECAR_SUFFIX}")
}

fn parse_scrollback_sidecar(raw: &[u8]) -> Option<Vec<u8>> {
    let newline = raw.iter().position(|b| *b == b'\n')?;
    let header = std::str::from_utf8(&raw[..newline]).ok()?;
    let (magic, declared_len) = header.split_once(' ')?;
    if magic != SIDECAR_HEADER {
        return None;
    }
    let declared_len: usize = declared_len.parse().ok()?;
    let body = &raw[newline + 1..];
    if body.len() != declared_len {
        return None;
    }
    Some(body.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_store() -> (SessionStore, TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::for_test_dir(dir.path().to_path_buf());
        (store, dir)
    }

    #[test]
    fn scrollback_round_trip_exact_bytes() {
        let (store, _dir) = fresh_store();
        let bytes: Vec<u8> = b"\x1b[32mbuild ok\x1b[0m\r\n$ \xff\xfe raw".to_vec();
        store
            .save_scrollback_tail("tab-a", &bytes, DEFAULT_SCROLLBACK_TAIL_BYTES)
            .unwrap();
        assert_eq!(store.load_scrollback_tail("tab-a").unwrap(), bytes);
        assert!(store.scrollback_path("tab-a").exists());
        assert!(
            !store.session_dir.join("last.json").exists(),
            "sidecars must live beside last.json, never inside it"
        );
    }

    #[test]
    fn scrollback_bound_persists_exactly_configured_tail() {
        let (store, _dir) = fresh_store();
        let limit = 1024;
        let over: Vec<u8> = (0..3 * limit).map(|i| (i % 251) as u8).collect();
        store.save_scrollback_tail("tab-a", &over, limit).unwrap();
        let restored = store.load_scrollback_tail("tab-a").unwrap();
        assert_eq!(restored.len(), limit);
        assert_eq!(restored, over[over.len() - limit..].to_vec());
    }

    #[test]
    fn scrollback_sidecar_deleted_when_tab_removed() {
        let (store, _dir) = fresh_store();
        store.save_scrollback_tail("tab-a", b"aaa", 64).unwrap();
        store.save_scrollback_tail("tab-b", b"bbb", 64).unwrap();

        let live: HashSet<String> = ["tab-a".to_string()].into_iter().collect();
        store.prune_scrollback_tails(&live);
        assert_eq!(store.load_scrollback_tail("tab-a").unwrap(), b"aaa");
        assert!(store.load_scrollback_tail("tab-b").is_none());

        store.delete_scrollback_tail("tab-a");
        assert!(store.load_scrollback_tail("tab-a").is_none());
    }

    #[test]
    fn scrollback_corrupt_sidecar_skipped() {
        let (store, _dir) = fresh_store();
        store.save_scrollback_tail("tab-a", b"healthy", 64).unwrap();
        let path = store.scrollback_path("tab-a");

        fs::write(&path, b"not a sidecar at all").unwrap();
        assert!(store.load_scrollback_tail("tab-a").is_none());

        fs::write(&path, b"CTSCROLLBACK1 999\nshort body").unwrap();
        assert!(store.load_scrollback_tail("tab-a").is_none());

        fs::write(&path, b"").unwrap();
        assert!(store.load_scrollback_tail("tab-a").is_none());
    }

    #[test]
    fn scrollback_empty_output_never_truncates_existing_sidecar() {
        let (store, _dir) = fresh_store();
        store.save_scrollback_tail("tab-a", b"history", 64).unwrap();
        store.save_scrollback_tail("tab-a", b"", 64).unwrap();
        assert_eq!(store.load_scrollback_tail("tab-a").unwrap(), b"history");
    }
}

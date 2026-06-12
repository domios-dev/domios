//! Sidecar layout cache: `<mem-dir>/graph_layout.json`, atomic writes, debounced flush.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

pub const LAYOUT_FORMAT_VERSION: u64 = 1;
const FLUSH_DEBOUNCE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Position {
    pub x: f32,
    pub y: f32,
    pub cluster_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskFile {
    format_version: u64,
    layout_version: u64,
    computed_at: String,
    positions: HashMap<String, [f32; 3]>,
}

#[derive(Default)]
struct Inner {
    positions: HashMap<String, Position>,
    seq: HashMap<String, u64>,
    layout_version: u64,
    computed_at: String,
}

pub struct LayoutCache {
    path: PathBuf,
    inner: RwLock<Inner>,
    seq_counter: AtomicU64,
    dirty_since: Mutex<Option<Instant>>,
}

impl LayoutCache {
    /// Load from disk. Corrupt or unknown-format file is treated as empty so
    /// the daemon stays up and a rebuild can be scheduled by the caller.
    pub fn load(path: PathBuf) -> Self {
        let inner = match Self::read_disk(&path) {
            Ok(Some(i)) => i,
            Ok(None) => Inner::default(),
            Err(e) => {
                log::warn!(
                    "[graph_layout] cache load failed ({e}) — starting empty, rebuild needed"
                );
                Inner::default()
            }
        };
        Self {
            path,
            inner: RwLock::new(inner),
            seq_counter: AtomicU64::new(0),
            dirty_since: Mutex::new(None),
        }
    }

    fn read_disk(path: &Path) -> Result<Option<Inner>, String> {
        if !path.exists() {
            return Ok(None);
        }
        let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
        let file: DiskFile = serde_json::from_slice(&bytes).map_err(|e| format!("parse: {e}"))?;
        if file.format_version != LAYOUT_FORMAT_VERSION {
            return Err(format!(
                "format_version {} != {}",
                file.format_version, LAYOUT_FORMAT_VERSION
            ));
        }
        let positions = file
            .positions
            .into_iter()
            .map(|(id, a)| {
                (
                    id,
                    Position {
                        x: a[0],
                        y: a[1],
                        cluster_id: a[2] as u32,
                    },
                )
            })
            .collect();
        Ok(Some(Inner {
            positions,
            seq: HashMap::new(),
            layout_version: file.layout_version,
            computed_at: file.computed_at,
        }))
    }

    pub fn is_empty(&self) -> bool {
        self.inner.read().positions.is_empty()
    }

    pub fn layout_version(&self) -> u64 {
        self.inner.read().layout_version
    }

    pub fn computed_at(&self) -> String {
        self.inner.read().computed_at.clone()
    }

    pub fn current_seq(&self) -> u64 {
        self.seq_counter.load(Ordering::SeqCst)
    }

    pub fn get(&self, id: &str) -> Option<Position> {
        self.inner.read().positions.get(id).copied()
    }

    /// Snapshot all positions filtered to `keep` (live item/entity ids).
    pub fn positions_filtered(
        &self,
        keep: &std::collections::HashSet<String>,
    ) -> Vec<(String, Position)> {
        self.inner
            .read()
            .positions
            .iter()
            .filter(|(id, _)| keep.contains(*id))
            .map(|(id, p)| (id.clone(), *p))
            .collect()
    }

    /// Positions mutated after `since_seq` (incremental placements), filtered to live ids.
    pub fn positions_since(
        &self,
        since_seq: u64,
        keep: &std::collections::HashSet<String>,
    ) -> Vec<(String, Position)> {
        let g = self.inner.read();
        g.seq
            .iter()
            .filter(|(_, s)| **s > since_seq)
            .filter_map(|(id, _)| {
                if !keep.contains(id) {
                    return None;
                }
                g.positions.get(id).map(|p| (id.clone(), *p))
            })
            .collect()
    }

    /// Replace the whole map after a full projection. Bumps layout_version and
    /// resets per-id seq tracking, then schedules a debounced flush.
    pub fn replace_all(&self, positions: HashMap<String, Position>, computed_at: String) -> u64 {
        let new_version = {
            let mut g = self.inner.write();
            g.layout_version += 1;
            g.computed_at = computed_at;
            g.positions = positions;
            g.seq.clear();
            g.layout_version
        };
        self.seq_counter.store(0, Ordering::SeqCst);
        self.mark_dirty();
        new_version
    }

    /// Insert/update one position (incremental placement). Bumps the seq counter.
    pub fn upsert(&self, id: String, pos: Position) {
        let seq = self.seq_counter.fetch_add(1, Ordering::SeqCst) + 1;
        {
            let mut g = self.inner.write();
            g.positions.insert(id.clone(), pos);
            g.seq.insert(id, seq);
        }
        self.mark_dirty();
    }

    fn mark_dirty(&self) {
        let mut d = self.dirty_since.lock();
        if d.is_none() {
            *d = Some(Instant::now());
        }
    }

    /// Flush to disk if dirty and the debounce window elapsed. Returns true if written.
    pub fn flush_if_due(&self) -> bool {
        {
            let d = self.dirty_since.lock();
            match *d {
                Some(t) if t.elapsed() >= FLUSH_DEBOUNCE => {}
                _ => return false,
            }
        }
        self.flush_now()
    }

    /// Force a flush regardless of debounce. Returns true if written.
    pub fn flush_now(&self) -> bool {
        let file = {
            let g = self.inner.read();
            DiskFile {
                format_version: LAYOUT_FORMAT_VERSION,
                layout_version: g.layout_version,
                computed_at: g.computed_at.clone(),
                positions: g
                    .positions
                    .iter()
                    .map(|(id, p)| (id.clone(), [p.x, p.y, p.cluster_id as f32]))
                    .collect(),
            }
        };
        match Self::write_atomic(&self.path, &file) {
            Ok(()) => {
                *self.dirty_since.lock() = None;
                true
            }
            Err(e) => {
                log::warn!("[graph_layout] flush failed: {e}");
                false
            }
        }
    }

    fn write_atomic(path: &Path, file: &DiskFile) -> Result<(), String> {
        let bytes = serde_json::to_vec(file).map_err(|e| format!("serialize: {e}"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &bytes).map_err(|e| format!("write tmp: {e}"))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path() -> PathBuf {
        std::env::temp_dir().join(format!("graph_layout_test_{}.json", ulid::Ulid::new()))
    }

    #[test]
    fn round_trip_replace_and_reload() {
        let path = tmp_path();
        let cache = LayoutCache::load(path.clone());
        let mut map = HashMap::new();
        map.insert(
            "a".into(),
            Position {
                x: 1.0,
                y: 2.0,
                cluster_id: 3,
            },
        );
        let v = cache.replace_all(map, "now".into());
        assert_eq!(v, 1);
        assert!(cache.flush_now());

        let reloaded = LayoutCache::load(path.clone());
        assert_eq!(reloaded.layout_version(), 1);
        assert_eq!(
            reloaded.get("a"),
            Some(Position {
                x: 1.0,
                y: 2.0,
                cluster_id: 3
            })
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn corrupt_file_loads_empty_not_panic() {
        let path = tmp_path();
        std::fs::write(&path, b"{ this is : not json").unwrap();
        let cache = LayoutCache::load(path.clone());
        assert!(cache.is_empty());
        assert_eq!(cache.layout_version(), 0);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn unknown_format_version_rejected() {
        let path = tmp_path();
        let bogus = serde_json::json!({
            "format_version": 999,
            "layout_version": 7,
            "computed_at": "x",
            "positions": {}
        });
        std::fs::write(&path, serde_json::to_vec(&bogus).unwrap()).unwrap();
        let cache = LayoutCache::load(path.clone());
        assert!(cache.is_empty());
        assert_eq!(cache.layout_version(), 0);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn atomic_write_leaves_no_torn_file_when_tmp_garbage_present() {
        let path = tmp_path();
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, b"garbage partial write").unwrap();
        let cache = LayoutCache::load(path.clone());
        let mut map = HashMap::new();
        map.insert(
            "x".into(),
            Position {
                x: 5.0,
                y: 6.0,
                cluster_id: 0,
            },
        );
        cache.replace_all(map, "now".into());
        assert!(cache.flush_now());
        let reloaded = LayoutCache::load(path.clone());
        assert_eq!(reloaded.get("x").map(|p| p.x), Some(5.0));
        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn upsert_bumps_seq_and_positions_since_filters() {
        let path = tmp_path();
        let cache = LayoutCache::load(path.clone());
        cache.upsert(
            "a".into(),
            Position {
                x: 0.0,
                y: 0.0,
                cluster_id: 0,
            },
        );
        let seq_after_a = cache.current_seq();
        cache.upsert(
            "b".into(),
            Position {
                x: 1.0,
                y: 1.0,
                cluster_id: 0,
            },
        );

        let keep: std::collections::HashSet<String> =
            ["a".to_string(), "b".to_string()].into_iter().collect();
        let delta = cache.positions_since(seq_after_a, &keep);
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].0, "b");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn positions_filtered_drops_stale_ids() {
        let path = tmp_path();
        let cache = LayoutCache::load(path.clone());
        cache.upsert(
            "live".into(),
            Position {
                x: 0.0,
                y: 0.0,
                cluster_id: 0,
            },
        );
        cache.upsert(
            "dead".into(),
            Position {
                x: 1.0,
                y: 1.0,
                cluster_id: 0,
            },
        );
        let keep: std::collections::HashSet<String> = ["live".to_string()].into_iter().collect();
        let out = cache.positions_filtered(&keep);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "live");
        std::fs::remove_file(&path).ok();
    }
}

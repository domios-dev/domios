//! Read-only connections reused per (plugin id, canonical db path) while a lease
//! on the plugin is held. Without a lease every query opens and closes its own
//! connection, so no handle outlives the tabs that need it (on Windows an open
//! handle blocks the provider from deleting or replacing its own database).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;

pub(super) const POOL_CONNECTIONS_PER_PLUGIN: usize = 8;

/// `state` is a leaf lock held only for map edits: never across a query, a VM
/// call or a mux guard; connections open and close outside it.
pub(super) struct ConnectionPool {
    pub(super) state: Mutex<PoolState>,
    pub(super) opens: AtomicUsize,
}

#[derive(Default)]
pub(super) struct PoolState {
    pub(super) entries: HashMap<String, Vec<PooledConnection>>,
    leases: HashMap<String, usize>,
}

pub(super) struct PooledConnection {
    requested: String,
    canonical: PathBuf,
    pub(super) identity: FileIdentity,
    pub(super) conn: rusqlite::Connection,
}

/// Keeps a plugin's connections pooled until the last lease drops.
pub struct PluginSqliteLease<'a> {
    pool: &'a ConnectionPool,
    plugin_id: String,
}

impl Drop for PluginSqliteLease<'_> {
    fn drop(&mut self) {
        self.pool.release(&self.plugin_id);
    }
}

/// Which file a path names, so a replaced db never serves the old file. Windows
/// uses creation time: the stable file index needs an unstable std feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FileIdentity {
    pub(super) volume: u64,
    pub(super) file: u128,
}

impl FileIdentity {
    fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Some(Self {
                volume: metadata.dev(),
                file: u128::from(metadata.ino()),
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            Some(Self {
                volume: 0,
                file: u128::from(metadata.creation_time()),
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = metadata;
            Some(Self { volume: 0, file: 0 })
        }
    }
}

impl ConnectionPool {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(PoolState::default()),
            opens: AtomicUsize::new(0),
        }
    }

    pub(super) fn lease(&self, plugin_id: &str) -> PluginSqliteLease<'_> {
        *self
            .state
            .lock()
            .leases
            .entry(plugin_id.to_string())
            .or_default() += 1;
        PluginSqliteLease {
            pool: self,
            plugin_id: plugin_id.to_string(),
        }
    }

    fn release(&self, plugin_id: &str) {
        let closed = {
            let mut state = self.state.lock();
            let Some(count) = state.leases.get_mut(plugin_id) else {
                return;
            };
            *count -= 1;
            if *count > 0 {
                return;
            }
            state.leases.remove(plugin_id);
            state.entries.remove(plugin_id)
        };
        drop(closed);
    }

    /// A pooled connection for `db_path` when one matches the file's identity,
    /// otherwise a fresh one; `None` when the file is missing or unopenable.
    pub(super) fn acquire(&self, plugin_id: &str, db_path: &str) -> Option<PooledConnection> {
        let resolved = std::fs::canonicalize(db_path)
            .ok()
            .and_then(|canonical| Some((FileIdentity::of(&canonical)?, canonical)));
        let Some((identity, canonical)) = resolved else {
            drop(self.evict(plugin_id, |entry| entry.requested == db_path));
            log::debug!("[sqlite-session-probe] stage=open db={db_path} error=missing");
            return None;
        };
        let (taken, stale) = {
            let mut state = self.state.lock();
            match state.entries.get_mut(plugin_id) {
                Some(list) => {
                    let (stale, kept): (Vec<_>, Vec<_>) =
                        std::mem::take(list).into_iter().partition(|entry| {
                            entry.canonical == canonical && entry.identity != identity
                        });
                    *list = kept;
                    let taken = list
                        .iter()
                        .rposition(|entry| entry.canonical == canonical)
                        .map(|index| list.remove(index));
                    (taken, stale)
                }
                None => (None, Vec::new()),
            }
        };
        drop(stale);
        if taken.is_some() {
            return taken;
        }
        let conn = super::open_read_only(db_path)?;
        self.opens.fetch_add(1, Ordering::Relaxed);
        Some(PooledConnection {
            requested: db_path.to_string(),
            canonical,
            identity,
            conn,
        })
    }

    /// Pools the connection only under a lease, outside any transaction, and
    /// after a query that did not fault the underlying file handle.
    pub(super) fn checkin(&self, plugin_id: &str, entry: PooledConnection, faulted: bool) {
        // A plugin-issued BEGIN would pin a stale snapshot for every later call.
        if faulted || !entry.conn.is_autocommit() {
            if faulted {
                drop(self.evict(plugin_id, |pooled| pooled.canonical == entry.canonical));
            }
            return;
        }
        let closed = {
            let mut state = self.state.lock();
            if state.leases.get(plugin_id).copied().unwrap_or(0) == 0 {
                vec![entry]
            } else {
                let list = state.entries.entry(plugin_id.to_string()).or_default();
                list.push(entry);
                let excess = list.len().saturating_sub(POOL_CONNECTIONS_PER_PLUGIN);
                list.drain(..excess).collect()
            }
        };
        drop(closed);
    }

    fn evict(
        &self,
        plugin_id: &str,
        matches: impl Fn(&PooledConnection) -> bool,
    ) -> Vec<PooledConnection> {
        let mut state = self.state.lock();
        let Some(list) = state.entries.get_mut(plugin_id) else {
            return Vec::new();
        };
        let (gone, kept) = std::mem::take(list).into_iter().partition(|e| matches(e));
        *list = kept;
        gone
    }

    pub(super) fn drop_plugin(&self, plugin_id: &str) {
        let removed = self.state.lock().entries.remove(plugin_id);
        drop(removed);
    }

    #[cfg(test)]
    pub(super) fn pooled(&self, plugin_id: &str) -> usize {
        self.state.lock().entries.get(plugin_id).map_or(0, Vec::len)
    }
}

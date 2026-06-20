//! sqlite-backed "recently closed" history.
//!
//! A growing, bounded, append+query history does not belong in the full-state
//! JSON snapshot (that file is rewritten WHOLE on every state change). It lives
//! here instead: one `closed_group` row per close action and one `closed_pane`
//! row per pane, keyed by `workspace_id`. Insert on close, trim oldest, query
//! newest-N for the UI, reopen by taking (fetch + delete) a pane or a whole
//! group.
//!
//! Schema follows the same rusqlite pattern as `agent::ledger` (WAL,
//! `busy_timeout`, `PRAGMA user_version` migration ladder) — no new db harness.

use parking_lot::Mutex;
use rusqlite::{params, Connection};

use crate::multiplexer::types::{ClosedPane, PaneBacking};

const CURRENT_CLOSED_SCHEMA_VERSION: u32 = 1;

/// Per-workspace cap on retained closed panes. Browser-style: oldest panes fall
/// off once exceeded. A group whose every pane has been trimmed is deleted too.
pub const RECENTLY_CLOSED_CAP: usize = 1000;

/// Global cap on retained groups across all workspaces. Bounds growth from many
/// created-and-deleted workspaces. Oldest whole groups (and their panes) drop.
pub const CLOSED_GROUP_CAP: usize = 1000;

/// How a set of panes was closed together. A `Single` close is one tab/pane; a
/// `Cascade` is an orchestrator + its spawned subtree; a `Workspace` is a whole
/// workspace and its tab list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupKind {
    Single,
    Cascade,
    Workspace,
}

impl GroupKind {
    pub fn as_str(self) -> &'static str {
        match self {
            GroupKind::Single => "single",
            GroupKind::Cascade => "cascade",
            GroupKind::Workspace => "workspace",
        }
    }

    fn parse(s: &str) -> GroupKind {
        match s {
            "cascade" => GroupKind::Cascade,
            "workspace" => GroupKind::Workspace,
            _ => GroupKind::Single,
        }
    }
}

/// One close action: the umbrella record that 1+ [`ClosedPane`] rows reference.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedGroup {
    pub group_id: String,
    pub kind: GroupKind,
    pub label: String,
    /// Origin workspace id. For a `Workspace` group this is the closed
    /// workspace's own id (used to recreate it on reopen).
    pub workspace_id: String,
    pub workspace_name: String,
    /// Workspace cwd — only meaningful for a `Workspace` group, lets reopen
    /// recreate the workspace at the same folder.
    pub workspace_cwd: Option<String>,
    pub closed_at: u64,
}

/// A pane row joined with its group's id + kind — the shape the snapshot view
/// builder and reopen paths consume.
#[derive(Debug, Clone)]
pub struct ClosedRow {
    pub group_id: String,
    pub group_kind: GroupKind,
    pub pane: ClosedPane,
}

/// sqlite-backed closed-history store. Leaf lock (its own `Mutex<Connection>`),
/// only ever taken while the mux lock is held; never across an await.
pub struct ClosedPanesStore {
    conn: Mutex<Connection>,
}

impl ClosedPanesStore {
    /// Open (or create) the db under `data_dir/closed_history.db`.
    pub fn open(data_dir: &std::path::Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| format!("create data dir: {}", e))?;
        Self::open_at(&data_dir.join("closed_history.db"))
    }

    /// Open a file-backed store at an explicit path (durability tests + callers
    /// that own the path).
    pub fn open_at(db_path: &std::path::Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {}", e))?;
        }
        let conn =
            Connection::open(db_path).map_err(|e| format!("open closed_history.db: {}", e))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("set busy_timeout: {}", e))?;
        Self::init_schema(&conn).map_err(|e| format!("init closed schema: {}", e))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// In-memory store — the default for `MultiplexerState` constructors and
    /// tests. Production swaps in a file-backed store at startup. Opening an
    /// in-memory db effectively never fails; a failure here is unrecoverable.
    pub fn in_memory() -> Self {
        let conn = Connection::open_in_memory().expect("open in-memory closed_history db");
        Self::init_schema(&conn).expect("init in-memory closed schema");
        Self {
            conn: Mutex::new(conn),
        }
    }

    fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            "#,
        )?;
        let version = Self::user_version(conn)?;
        Self::migrate(conn, version)
    }

    fn migrate(conn: &Connection, from_version: u32) -> rusqlite::Result<()> {
        let mut version = from_version;
        loop {
            match version {
                0 => {
                    Self::create_v1_schema(conn)?;
                    version = 1;
                }
                CURRENT_CLOSED_SCHEMA_VERSION => return Ok(()),
                _ => return Err(rusqlite::Error::InvalidQuery),
            }
        }
    }

    fn create_v1_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS closed_group (
                group_id       TEXT PRIMARY KEY,
                kind           TEXT NOT NULL DEFAULT 'single',
                label          TEXT NOT NULL DEFAULT '',
                workspace_id   TEXT NOT NULL DEFAULT '',
                workspace_name TEXT NOT NULL DEFAULT '',
                workspace_cwd  TEXT,
                closed_at      INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS closed_pane (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                group_id         TEXT NOT NULL,
                pane_id          TEXT NOT NULL DEFAULT '',
                parent_pane_id   TEXT,
                title            TEXT NOT NULL DEFAULT '',
                description      TEXT,
                backing_json     TEXT NOT NULL DEFAULT '{"kind":"pty"}',
                cwd              TEXT,
                agent_type       TEXT,
                agent_session_id TEXT,
                model            TEXT,
                workspace_id     TEXT NOT NULL DEFAULT '',
                workspace_name   TEXT NOT NULL DEFAULT '',
                closed_at        INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_closed_pane_workspace
                ON closed_pane(workspace_id, closed_at DESC);
            CREATE INDEX IF NOT EXISTS idx_closed_pane_group
                ON closed_pane(group_id);
            CREATE INDEX IF NOT EXISTS idx_closed_pane_recent
                ON closed_pane(closed_at DESC);

            PRAGMA user_version = 1;
            "#,
        )
    }

    fn user_version(conn: &Connection) -> rusqlite::Result<u32> {
        conn.query_row(
            "PRAGMA user_version",
            [],
            |r| Ok(r.get::<_, i64>(0)? as u32),
        )
    }

    /// Insert one close action: the group plus its panes, in a single
    /// transaction, then enforce the per-workspace and global caps.
    pub fn insert_group(&self, group: &ClosedGroup, panes: &[ClosedPane]) -> Result<(), String> {
        if panes.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("closed insert tx: {}", e))?;
        tx.execute(
            r#"
            INSERT INTO closed_group
                (group_id, kind, label, workspace_id, workspace_name, workspace_cwd, closed_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                group.group_id,
                group.kind.as_str(),
                group.label,
                group.workspace_id,
                group.workspace_name,
                group.workspace_cwd,
                group.closed_at as i64,
            ],
        )
        .map_err(|e| format!("insert group: {}", e))?;

        for pane in panes {
            let backing_json = serde_json::to_string(&pane.backing)
                .unwrap_or_else(|_| r#"{"kind":"pty"}"#.to_string());
            tx.execute(
                r#"
                INSERT INTO closed_pane
                    (group_id, pane_id, parent_pane_id, title, description, backing_json,
                     cwd, agent_type, agent_session_id, model, workspace_id, workspace_name, closed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                "#,
                params![
                    group.group_id,
                    pane.pane_id,
                    pane.parent_pane_id,
                    pane.title,
                    pane.description,
                    backing_json,
                    pane.cwd,
                    pane.agent_type,
                    pane.agent_session_id,
                    pane.model,
                    pane.workspace_id,
                    pane.workspace_name,
                    pane.closed_at as i64,
                ],
            )
            .map_err(|e| format!("insert pane: {}", e))?;
        }

        Self::trim_workspace(&tx, &group.workspace_id, RECENTLY_CLOSED_CAP)?;
        Self::trim_global_groups(&tx, CLOSED_GROUP_CAP)?;
        Self::prune_empty_groups(&tx)?;

        tx.commit().map_err(|e| format!("closed commit: {}", e))?;
        Ok(())
    }

    /// Delete oldest panes in a workspace beyond `cap`.
    fn trim_workspace(conn: &Connection, workspace_id: &str, cap: usize) -> Result<(), String> {
        conn.execute(
            r#"
            DELETE FROM closed_pane
            WHERE workspace_id = ?1
              AND id NOT IN (
                SELECT id FROM closed_pane
                WHERE workspace_id = ?1
                ORDER BY closed_at DESC, id DESC
                LIMIT ?2
              )
            "#,
            params![workspace_id, cap as i64],
        )
        .map_err(|e| format!("trim workspace: {}", e))?;
        Ok(())
    }

    /// Delete oldest whole groups (and their panes) beyond `cap`.
    fn trim_global_groups(conn: &Connection, cap: usize) -> Result<(), String> {
        conn.execute(
            r#"
            DELETE FROM closed_pane
            WHERE group_id IN (
                SELECT group_id FROM closed_group
                WHERE group_id NOT IN (
                    SELECT group_id FROM closed_group
                    ORDER BY closed_at DESC, group_id DESC
                    LIMIT ?1
                )
            )
            "#,
            params![cap as i64],
        )
        .map_err(|e| format!("trim groups panes: {}", e))?;
        conn.execute(
            r#"
            DELETE FROM closed_group
            WHERE group_id NOT IN (
                SELECT group_id FROM closed_group
                ORDER BY closed_at DESC, group_id DESC
                LIMIT ?1
            )
            "#,
            params![cap as i64],
        )
        .map_err(|e| format!("trim groups: {}", e))?;
        Ok(())
    }

    /// Drop group rows that no longer have any panes (trimmed away).
    fn prune_empty_groups(conn: &Connection) -> Result<(), String> {
        conn.execute(
            "DELETE FROM closed_group WHERE group_id NOT IN (SELECT DISTINCT group_id FROM closed_pane)",
            [],
        )
        .map_err(|e| format!("prune empty groups: {}", e))?;
        Ok(())
    }

    /// Newest-first panes for one workspace, capped at [`RECENTLY_CLOSED_CAP`].
    pub fn list_for_workspace(&self, workspace_id: &str) -> Vec<ClosedPane> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            r#"
            SELECT pane_id, parent_pane_id, title, description, backing_json, cwd,
                   agent_type, agent_session_id, model, workspace_id, workspace_name, closed_at
            FROM closed_pane
            WHERE workspace_id = ?1
            ORDER BY closed_at DESC, id DESC
            LIMIT ?2
            "#,
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(params![workspace_id, RECENTLY_CLOSED_CAP as i64], |r| {
            Ok(Self::pane_from_row(r))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    /// Newest-first rows across all workspaces (with group id + kind), capped at
    /// `limit`. Feeds the snapshot view list.
    pub fn list_recent(&self, limit: usize) -> Vec<ClosedRow> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            r#"
            SELECT p.group_id, g.kind, p.pane_id, p.parent_pane_id, p.title, p.description,
                   p.backing_json, p.cwd, p.agent_type, p.agent_session_id, p.model,
                   p.workspace_id, p.workspace_name, p.closed_at
            FROM closed_pane p
            JOIN closed_group g ON g.group_id = p.group_id
            ORDER BY p.closed_at DESC, p.id DESC
            LIMIT ?1
            "#,
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(params![limit as i64], |r| {
            let group_id: String = r.get(0)?;
            let kind: String = r.get(1)?;
            Ok(ClosedRow {
                group_id,
                group_kind: GroupKind::parse(&kind),
                pane: Self::pane_from_row_offset(r, 2),
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    /// Take (fetch + delete) the `index`-th newest pane in a workspace.
    /// `index` 0 = most recent. Returns `None` if out of range.
    pub fn take_in_workspace(&self, workspace_id: &str, index: usize) -> Option<ClosedPane> {
        let (row_id, pane) = self.peek_in_workspace(workspace_id, index)?;
        let _ = self.delete_pane_row(row_id);
        Some(pane)
    }

    /// Fetch (WITHOUT deleting) the `index`-th newest pane in a workspace and
    /// its sqlite row id. Reopen materializes from this, then calls
    /// [`delete_pane_row`] only on success, so a failed materialize never loses
    /// the history entry (MED data-loss fix).
    ///
    /// [`delete_pane_row`]: Self::delete_pane_row
    pub fn peek_in_workspace(&self, workspace_id: &str, index: usize) -> Option<(i64, ClosedPane)> {
        let conn = self.conn.lock();
        conn.query_row(
            r#"
                SELECT id, pane_id, parent_pane_id, title, description, backing_json, cwd,
                       agent_type, agent_session_id, model, workspace_id, workspace_name, closed_at
                FROM closed_pane
                WHERE workspace_id = ?1
                ORDER BY closed_at DESC, id DESC
                LIMIT 1 OFFSET ?2
                "#,
            params![workspace_id, index as i64],
            |r| Ok((r.get::<_, i64>(0)?, Self::pane_from_row_offset(r, 1))),
        )
        .ok()
    }

    /// Delete a single pane row by sqlite id, then prune any now-empty group.
    /// Paired with [`peek_in_workspace`]/[`peek_by_pane_id`] for
    /// delete-after-success. Returns `Err` if the sqlite delete failed OR removed
    /// no row (the entry is still in history): the reopen paths must surface this,
    /// because a "succeeded" reopen whose delete silently failed leaves the entry
    /// behind and a retry then duplicates the reopened pane (MED data-loss fix).
    ///
    /// [`peek_in_workspace`]: Self::peek_in_workspace
    /// [`peek_by_pane_id`]: Self::peek_by_pane_id
    pub fn delete_pane_row(&self, row_id: i64) -> Result<(), String> {
        let conn = self.conn.lock();
        let affected =
            Self::delete_pane(&conn, row_id).map_err(|e| format!("delete closed pane row: {e}"))?;
        Self::prune_empty_groups(&conn).ok();
        if affected == 0 {
            return Err(format!(
                "closed pane row {row_id} was already gone (no row deleted)"
            ));
        }
        Ok(())
    }

    /// Count of retained panes in a workspace (for range error messages).
    pub fn count_for_workspace(&self, workspace_id: &str) -> usize {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM closed_pane WHERE workspace_id = ?1",
            params![workspace_id],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as usize)
        .unwrap_or(0)
    }

    /// Take (fetch + delete) the newest pane with a given original `pane_id`,
    /// regardless of workspace. Used by orphaned reopen.
    pub fn take_by_pane_id(&self, pane_id: &str) -> Option<ClosedPane> {
        let (row_id, pane) = self.peek_by_pane_id(pane_id)?;
        let _ = self.delete_pane_row(row_id);
        Some(pane)
    }

    /// Fetch (WITHOUT deleting) the newest pane with a given original `pane_id`
    /// and its sqlite row id. Orphaned reopen materializes from this, then
    /// deletes via [`delete_pane_row`] only on success.
    ///
    /// [`delete_pane_row`]: Self::delete_pane_row
    pub fn peek_by_pane_id(&self, pane_id: &str) -> Option<(i64, ClosedPane)> {
        let conn = self.conn.lock();
        conn.query_row(
            r#"
                SELECT id, pane_id, parent_pane_id, title, description, backing_json, cwd,
                       agent_type, agent_session_id, model, workspace_id, workspace_name, closed_at
                FROM closed_pane
                WHERE pane_id = ?1
                ORDER BY closed_at DESC, id DESC
                LIMIT 1
                "#,
            params![pane_id],
            |r| Ok((r.get::<_, i64>(0)?, Self::pane_from_row_offset(r, 1))),
        )
        .ok()
    }

    /// Take (fetch + delete) a whole group: its meta + every pane, newest-first.
    pub fn take_group(&self, group_id: &str) -> Option<(ClosedGroup, Vec<ClosedPane>)> {
        let res = self.peek_group(group_id)?;
        let _ = self.delete_group(group_id);
        Some(res)
    }

    /// Fetch (WITHOUT deleting) a whole group: its meta + every pane,
    /// newest-first. Group reopen materializes every pane from this and only
    /// then calls [`delete_group`]; a mid-loop materialize failure thus leaves
    /// the whole group intact in history instead of dropping it (MED data-loss
    /// fix — earlier panes materialized but the group can be retried).
    ///
    /// [`delete_group`]: Self::delete_group
    pub fn peek_group(&self, group_id: &str) -> Option<(ClosedGroup, Vec<ClosedPane>)> {
        let conn = self.conn.lock();
        let group = conn
            .query_row(
                r#"
                SELECT group_id, kind, label, workspace_id, workspace_name, workspace_cwd, closed_at
                FROM closed_group WHERE group_id = ?1
                "#,
                params![group_id],
                |r| {
                    let kind: String = r.get(1)?;
                    Ok(ClosedGroup {
                        group_id: r.get(0)?,
                        kind: GroupKind::parse(&kind),
                        label: r.get(2)?,
                        workspace_id: r.get(3)?,
                        workspace_name: r.get(4)?,
                        workspace_cwd: r.get(5)?,
                        closed_at: r.get::<_, i64>(6)? as u64,
                    })
                },
            )
            .ok()?;

        let panes: Vec<ClosedPane> = {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT pane_id, parent_pane_id, title, description, backing_json, cwd,
                           agent_type, agent_session_id, model, workspace_id, workspace_name, closed_at
                    FROM closed_pane WHERE group_id = ?1
                    ORDER BY closed_at DESC, id DESC
                    "#,
                )
                .ok()?;
            stmt.query_map(params![group_id], |r| Ok(Self::pane_from_row(r)))
                .map(|rows| rows.filter_map(Result::ok).collect())
                .unwrap_or_default()
        };

        Some((group, panes))
    }

    /// Delete a whole group (its panes + meta) in one transaction. Paired with
    /// [`peek_group`] for delete-after-success on reopen.
    ///
    /// [`peek_group`]: Self::peek_group
    pub fn delete_group(&self, group_id: &str) -> Result<(), String> {
        let mut conn = self.conn.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("delete group tx: {e}"))?;
        tx.execute(
            "DELETE FROM closed_pane WHERE group_id = ?1",
            params![group_id],
        )
        .map_err(|e| format!("delete group panes: {e}"))?;
        tx.execute(
            "DELETE FROM closed_group WHERE group_id = ?1",
            params![group_id],
        )
        .map_err(|e| format!("delete group meta: {e}"))?;
        tx.commit()
            .map_err(|e| format!("delete group commit: {e}"))?;
        Ok(())
    }

    fn delete_pane(conn: &Connection, row_id: i64) -> rusqlite::Result<usize> {
        conn.execute("DELETE FROM closed_pane WHERE id = ?1", params![row_id])
    }

    /// Deserialize a [`ClosedPane`] from a row whose first column is `pane_id`.
    fn pane_from_row(r: &rusqlite::Row<'_>) -> ClosedPane {
        Self::pane_from_row_offset(r, 0)
    }

    /// Deserialize a [`ClosedPane`] from a row where `pane_id` starts at `base`.
    fn pane_from_row_offset(r: &rusqlite::Row<'_>, base: usize) -> ClosedPane {
        let backing_json: String = r.get(base + 4).unwrap_or_default();
        let backing = serde_json::from_str::<PaneBacking>(&backing_json).unwrap_or_default();
        ClosedPane {
            pane_id: r.get(base).unwrap_or_default(),
            parent_pane_id: r.get(base + 1).ok().flatten(),
            title: r.get(base + 2).unwrap_or_default(),
            description: r.get(base + 3).ok().flatten(),
            backing,
            cwd: r.get(base + 5).ok().flatten(),
            agent_type: r.get(base + 6).ok().flatten(),
            agent_session_id: r.get(base + 7).ok().flatten(),
            model: r.get(base + 8).ok().flatten(),
            workspace_id: r.get(base + 9).unwrap_or_default(),
            workspace_name: r.get(base + 10).unwrap_or_default(),
            closed_at: r.get::<_, i64>(base + 11).unwrap_or(0) as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(pane_id: &str, ws: &str, title: &str, closed_at: u64) -> ClosedPane {
        ClosedPane {
            pane_id: pane_id.into(),
            parent_pane_id: None,
            title: title.into(),
            description: None,
            backing: PaneBacking::Pty,
            cwd: None,
            agent_type: None,
            agent_session_id: None,
            model: None,
            closed_at,
            workspace_id: ws.into(),
            workspace_name: "WS".into(),
        }
    }

    fn group(id: &str, kind: GroupKind, ws: &str, closed_at: u64) -> ClosedGroup {
        ClosedGroup {
            group_id: id.into(),
            kind,
            label: "".into(),
            workspace_id: ws.into(),
            workspace_name: "WS".into(),
            workspace_cwd: None,
            closed_at,
        }
    }

    fn temp_db() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "closed-test-{}/closed_history.db",
            ulid::Ulid::new()
        ))
    }

    #[test]
    fn insert_then_list_newest_first() {
        let store = ClosedPanesStore::in_memory();
        store
            .insert_group(
                &group("g1", GroupKind::Single, "w", 100),
                &[pane("p1", "w", "first", 100)],
            )
            .unwrap();
        store
            .insert_group(
                &group("g2", GroupKind::Single, "w", 200),
                &[pane("p2", "w", "second", 200)],
            )
            .unwrap();
        let list = store.list_for_workspace("w");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].title, "second", "newest first");
        assert_eq!(list[1].title, "first");
    }

    #[test]
    fn per_workspace_cap_drops_oldest() {
        let store = ClosedPanesStore::in_memory();
        for i in 0..(RECENTLY_CLOSED_CAP + 5) {
            store
                .insert_group(
                    &group(&format!("g{i}"), GroupKind::Single, "w", i as u64),
                    &[pane(&format!("p{i}"), "w", "t", i as u64)],
                )
                .unwrap();
        }
        assert_eq!(store.count_for_workspace("w"), RECENTLY_CLOSED_CAP);
        assert_eq!(store.list_for_workspace("w").len(), RECENTLY_CLOSED_CAP);
    }

    #[test]
    fn take_in_workspace_fetches_and_removes() {
        let store = ClosedPanesStore::in_memory();
        store
            .insert_group(
                &group("g1", GroupKind::Single, "w", 1),
                &[pane("older", "w", "older", 1)],
            )
            .unwrap();
        store
            .insert_group(
                &group("g2", GroupKind::Single, "w", 2),
                &[pane("newer", "w", "newer", 2)],
            )
            .unwrap();
        // index 1 = the older one
        let taken = store.take_in_workspace("w", 1).unwrap();
        assert_eq!(taken.pane_id, "older");
        let rest = store.list_for_workspace("w");
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].pane_id, "newer");
        // group with no remaining panes is pruned
        assert!(store.take_group("g1").is_none());
    }

    #[test]
    fn take_out_of_range_returns_none() {
        let store = ClosedPanesStore::in_memory();
        store
            .insert_group(
                &group("g1", GroupKind::Single, "w", 1),
                &[pane("p", "w", "t", 1)],
            )
            .unwrap();
        assert!(store.take_in_workspace("w", 5).is_none());
    }

    #[test]
    fn take_group_returns_meta_and_all_panes() {
        let store = ClosedPanesStore::in_memory();
        let mut root = pane("root", "w", "Orchestrator", 10);
        let mut child = pane("child", "w", "Worker", 10);
        child.parent_pane_id = Some("root".into());
        root.parent_pane_id = None;
        store
            .insert_group(&group("g1", GroupKind::Cascade, "w", 10), &[root, child])
            .unwrap();
        let (g, panes) = store.take_group("g1").unwrap();
        assert_eq!(g.kind, GroupKind::Cascade);
        assert_eq!(panes.len(), 2);
        assert!(panes
            .iter()
            .any(|p| p.parent_pane_id.as_deref() == Some("root")));
        // consumed
        assert!(store.list_for_workspace("w").is_empty());
    }

    #[test]
    fn peek_group_is_non_destructive_until_delete() {
        // MED data-loss fix: reopen now peeks the group, materializes every
        // pane, and only THEN deletes. A reopen that fails before delete_group
        // must leave the whole group in history (nothing lost, retry possible).
        let store = ClosedPanesStore::in_memory();
        let root = pane("root", "w", "Orchestrator", 10);
        let mut child = pane("child", "w", "Worker", 10);
        child.parent_pane_id = Some("root".into());
        store
            .insert_group(&group("g1", GroupKind::Cascade, "w", 10), &[root, child])
            .unwrap();

        // Peeking (what reopen does first) does NOT consume the group.
        let (_g, panes) = store.peek_group("g1").expect("group present");
        assert_eq!(panes.len(), 2);
        assert_eq!(
            store.peek_group("g1").map(|(_, p)| p.len()),
            Some(2),
            "a second peek still sees the group — a failed materialize loses nothing"
        );
        assert_eq!(
            store.list_for_workspace("w").len(),
            2,
            "both panes remain in history until delete"
        );

        // Only an explicit delete (after a fully successful materialize) drops it.
        store.delete_group("g1").expect("delete group succeeds");
        assert!(
            store.peek_group("g1").is_none(),
            "group consumed only after delete_group"
        );
        assert!(store.list_for_workspace("w").is_empty());
    }

    #[test]
    fn peek_in_workspace_then_delete_row_is_a_split_take() {
        // The single-pane reopen path peeks (no delete) → materializes → deletes
        // by row id only on success.
        let store = ClosedPanesStore::in_memory();
        store
            .insert_group(
                &group("g1", GroupKind::Single, "w", 1),
                &[pane("p1", "w", "kept", 1)],
            )
            .unwrap();
        let (row_id, peeked) = store.peek_in_workspace("w", 0).expect("peeked");
        assert_eq!(peeked.pane_id, "p1");
        assert_eq!(
            store.list_for_workspace("w").len(),
            1,
            "peek leaves the pane in history"
        );
        store.delete_pane_row(row_id).expect("row deleted");
        assert!(
            store.list_for_workspace("w").is_empty(),
            "delete_pane_row removes only after success"
        );
    }

    #[test]
    fn delete_reports_failure_instead_of_swallowing() {
        // MED data-loss fix: a delete that removes no row must return Err so the
        // reopen path can surface it — a silently-swallowed failure leaves the
        // entry in history and a retry then reopens a duplicate.
        let store = ClosedPanesStore::in_memory();
        store
            .insert_group(
                &group("g1", GroupKind::Single, "w", 1),
                &[pane("p1", "w", "t", 1)],
            )
            .unwrap();
        let (row_id, _) = store.peek_in_workspace("w", 0).expect("peeked");

        // First delete removes the row and reports success.
        store
            .delete_pane_row(row_id)
            .expect("first delete succeeds");
        // Second delete of the same id removes nothing → Err (entry already gone).
        assert!(
            store.delete_pane_row(row_id).is_err(),
            "deleting an absent row must report failure, not silently 'succeed'"
        );
        // Deleting an absent group still succeeds (idempotent: nothing to retry).
        assert!(store.delete_group("nope").is_ok());
    }

    #[test]
    fn list_recent_carries_group_kind() {
        let store = ClosedPanesStore::in_memory();
        store
            .insert_group(
                &group("g1", GroupKind::Workspace, "w", 5),
                &[pane("p1", "w", "t", 5)],
            )
            .unwrap();
        let recent = store.list_recent(10);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].group_kind, GroupKind::Workspace);
        assert_eq!(recent[0].group_id, "g1");
    }

    #[test]
    fn backing_round_trips_through_sqlite() {
        let store = ClosedPanesStore::in_memory();
        let mut p = pane("p1", "w", "Chat", 1);
        p.backing = PaneBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s7".into(),
        };
        store
            .insert_group(&group("g1", GroupKind::Single, "w", 1), &[p])
            .unwrap();
        let back = store.list_for_workspace("w");
        assert_eq!(
            back[0].backing,
            PaneBacking::ChatBackend {
                plugin_id: "lmstudio".into(),
                session_id: "s7".into(),
            }
        );
    }

    #[test]
    fn survives_drop_and_reopen() {
        let db = temp_db();
        {
            let store = ClosedPanesStore::open_at(&db).unwrap();
            store
                .insert_group(
                    &group("g1", GroupKind::Single, "w", 1),
                    &[pane("p1", "w", "kept", 1)],
                )
                .unwrap();
        } // dropped — simulates restart
        let reopened = ClosedPanesStore::open_at(&db).unwrap();
        let list = reopened.list_for_workspace("w");
        assert_eq!(list.len(), 1, "history persists across reopen");
        assert_eq!(list[0].title, "kept");
        let conn = Connection::open(&db).unwrap();
        assert_eq!(
            ClosedPanesStore::user_version(&conn).unwrap(),
            CURRENT_CLOSED_SCHEMA_VERSION
        );
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }

    #[test]
    fn global_group_cap_drops_oldest_groups() {
        let store = ClosedPanesStore::in_memory();
        for i in 0..(CLOSED_GROUP_CAP + 3) {
            // each in its own workspace so per-workspace cap never fires
            let ws = format!("w{i}");
            store
                .insert_group(
                    &group(&format!("g{i}"), GroupKind::Workspace, &ws, i as u64),
                    &[pane(&format!("p{i}"), &ws, "t", i as u64)],
                )
                .unwrap();
        }
        let total: usize = store.list_recent(CLOSED_GROUP_CAP + 100).len();
        assert_eq!(total, CLOSED_GROUP_CAP, "global group cap bounds total");
        // oldest workspace gone
        assert!(store.list_for_workspace("w0").is_empty());
    }

    #[test]
    fn reopening_existing_db_is_idempotent() {
        let db = temp_db();
        let s1 = ClosedPanesStore::open_at(&db).unwrap();
        s1.insert_group(
            &group("g1", GroupKind::Single, "w", 1),
            &[pane("p1", "w", "t", 1)],
        )
        .unwrap();
        drop(s1);
        let s2 = ClosedPanesStore::open_at(&db).unwrap();
        assert_eq!(s2.list_for_workspace("w").len(), 1);
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }
}

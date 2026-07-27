//! sqlite-backed "recently closed" history — kept out of the full-state JSON
//! snapshot, which is rewritten whole on every state change. One `closed_group`
//! row per close action, one `closed_tab` row per tab, keyed by `workspace_id`.

use parking_lot::Mutex;
use rusqlite::{params, Connection};

use crate::multiplexer::types::{ClosedTab, TabBacking};

const CURRENT_CLOSED_SCHEMA_VERSION: u32 = 3;

/// Per-workspace cap on retained closed tabs; oldest fall off once exceeded.
pub const RECENTLY_CLOSED_CAP: usize = 1000;

/// Global cap on retained groups across all workspaces.
pub const CLOSED_GROUP_CAP: usize = 1000;

/// How a set of tabs was closed: a single tab, an orchestrator subtree, or a whole workspace.
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

/// One close action: the umbrella record that 1+ [`ClosedTab`] rows reference.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedGroup {
    pub group_id: String,
    pub kind: GroupKind,
    pub label: String,
    /// Origin workspace id; for a `Workspace` group, the closed workspace's own id (recreated on reopen).
    pub workspace_id: String,
    pub workspace_name: String,
    /// Workspace cwd — only set for a `Workspace` group, so reopen recreates it at the same folder.
    pub workspace_cwd: Option<String>,
    pub closed_at: u64,
}

/// A tab row joined with its group's id + kind.
#[derive(Debug, Clone)]
pub struct ClosedRow {
    pub group_id: String,
    pub group_kind: GroupKind,
    pub tab: ClosedTab,
}

/// sqlite-backed closed-history store. Leaf lock, only taken under the mux lock, never across an await.
pub struct ClosedTabsStore {
    conn: Mutex<Connection>,
}

impl ClosedTabsStore {
    /// Open (or create) the db under `data_dir/closed_history.db`.
    pub fn open(data_dir: &std::path::Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| format!("create data dir: {}", e))?;
        Self::open_at(&data_dir.join("closed_history.db"))
    }

    /// Open a file-backed store at an explicit path.
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

    /// In-memory store — the default for constructors and tests; prod swaps in a file-backed store at startup.
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
                    version = CURRENT_CLOSED_SCHEMA_VERSION;
                }
                1 => {
                    Self::migrate_v2_add_chat_view(conn)?;
                    version = 2;
                }
                2 => {
                    Self::migrate_v3_add_role_profile(conn)?;
                    version = 3;
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

            CREATE TABLE IF NOT EXISTS closed_tab (
                id               INTEGER PRIMARY KEY AUTOINCREMENT,
                group_id         TEXT NOT NULL,
                tab_id          TEXT NOT NULL DEFAULT '',
                parent_tab_id   TEXT,
                title            TEXT NOT NULL DEFAULT '',
                description      TEXT,
                backing_json     TEXT NOT NULL DEFAULT '{"kind":"pty"}',
                cwd              TEXT,
                agent_type       TEXT,
                agent_session_id TEXT,
                role_profile     TEXT,
                model            TEXT,
                chat_view        INTEGER NOT NULL DEFAULT 0,
                workspace_id     TEXT NOT NULL DEFAULT '',
                workspace_name   TEXT NOT NULL DEFAULT '',
                closed_at        INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_closed_tab_workspace
                ON closed_tab(workspace_id, closed_at DESC);
            CREATE INDEX IF NOT EXISTS idx_closed_tab_group
                ON closed_tab(group_id);
            CREATE INDEX IF NOT EXISTS idx_closed_tab_recent
                ON closed_tab(closed_at DESC);

            PRAGMA user_version = 3;
            "#,
        )
    }

    fn migrate_v2_add_chat_view(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            r#"
            ALTER TABLE closed_tab ADD COLUMN chat_view INTEGER NOT NULL DEFAULT 0;
            PRAGMA user_version = 2;
            "#,
        )
    }

    fn migrate_v3_add_role_profile(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            r#"
            ALTER TABLE closed_tab ADD COLUMN role_profile TEXT;
            PRAGMA user_version = 3;
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

    /// Insert one close action (group + tabs) in a single transaction, then enforce the caps.
    pub fn insert_group(&self, group: &ClosedGroup, tabs: &[ClosedTab]) -> Result<(), String> {
        if tabs.is_empty() {
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

        for tab in tabs {
            let backing_json = serde_json::to_string(&tab.backing)
                .unwrap_or_else(|_| r#"{"kind":"pty"}"#.to_string());
            tx.execute(
                r#"
                INSERT INTO closed_tab
                    (group_id, tab_id, parent_tab_id, title, description, backing_json,
                     cwd, agent_type, agent_session_id, role_profile, model, chat_view, workspace_id, workspace_name, closed_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                "#,
                params![
                    group.group_id,
                    tab.tab_id,
                    tab.parent_tab_id,
                    tab.title,
                    tab.description,
                    backing_json,
                    tab.cwd,
                    tab.agent_type,
                    tab.agent_session_id,
                    tab.role_profile,
                    tab.model,
                    if tab.chat_view { 1 } else { 0 },
                    tab.workspace_id,
                    tab.workspace_name,
                    tab.closed_at as i64,
                ],
            )
            .map_err(|e| format!("insert tab: {}", e))?;
        }

        Self::trim_workspace(&tx, &group.workspace_id, RECENTLY_CLOSED_CAP)?;
        Self::trim_global_groups(&tx, CLOSED_GROUP_CAP)?;
        Self::prune_empty_groups(&tx)?;

        tx.commit().map_err(|e| format!("closed commit: {}", e))?;
        Ok(())
    }

    /// Delete oldest tabs in a workspace beyond `cap`.
    fn trim_workspace(conn: &Connection, workspace_id: &str, cap: usize) -> Result<(), String> {
        conn.execute(
            r#"
            DELETE FROM closed_tab
            WHERE workspace_id = ?1
              AND id NOT IN (
                SELECT id FROM closed_tab
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

    /// Delete oldest whole groups (and their tabs) beyond `cap`.
    ///
    /// Ties on `closed_at` (same millisecond) break on `rowid DESC`, matching
    /// insertion order — `group_id` (a ULID) has a random tail on collision and
    /// would rank ties differently from `trim_workspace`'s `id DESC` on
    /// `closed_tab`, letting the two caps disagree on which rows survive.
    fn trim_global_groups(conn: &Connection, cap: usize) -> Result<(), String> {
        conn.execute(
            r#"
            DELETE FROM closed_tab
            WHERE group_id IN (
                SELECT group_id FROM closed_group
                WHERE group_id NOT IN (
                    SELECT group_id FROM closed_group
                    ORDER BY closed_at DESC, rowid DESC
                    LIMIT ?1
                )
            )
            "#,
            params![cap as i64],
        )
        .map_err(|e| format!("trim groups Tabs: {}", e))?;
        conn.execute(
            r#"
            DELETE FROM closed_group
            WHERE group_id NOT IN (
                SELECT group_id FROM closed_group
                ORDER BY closed_at DESC, rowid DESC
                LIMIT ?1
            )
            "#,
            params![cap as i64],
        )
        .map_err(|e| format!("trim groups: {}", e))?;
        Ok(())
    }

    /// Drop group rows that no longer have any tabs (trimmed away).
    fn prune_empty_groups(conn: &Connection) -> Result<(), String> {
        conn.execute(
            "DELETE FROM closed_group WHERE group_id NOT IN (SELECT DISTINCT group_id FROM closed_tab)",
            [],
        )
        .map_err(|e| format!("prune empty groups: {}", e))?;
        Ok(())
    }

    /// Newest-first tabs for one workspace, capped at [`RECENTLY_CLOSED_CAP`].
    pub fn list_for_workspace(&self, workspace_id: &str) -> Vec<ClosedTab> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            r#"
            SELECT tab_id, parent_tab_id, title, description, backing_json, cwd,
                   agent_type, agent_session_id, role_profile, model, chat_view, workspace_id, workspace_name, closed_at
            FROM closed_tab
            WHERE workspace_id = ?1
            ORDER BY closed_at DESC, id DESC
            LIMIT ?2
            "#,
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(params![workspace_id, RECENTLY_CLOSED_CAP as i64], |r| {
            Ok(Self::tab_from_row(r))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    /// Newest-first rows across all workspaces (with group id + kind), capped at `limit`.
    pub fn list_recent(&self, limit: usize) -> Vec<ClosedRow> {
        let conn = self.conn.lock();
        let mut stmt = match conn.prepare(
            r#"
            SELECT p.group_id, g.kind, p.tab_id, p.parent_tab_id, p.title, p.description,
                   p.backing_json, p.cwd, p.agent_type, p.agent_session_id, p.role_profile, p.model, p.chat_view,
                   p.workspace_id, p.workspace_name, p.closed_at
            FROM closed_tab p
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
                tab: Self::tab_from_row_offset(r, 2),
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    /// Take (fetch + delete) the `index`-th newest tab in a workspace.
    /// `index` 0 = most recent. Returns `None` if out of range.
    pub fn take_in_workspace(&self, workspace_id: &str, index: usize) -> Option<ClosedTab> {
        let (row_id, tab) = self.peek_in_workspace(workspace_id, index)?;
        let _ = self.delete_tab_row(row_id);
        Some(tab)
    }

    /// Fetch (without deleting) the `index`-th newest tab + its row id, so reopen
    /// can delete only after a successful materialize and never lose the entry.
    pub fn peek_in_workspace(&self, workspace_id: &str, index: usize) -> Option<(i64, ClosedTab)> {
        let conn = self.conn.lock();
        conn.query_row(
            r#"
                SELECT id, tab_id, parent_tab_id, title, description, backing_json, cwd,
                       agent_type, agent_session_id, role_profile, model, chat_view, workspace_id, workspace_name, closed_at
                FROM closed_tab
                WHERE workspace_id = ?1
                ORDER BY closed_at DESC, id DESC
                LIMIT 1 OFFSET ?2
                "#,
            params![workspace_id, index as i64],
            |r| Ok((r.get::<_, i64>(0)?, Self::tab_from_row_offset(r, 1))),
        )
        .ok()
    }

    /// Delete a single tab row by id, then prune any now-empty group. `Err` if the
    /// delete removed no row — else a silently-failed reopen-delete duplicates on retry.
    pub fn delete_tab_row(&self, row_id: i64) -> Result<(), String> {
        let conn = self.conn.lock();
        let affected =
            Self::delete_tab(&conn, row_id).map_err(|e| format!("delete closed Tab row: {e}"))?;
        Self::prune_empty_groups(&conn).ok();
        if affected == 0 {
            return Err(format!(
                "closed Tab row {row_id} was already gone (no row deleted)"
            ));
        }
        Ok(())
    }

    /// Count of retained tabs in a workspace (for range error messages).
    pub fn count_for_workspace(&self, workspace_id: &str) -> usize {
        let conn = self.conn.lock();
        conn.query_row(
            "SELECT COUNT(*) FROM closed_tab WHERE workspace_id = ?1",
            params![workspace_id],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n as usize)
        .unwrap_or(0)
    }

    /// Take (fetch + delete) the newest tab with a given original `tab_id`,
    /// regardless of workspace. Used by orphaned reopen.
    pub fn take_by_tab_id(&self, tab_id: &str) -> Option<ClosedTab> {
        let (row_id, tab) = self.peek_by_tab_id(tab_id)?;
        let _ = self.delete_tab_row(row_id);
        Some(tab)
    }

    /// Fetch (without deleting) the newest tab with a given `tab_id` + its row id, for orphaned reopen.
    pub fn peek_by_tab_id(&self, tab_id: &str) -> Option<(i64, ClosedTab)> {
        let conn = self.conn.lock();
        conn.query_row(
            r#"
                SELECT id, tab_id, parent_tab_id, title, description, backing_json, cwd,
                       agent_type, agent_session_id, role_profile, model, chat_view, workspace_id, workspace_name, closed_at
                FROM closed_tab
                WHERE tab_id = ?1
                ORDER BY closed_at DESC, id DESC
                LIMIT 1
                "#,
            params![tab_id],
            |r| Ok((r.get::<_, i64>(0)?, Self::tab_from_row_offset(r, 1))),
        )
        .ok()
    }

    /// Take (fetch + delete) a whole group: its meta + every tab, newest-first.
    pub fn take_group(&self, group_id: &str) -> Option<(ClosedGroup, Vec<ClosedTab>)> {
        let res = self.peek_group(group_id)?;
        let _ = self.delete_group(group_id);
        Some(res)
    }

    /// Fetch (without deleting) a whole group: meta + every tab, newest-first; so a
    /// mid-loop materialize failure on reopen leaves the group intact for retry.
    pub fn peek_group(&self, group_id: &str) -> Option<(ClosedGroup, Vec<ClosedTab>)> {
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

        let tabs: Vec<ClosedTab> = {
            let mut stmt = conn
                .prepare(
                    r#"
                    SELECT tab_id, parent_tab_id, title, description, backing_json, cwd,
                           agent_type, agent_session_id, role_profile, model, chat_view, workspace_id, workspace_name, closed_at
                    FROM closed_tab WHERE group_id = ?1
                    ORDER BY closed_at DESC, id DESC
                    "#,
                )
                .ok()?;
            stmt.query_map(params![group_id], |r| Ok(Self::tab_from_row(r)))
                .map(|rows| rows.filter_map(Result::ok).collect())
                .unwrap_or_default()
        };

        Some((group, tabs))
    }

    /// Delete a whole group (tabs + meta) in one transaction.
    pub fn delete_group(&self, group_id: &str) -> Result<(), String> {
        let mut conn = self.conn.lock();
        let tx = conn
            .transaction()
            .map_err(|e| format!("delete group tx: {e}"))?;
        tx.execute(
            "DELETE FROM closed_tab WHERE group_id = ?1",
            params![group_id],
        )
        .map_err(|e| format!("delete group Tabs: {e}"))?;
        tx.execute(
            "DELETE FROM closed_group WHERE group_id = ?1",
            params![group_id],
        )
        .map_err(|e| format!("delete group meta: {e}"))?;
        tx.commit()
            .map_err(|e| format!("delete group commit: {e}"))?;
        Ok(())
    }

    fn delete_tab(conn: &Connection, row_id: i64) -> rusqlite::Result<usize> {
        conn.execute("DELETE FROM closed_tab WHERE id = ?1", params![row_id])
    }

    /// Deserialize a [`ClosedTab`] from a row whose first column is `tab_id`.
    fn tab_from_row(r: &rusqlite::Row<'_>) -> ClosedTab {
        Self::tab_from_row_offset(r, 0)
    }

    /// Deserialize a [`ClosedTab`] from a row where `tab_id` starts at `base`.
    fn tab_from_row_offset(r: &rusqlite::Row<'_>, base: usize) -> ClosedTab {
        let backing_json: String = r.get(base + 4).unwrap_or_default();
        let backing = serde_json::from_str::<TabBacking>(&backing_json).unwrap_or_default();
        ClosedTab {
            tab_id: r.get(base).unwrap_or_default(),
            parent_tab_id: r.get(base + 1).ok().flatten(),
            title: r.get(base + 2).unwrap_or_default(),
            description: r.get(base + 3).ok().flatten(),
            backing,
            cwd: r.get(base + 5).ok().flatten(),
            agent_type: r.get(base + 6).ok().flatten(),
            agent_session_id: r.get(base + 7).ok().flatten(),
            role_profile: r.get(base + 8).ok().flatten(),
            model: r.get(base + 9).ok().flatten(),
            chat_view: r.get::<_, i64>(base + 10).unwrap_or(0) != 0,
            workspace_id: r.get(base + 11).unwrap_or_default(),
            workspace_name: r.get(base + 12).unwrap_or_default(),
            closed_at: r.get::<_, i64>(base + 13).unwrap_or(0) as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(tab_id: &str, ws: &str, title: &str, closed_at: u64) -> ClosedTab {
        ClosedTab {
            tab_id: tab_id.into(),
            parent_tab_id: None,
            title: title.into(),
            description: None,
            backing: TabBacking::Pty,
            cwd: None,
            agent_type: None,
            agent_session_id: None,
            role_profile: None,
            model: None,
            chat_view: false,
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
        let store = ClosedTabsStore::in_memory();
        store
            .insert_group(
                &group("g1", GroupKind::Single, "w", 100),
                &[tab("p1", "w", "first", 100)],
            )
            .unwrap();
        store
            .insert_group(
                &group("g2", GroupKind::Single, "w", 200),
                &[tab("p2", "w", "second", 200)],
            )
            .unwrap();
        let list = store.list_for_workspace("w");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].title, "second", "newest first");
        assert_eq!(list[1].title, "first");
    }

    #[test]
    fn per_workspace_cap_drops_oldest() {
        let store = ClosedTabsStore::in_memory();
        for i in 0..(RECENTLY_CLOSED_CAP + 5) {
            store
                .insert_group(
                    &group(&format!("g{i}"), GroupKind::Single, "w", i as u64),
                    &[tab(&format!("p{i}"), "w", "t", i as u64)],
                )
                .unwrap();
        }
        assert_eq!(store.count_for_workspace("w"), RECENTLY_CLOSED_CAP);
        assert_eq!(store.list_for_workspace("w").len(), RECENTLY_CLOSED_CAP);
    }

    #[test]
    fn take_in_workspace_fetches_and_removes() {
        let store = ClosedTabsStore::in_memory();
        store
            .insert_group(
                &group("g1", GroupKind::Single, "w", 1),
                &[tab("older", "w", "older", 1)],
            )
            .unwrap();
        store
            .insert_group(
                &group("g2", GroupKind::Single, "w", 2),
                &[tab("newer", "w", "newer", 2)],
            )
            .unwrap();
        let taken = store.take_in_workspace("w", 1).unwrap();
        assert_eq!(taken.tab_id, "older");
        let rest = store.list_for_workspace("w");
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].tab_id, "newer");
        assert!(store.take_group("g1").is_none());
    }

    #[test]
    fn take_out_of_range_returns_none() {
        let store = ClosedTabsStore::in_memory();
        store
            .insert_group(
                &group("g1", GroupKind::Single, "w", 1),
                &[tab("p", "w", "t", 1)],
            )
            .unwrap();
        assert!(store.take_in_workspace("w", 5).is_none());
    }

    #[test]
    fn take_group_returns_meta_and_all_tabs() {
        let store = ClosedTabsStore::in_memory();
        let mut root = tab("root", "w", "Orchestrator", 10);
        let mut child = tab("child", "w", "Worker", 10);
        child.parent_tab_id = Some("root".into());
        root.parent_tab_id = None;
        store
            .insert_group(&group("g1", GroupKind::Cascade, "w", 10), &[root, child])
            .unwrap();
        let (g, tabs) = store.take_group("g1").unwrap();
        assert_eq!(g.kind, GroupKind::Cascade);
        assert_eq!(tabs.len(), 2);
        assert!(tabs
            .iter()
            .any(|p| p.parent_tab_id.as_deref() == Some("root")));
        assert!(store.list_for_workspace("w").is_empty());
    }

    #[test]
    fn peek_group_is_non_destructive_until_delete() {
        let store = ClosedTabsStore::in_memory();
        let root = tab("root", "w", "Orchestrator", 10);
        let mut child = tab("child", "w", "Worker", 10);
        child.parent_tab_id = Some("root".into());
        store
            .insert_group(&group("g1", GroupKind::Cascade, "w", 10), &[root, child])
            .unwrap();

        // Peeking (what reopen does first) does NOT consume the group.
        let (_g, tabs) = store.peek_group("g1").expect("group present");
        assert_eq!(tabs.len(), 2);
        assert_eq!(
            store.peek_group("g1").map(|(_, p)| p.len()),
            Some(2),
            "a second peek still sees the group — a failed materialize loses nothing"
        );
        assert_eq!(
            store.list_for_workspace("w").len(),
            2,
            "both Tabs remain in history until delete"
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
        let store = ClosedTabsStore::in_memory();
        store
            .insert_group(
                &group("g1", GroupKind::Single, "w", 1),
                &[tab("p1", "w", "kept", 1)],
            )
            .unwrap();
        let (row_id, peeked) = store.peek_in_workspace("w", 0).expect("peeked");
        assert_eq!(peeked.tab_id, "p1");
        assert_eq!(
            store.list_for_workspace("w").len(),
            1,
            "peek leaves the Tab in history"
        );
        store.delete_tab_row(row_id).expect("row deleted");
        assert!(
            store.list_for_workspace("w").is_empty(),
            "delete_tab_row removes only after success"
        );
    }

    #[test]
    fn delete_reports_failure_instead_of_swallowing() {
        let store = ClosedTabsStore::in_memory();
        store
            .insert_group(
                &group("g1", GroupKind::Single, "w", 1),
                &[tab("p1", "w", "t", 1)],
            )
            .unwrap();
        let (row_id, _) = store.peek_in_workspace("w", 0).expect("peeked");

        // First delete removes the row and reports success.
        store
            .delete_tab_row(row_id)
            .expect("first delete succeeds");
        // Second delete of the same id removes nothing → Err (entry already gone).
        assert!(
            store.delete_tab_row(row_id).is_err(),
            "deleting an absent row must report failure, not silently 'succeed'"
        );
        // Deleting an absent group still succeeds (idempotent: nothing to retry).
        assert!(store.delete_group("nope").is_ok());
    }

    #[test]
    fn list_recent_carries_group_kind() {
        let store = ClosedTabsStore::in_memory();
        store
            .insert_group(
                &group("g1", GroupKind::Workspace, "w", 5),
                &[tab("p1", "w", "t", 5)],
            )
            .unwrap();
        let recent = store.list_recent(10);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].group_kind, GroupKind::Workspace);
        assert_eq!(recent[0].group_id, "g1");
    }

    #[test]
    fn backing_round_trips_through_sqlite() {
        let store = ClosedTabsStore::in_memory();
        let mut p = tab("p1", "w", "Chat", 1);
        p.backing = TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s7".into(),
        };
        store
            .insert_group(&group("g1", GroupKind::Single, "w", 1), &[p])
            .unwrap();
        let back = store.list_for_workspace("w");
        assert_eq!(
            back[0].backing,
            TabBacking::ChatBackend {
                plugin_id: "lmstudio".into(),
                session_id: "s7".into(),
            }
        );
    }

    #[test]
    fn v1_database_migrates_with_terminal_mode_default() {
        let db = temp_db();
        {
            std::fs::create_dir_all(db.parent().unwrap()).unwrap();
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE closed_group (
                    group_id TEXT PRIMARY KEY,
                    kind TEXT NOT NULL DEFAULT 'single',
                    label TEXT NOT NULL DEFAULT '',
                    workspace_id TEXT NOT NULL DEFAULT '',
                    workspace_name TEXT NOT NULL DEFAULT '',
                    workspace_cwd TEXT,
                    closed_at INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE closed_tab (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    group_id TEXT NOT NULL,
                    tab_id TEXT NOT NULL DEFAULT '',
                    parent_tab_id TEXT,
                    title TEXT NOT NULL DEFAULT '',
                    description TEXT,
                    backing_json TEXT NOT NULL DEFAULT '{"kind":"pty"}',
                    cwd TEXT,
                    agent_type TEXT,
                    agent_session_id TEXT,
                    model TEXT,
                    workspace_id TEXT NOT NULL DEFAULT '',
                    workspace_name TEXT NOT NULL DEFAULT '',
                    closed_at INTEGER NOT NULL DEFAULT 0
                );
                INSERT INTO closed_group (group_id, kind, label, workspace_id, workspace_name, closed_at)
                VALUES ('g1', 'single', 'old', 'w', 'WS', 1);
                INSERT INTO closed_tab (group_id, tab_id, title, backing_json, agent_type, workspace_id, workspace_name, closed_at)
                VALUES ('g1', 'p1', 'old tab', '{"kind":"pty"}', 'codex', 'w', 'WS', 1);
                PRAGMA user_version = 1;
                "#,
            )
            .unwrap();
        }

        let store = ClosedTabsStore::open_at(&db).unwrap();
        let list = store.list_for_workspace("w");
        assert_eq!(list.len(), 1);
        assert!(!list[0].chat_view);
        let conn = Connection::open(&db).unwrap();
        assert_eq!(
            ClosedTabsStore::user_version(&conn).unwrap(),
            CURRENT_CLOSED_SCHEMA_VERSION
        );
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }

    #[test]
    fn survives_drop_and_reopen() {
        let db = temp_db();
        {
            let store = ClosedTabsStore::open_at(&db).unwrap();
            store
                .insert_group(
                    &group("g1", GroupKind::Single, "w", 1),
                    &[tab("p1", "w", "kept", 1)],
                )
                .unwrap();
        } // dropped — simulates restart
        let reopened = ClosedTabsStore::open_at(&db).unwrap();
        let list = reopened.list_for_workspace("w");
        assert_eq!(list.len(), 1, "history persists across reopen");
        assert_eq!(list[0].title, "kept");
        let conn = Connection::open(&db).unwrap();
        assert_eq!(
            ClosedTabsStore::user_version(&conn).unwrap(),
            CURRENT_CLOSED_SCHEMA_VERSION
        );
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }

    #[test]
    fn global_group_cap_drops_oldest_groups() {
        let store = ClosedTabsStore::in_memory();
        for i in 0..(CLOSED_GROUP_CAP + 3) {
            let ws = format!("w{i}");
            store
                .insert_group(
                    &group(&format!("g{i}"), GroupKind::Workspace, &ws, i as u64),
                    &[tab(&format!("p{i}"), &ws, "t", i as u64)],
                )
                .unwrap();
        }
        let total: usize = store.list_recent(CLOSED_GROUP_CAP + 100).len();
        assert_eq!(total, CLOSED_GROUP_CAP, "global group cap bounds total");
        assert!(store.list_for_workspace("w0").is_empty());
    }

    #[test]
    fn reopening_existing_db_is_idempotent() {
        let db = temp_db();
        let s1 = ClosedTabsStore::open_at(&db).unwrap();
        s1.insert_group(
            &group("g1", GroupKind::Single, "w", 1),
            &[tab("p1", "w", "t", 1)],
        )
        .unwrap();
        drop(s1);
        let s2 = ClosedTabsStore::open_at(&db).unwrap();
        assert_eq!(s2.list_for_workspace("w").len(), 1);
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }
}

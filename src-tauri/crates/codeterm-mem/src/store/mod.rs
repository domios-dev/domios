//! mem_items SQLite schema + CRUD + blob store.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::MemStats;

pub(super) const INLINE_BODY_LIMIT: usize = 2048;

mod blob;
mod crud;
mod pinned;
mod query;
mod schema;

#[cfg(test)]
mod tests;

pub use crud::*;
pub use pinned::*;
pub use query::*;
pub use schema::init_schema;

// Use a thread-local actor label so callers don't pass `changed_by` on every mutation.
// REST handlers, workers, and dreaming wrap store calls in `with_changed_by`; default is `"user"`.

thread_local! {
    static CHANGED_BY: std::cell::RefCell<String> = std::cell::RefCell::new(String::from("user"));
}

/// Run `f` with the given actor label installed as the current thread-local
/// `changed_by`. The previous label is restored when the closure returns.
///
/// Callers:
/// - REST handlers pass `"user"` (default — no guard needed for plain user
///   requests, but a worker-bearing request overrides it).
/// - Extraction workers pass `"worker:extract"`.
/// - Dreaming passes `"dreaming:rem"`.
/// - Agents may pass `"agent:<id>"`.
pub fn with_changed_by<F, R>(label: &str, f: F) -> R
where
    F: FnOnce() -> R,
{
    let previous =
        CHANGED_BY.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), label.to_string()));
    let out = f();
    CHANGED_BY.with(|slot| *slot.borrow_mut() = previous);
    out
}

/// Read the current thread-local `changed_by` label.
pub fn current_changed_by() -> String {
    CHANGED_BY.with(|slot| slot.borrow().clone())
}

// Authorization identity, kept separate from the `changed_by` audit label: the
// label is free-form text, this is the exact worker id a commit is attributed to.
thread_local! {
    static WRITE_WORKER: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Run `f` with `worker_id` installed as the commit's origin, so the storage
/// boundary can resolve it to an extraction generation. `None` means the write
/// belongs to no tracked generation and is left unattributed.
pub fn with_write_worker<F, R>(worker_id: Option<&str>, f: F) -> R
where
    F: FnOnce() -> R,
{
    let previous = WRITE_WORKER
        .with(|slot| std::mem::replace(&mut *slot.borrow_mut(), worker_id.map(str::to_string)));
    let out = f();
    WRITE_WORKER.with(|slot| *slot.borrow_mut() = previous);
    out
}

/// Read the worker id the current commit is attributed to, if any.
pub fn current_write_worker() -> Option<String> {
    WRITE_WORKER.with(|slot| slot.borrow().clone())
}

/// One row from `mem_item_history`. All fields of `old_*` / `new_*` are
/// `None` when not applicable (e.g. `old_*` on create, `new_*` on delete).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub item_id: String,
    pub changed_at: String,
    pub changed_by: String,
    pub change_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_importance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_importance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Captured state before a mutation; see `history_diff`.
#[derive(Debug, Clone, Default)]
pub struct HistorySnapshot {
    pub title: Option<String>,
    pub body: Option<String>,
    pub importance: Option<f64>,
    pub kind: Option<String>,
}

/// Append a row to `mem_item_history`. Never panics — on error we log and
/// drop the row so history failures never abort the caller's real write.
pub fn history_log(
    conn: &Connection,
    item_id: &str,
    change_type: &str,
    old: &HistorySnapshot,
    new: &HistorySnapshot,
    reason: Option<&str>,
) {
    let changed_by = current_changed_by();
    let now = chrono::Utc::now().to_rfc3339();
    let res = conn.execute(
        r#"INSERT INTO mem_item_history
           (item_id, changed_at, changed_by, change_type,
            old_title, new_title, old_body, new_body,
            old_importance, new_importance, old_kind, new_kind, reason)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"#,
        params![
            item_id,
            &now,
            &changed_by,
            change_type,
            &old.title,
            &new.title,
            &old.body,
            &new.body,
            old.importance,
            new.importance,
            &old.kind,
            &new.kind,
            reason,
        ],
    );
    if let Err(e) = res {
        log::warn!("[mem] history_log failed for {}: {}", item_id, e);
    }
}

/// Return the full history for one item in chronological order.
pub fn get_item_history(conn: &Connection, item_id: &str) -> Result<Vec<HistoryEntry>, String> {
    let mut stmt = conn
        .prepare(
            r#"SELECT id, item_id, changed_at, changed_by, change_type,
                      old_title, new_title, old_body, new_body,
                      old_importance, new_importance, old_kind, new_kind, reason
               FROM mem_item_history
               WHERE item_id = ?1
               ORDER BY changed_at ASC, id ASC"#,
        )
        .map_err(|e| format!("prepare history: {}", e))?;
    let rows = stmt
        .query_map(params![item_id], |r| {
            Ok(HistoryEntry {
                id: r.get(0)?,
                item_id: r.get(1)?,
                changed_at: r.get(2)?,
                changed_by: r.get(3)?,
                change_type: r.get(4)?,
                old_title: r.get(5)?,
                new_title: r.get(6)?,
                old_body: r.get(7)?,
                new_body: r.get(8)?,
                old_importance: r.get(9)?,
                new_importance: r.get(10)?,
                old_kind: r.get(11)?,
                new_kind: r.get(12)?,
                reason: r.get(13)?,
            })
        })
        .map_err(|e| format!("query history: {}", e))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("row history: {}", e))?);
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemKind {
    Skill,
    Fact,
    Note,
    Conversation,
    Script,
    Peer,
    Preference,
    Daily,
    Learning,
    Rule,
    Summary,
    Document,
    Plan,
    PlanSection,
    Spec,
    SpecSection,
    Task,
    Stride,
}

impl MemKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Fact => "fact",
            Self::Note => "note",
            Self::Conversation => "conversation",
            Self::Script => "script",
            Self::Peer => "peer",
            Self::Preference => "preference",
            Self::Daily => "daily",
            Self::Learning => "learning",
            Self::Rule => "rule",
            Self::Summary => "summary",
            Self::Document => "document",
            Self::Plan => "plan",
            Self::PlanSection => "plan_section",
            Self::Spec => "spec",
            Self::SpecSection => "spec_section",
            Self::Task => "task",
            Self::Stride => "stride",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "skill" => Self::Skill,
            "fact" => Self::Fact,
            "note" => Self::Note,
            "conversation" => Self::Conversation,
            "script" => Self::Script,
            "peer" => Self::Peer,
            "preference" => Self::Preference,
            "daily" => Self::Daily,
            "learning" => Self::Learning,
            "rule" => Self::Rule,
            "summary" => Self::Summary,
            "document" => Self::Document,
            "plan" => Self::Plan,
            "plan_section" => Self::PlanSection,
            "spec" => Self::Spec,
            "spec_section" => Self::SpecSection,
            "task" => Self::Task,
            "stride" => Self::Stride,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemScope {
    Global,
    Workspace(String),
}

impl MemScope {
    pub fn as_str(&self) -> String {
        match self {
            Self::Global => "global".into(),
            Self::Workspace(id) => format!("workspace:{}", id),
        }
    }

    pub fn from_str(s: &str) -> Self {
        if let Some(rest) = s.strip_prefix("workspace:") {
            Self::Workspace(rest.to_string())
        } else {
            Self::Global
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMemItem {
    pub kind: MemKind,
    pub scope: MemScope,
    pub title: String,
    pub summary: Option<String>,
    pub area: Option<String>,
    pub tags: Vec<String>,
    pub body: String,
    pub source: Option<String>,
    pub importance: f64,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemItem {
    pub id: String,
    pub uid: String,
    pub kind: String,
    pub scope: String,
    pub title: String,
    pub summary: Option<String>,
    pub area: Option<String>,
    pub tags: Vec<String>,
    pub body: String,
    pub source: Option<String>,
    pub importance: f64,
    pub created_at: String,
    pub updated_at: String,
    pub accessed_at: Option<String>,
    #[serde(default)]
    pub access_count: i64,
    #[serde(default)]
    pub recurrence_count: i64,
}

/// One entry in `graph_pinned_positions` — user has anchored this node
/// so the force sim respects it and it survives reload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinnedPosition {
    pub mem_id: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Default)]
pub struct MemUpdate {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub area: Option<String>,
    pub tags: Option<Vec<String>>,
    pub body: Option<String>,
    pub importance: Option<f64>,
    pub bump_recurrence: Option<i64>,
    pub pinned: Option<bool>,
}

/// One row returned by [`tail_recent`]. Uses `mem_item_history.changed_by`
/// when a history row exists for the item; otherwise falls back to
/// `"user"` for items that predate the Phase C migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailEntry {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub area: Option<String>,
    pub updated_at: String,
    pub changed_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    pub importance: f64,
    pub tags: Vec<String>,
    pub has_vector: bool,
    pub has_body: bool,
    #[serde(default)]
    pub access_count: i64,
    #[serde(default)]
    pub recurrence_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub rel: String,
    pub confidence: f64,
    pub valid_from: String,
    pub valid_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphDump {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub stats: GraphDumpStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphDumpStats {
    pub node_count: usize,
    pub edge_count: usize,
}

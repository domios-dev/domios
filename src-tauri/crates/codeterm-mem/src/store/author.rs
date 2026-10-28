use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemAuthorKind {
    Owner,
    Neighbor,
    System,
}

impl MemAuthorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Neighbor => "neighbor",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemAuthor {
    pub kind: MemAuthorKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friendship_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl MemAuthor {
    pub fn owner(account_uid: Option<String>) -> Self {
        Self {
            kind: MemAuthorKind::Owner,
            account_uid,
            friendship_id: None,
            label: None,
        }
    }

    pub fn neighbor(account_uid: String, friendship_id: String) -> Self {
        Self {
            kind: MemAuthorKind::Neighbor,
            account_uid: Some(account_uid),
            friendship_id: Some(friendship_id),
            label: None,
        }
    }

    pub fn system(label: impl Into<String>) -> Self {
        Self {
            kind: MemAuthorKind::System,
            account_uid: None,
            friendship_id: None,
            label: Some(label.into()),
        }
    }

    pub fn is_neighbor(&self) -> bool {
        self.kind == MemAuthorKind::Neighbor
    }

    pub fn from_row(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<Self> {
        let kind: String = row.get(offset)?;
        let kind = match kind.as_str() {
            "owner" => MemAuthorKind::Owner,
            "neighbor" => MemAuthorKind::Neighbor,
            "system" => MemAuthorKind::System,
            _ => return Err(rusqlite::Error::InvalidQuery),
        };
        Ok(Self {
            kind,
            account_uid: row.get(offset + 1)?,
            friendship_id: row.get(offset + 2)?,
            label: row.get(offset + 3)?,
        })
    }
}

thread_local! {
    static AUTHOR: std::cell::RefCell<Option<MemAuthor>> = const { std::cell::RefCell::new(None) };
}

pub fn with_author<R>(author: MemAuthor, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<MemAuthor>);
    impl Drop for Restore {
        fn drop(&mut self) {
            AUTHOR.with(|s| *s.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(AUTHOR.with(|s| s.replace(Some(author))));
    f()
}

pub(crate) fn scoped_author() -> Option<MemAuthor> {
    AUTHOR.with(|s| s.borrow().clone())
}

pub fn current_author() -> MemAuthor {
    AUTHOR
        .with(|s| s.borrow().clone())
        .unwrap_or_else(|| MemAuthor::system("unscoped"))
}

pub fn item_author(conn: &Connection, id: &str) -> Result<Option<MemAuthor>, String> {
    conn.query_row(
        "SELECT author_kind,author_uid,author_friendship,author_label FROM mem_items WHERE id=?1",
        [id],
        |r| MemAuthor::from_row(r, 0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn guard_instruction(author: &MemAuthor, kind: &str, pinned: bool) -> Result<(), String> {
    if author.is_neighbor() && (pinned || matches!(kind, "rule" | "preference" | "skill")) {
        return Err("neighbor content cannot become instructions or be pinned".into());
    }
    Ok(())
}

pub fn refuse_neighbor_promotion(conn: &Connection, id: &str) -> Result<(), String> {
    if item_author(conn, id)?.is_some_and(|a| a.is_neighbor()) {
        return Err("neighbor content cannot be promoted".into());
    }
    Ok(())
}

impl Default for MemAuthor {
    fn default() -> Self {
        Self::system("unscoped")
    }
}

pub fn with_authored_change<R>(author: MemAuthor, label: &str, f: impl FnOnce() -> R) -> R {
    with_author(author, || super::with_changed_by(label, f))
}

pub fn with_system_author<R>(label: &str, f: impl FnOnce() -> R) -> R {
    with_authored_change(MemAuthor::system(label), label, f)
}

pub fn with_default_system_author<R>(label: &str, f: impl FnOnce() -> R) -> R {
    let author = AUTHOR
        .with(|slot| slot.borrow().clone())
        .unwrap_or_else(|| MemAuthor::system(label));
    with_author(author, f)
}

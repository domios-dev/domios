//! Which session held each closed agent tab's transcript. Kept apart from the
//! reopen history: reopening consumes `closed_tab`, and hidden agents never enter it.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::ClosedTabsStore;
use crate::multiplexer::types::{ContextEpochArchive, Tab, TabBacking};

/// Shortest tab id prefix a closed lookup accepts, matching the short ids frames show.
const MIN_TAB_PREFIX: usize = 8;

/// One closed agent tab: its authority edges and the session its transcript lives in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosedTranscript {
    pub tab_id: String,
    pub provider: String,
    pub session_id: String,
    pub cwd: String,
    #[serde(default)]
    pub chat_backend: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub closed_at: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archives: Vec<ContextEpochArchive>,
}

impl ClosedTranscript {
    /// `None` for tabs that never held an agent session, such as shells.
    pub(in crate::multiplexer::state::closed) fn from_tab(
        tab: &Tab,
        closed_at: u64,
    ) -> Option<Self> {
        let (provider, session_id, chat_backend) = match &tab.backing {
            TabBacking::ChatBackend {
                plugin_id,
                session_id,
            } => (
                plugin_id.clone(),
                if session_id.is_empty() {
                    tab.agent_session_id.clone().unwrap_or_default()
                } else {
                    session_id.clone()
                },
                true,
            ),
            TabBacking::Pty => (
                tab.agent_type.clone()?,
                tab.agent_session_id.clone().unwrap_or_default(),
                false,
            ),
            TabBacking::Surface(_) => return None,
        };
        if session_id.is_empty() && tab.context_epoch_archives.is_empty() {
            return None;
        }
        Some(Self {
            tab_id: tab.id.clone(),
            provider,
            session_id,
            cwd: tab.cwd.clone().unwrap_or_default(),
            chat_backend,
            parent_tab_id: tab.parent_tab_id.clone(),
            team_id: tab.team_id.clone(),
            title: tab.title.clone(),
            role: tab.role_profile.clone(),
            model: tab.agent_session_model_label().or_else(|| {
                tab.agent_provider_resolved_model
                    .as_deref()
                    .map(codeterm_types::pty::plain_model_id)
            }),
            closed_at,
            archives: tab.context_epoch_archives.clone(),
        })
    }

    /// The agent side of closed-read authority: the tab's parent or its Team.
    /// The owner reads every closed tab and is decided by the caller's transport.
    pub fn readable_by(&self, caller_tab: &str, caller_team: Option<&str>) -> bool {
        self.parent_tab_id.as_deref() == Some(caller_tab)
            || caller_team
                .is_some_and(|team| !team.is_empty() && self.team_id.as_deref() == Some(team))
    }
}

fn decode(payload: String) -> Result<ClosedTranscript, String> {
    serde_json::from_str(&payload).map_err(|error| format!("decode closed transcript: {error}"))
}

fn payloads(
    conn: &Connection,
    sql: &str,
    bind: impl rusqlite::Params,
) -> Result<Vec<ClosedTranscript>, String> {
    let mut statement = conn
        .prepare(sql)
        .map_err(|error| format!("prepare closed transcript read: {error}"))?;
    let rows = statement
        .query_map(bind, |row| row.get::<_, String>(0))
        .map_err(|error| format!("read closed transcripts: {error}"))?;
    rows.map(|row| {
        row.map_err(|error| format!("read closed transcript: {error}"))
            .and_then(decode)
    })
    .collect()
}

impl ClosedTabsStore {
    pub(super) fn init_transcripts(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS closed_transcript (
            tab_id TEXT PRIMARY KEY, session_id TEXT NOT NULL, parent_tab_id TEXT,
            closed_at INTEGER NOT NULL, payload TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS closed_transcript_recent
            ON closed_transcript(closed_at DESC);
        CREATE INDEX IF NOT EXISTS closed_transcript_session
            ON closed_transcript(session_id);
        CREATE INDEX IF NOT EXISTS closed_transcript_parent
            ON closed_transcript(parent_tab_id, closed_at DESC);",
        )
    }

    /// Record closes, then trim to the retention cap. Sessions no surviving row
    /// names are queued for [`Self::take_evicted_transcripts`].
    pub fn insert_transcripts(&self, records: &[ClosedTranscript]) -> Result<(), String> {
        if records.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock();
        let transaction = conn
            .transaction()
            .map_err(|error| format!("closed transcript tx: {error}"))?;
        for record in records {
            let payload = serde_json::to_string(record).map_err(|error| error.to_string())?;
            transaction
                .execute(
                    "INSERT INTO closed_transcript
                        (tab_id, session_id, parent_tab_id, closed_at, payload)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(tab_id) DO UPDATE SET
                        session_id = excluded.session_id,
                        parent_tab_id = excluded.parent_tab_id,
                        closed_at = excluded.closed_at,
                        payload = excluded.payload",
                    params![
                        record.tab_id,
                        record.session_id,
                        record.parent_tab_id,
                        record.closed_at as i64,
                        payload
                    ],
                )
                .map_err(|error| format!("insert closed transcript: {error}"))?;
        }
        let cap = self.group_cap as i64;
        let trimmed = payloads(
            &transaction,
            "SELECT payload FROM closed_transcript WHERE tab_id NOT IN (
                SELECT tab_id FROM closed_transcript
                ORDER BY closed_at DESC, rowid DESC LIMIT ?1
            )",
            params![cap],
        )?;
        transaction
            .execute(
                "DELETE FROM closed_transcript WHERE tab_id NOT IN (
                    SELECT tab_id FROM closed_transcript
                    ORDER BY closed_at DESC, rowid DESC LIMIT ?1
                )",
                params![cap],
            )
            .map_err(|error| format!("trim closed transcripts: {error}"))?;
        let mut evicted = Vec::new();
        for record in trimmed {
            let still_named: i64 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM closed_transcript WHERE session_id = ?1",
                    params![record.session_id],
                    |row| row.get(0),
                )
                .map_err(|error| format!("count closed transcript session: {error}"))?;
            if still_named == 0 {
                evicted.push(record);
            }
        }
        transaction
            .commit()
            .map_err(|error| format!("closed transcript commit: {error}"))?;
        drop(conn);
        self.evicted_transcripts.lock().extend(evicted);
        Ok(())
    }

    /// Exact tab id, or a unique short-id prefix of one.
    pub fn transcript_by_tab(&self, key: &str) -> Result<Option<ClosedTranscript>, String> {
        let conn = self.conn.lock();
        let exact: Option<String> = conn
            .query_row(
                "SELECT payload FROM closed_transcript WHERE tab_id = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("read closed transcript: {error}"))?;
        if let Some(payload) = exact {
            return decode(payload).map(Some);
        }
        if key.len() < MIN_TAB_PREFIX {
            return Ok(None);
        }
        let mut matches = payloads(
            &conn,
            "SELECT payload FROM closed_transcript
             WHERE substr(tab_id, 1, length(?1)) = ?1 LIMIT 2",
            params![key],
        )?;
        Ok(if matches.len() == 1 {
            matches.pop()
        } else {
            None
        })
    }

    /// The tab that most recently closed holding this session.
    pub fn transcript_by_session(&self, session: &str) -> Result<Option<ClosedTranscript>, String> {
        if session.is_empty() {
            return Ok(None);
        }
        let conn = self.conn.lock();
        Ok(payloads(
            &conn,
            "SELECT payload FROM closed_transcript WHERE session_id = ?1
             ORDER BY closed_at DESC, rowid DESC LIMIT 1",
            params![session],
        )?
        .pop())
    }

    /// Newest-first closed children of one parent tab.
    pub fn closed_children(
        &self,
        parent: &str,
        limit: usize,
    ) -> Result<Vec<ClosedTranscript>, String> {
        let conn = self.conn.lock();
        payloads(
            &conn,
            "SELECT payload FROM closed_transcript WHERE parent_tab_id = ?1
             ORDER BY closed_at DESC, rowid DESC LIMIT ?2",
            params![parent, limit as i64],
        )
    }

    /// Sessions retention just dropped, for the owner of their transcript files.
    pub fn take_evicted_transcripts(&self) -> Vec<ClosedTranscript> {
        std::mem::take(&mut *self.evicted_transcripts.lock())
    }
}

/// Closed-transcript reads that also see closes still waiting on their batch.
pub struct ClosedTranscriptReader {
    pub(in crate::multiplexer::state::closed) store: ClosedTabsStore,
    pub(in crate::multiplexer::state::closed) pending: Vec<ClosedTranscript>,
}

impl ClosedTranscriptReader {
    pub fn by_tab(&self, key: &str) -> Result<Option<ClosedTranscript>, String> {
        if let Some(record) = self.pending.iter().find(|record| record.tab_id == key) {
            return Ok(Some(record.clone()));
        }
        self.store.transcript_by_tab(key)
    }

    pub fn by_session(&self, session: &str) -> Result<Option<ClosedTranscript>, String> {
        if let Some(record) = self
            .pending
            .iter()
            .find(|record| !session.is_empty() && record.session_id == session)
        {
            return Ok(Some(record.clone()));
        }
        self.store.transcript_by_session(session)
    }

    pub fn children(&self, parent: &str, limit: usize) -> Result<Vec<ClosedTranscript>, String> {
        let mut rows: Vec<ClosedTranscript> = self
            .pending
            .iter()
            .filter(|record| record.parent_tab_id.as_deref() == Some(parent))
            .cloned()
            .collect();
        for row in self.store.closed_children(parent, limit)? {
            if !rows.iter().any(|known| known.tab_id == row.tab_id) {
                rows.push(row);
            }
        }
        rows.truncate(limit);
        Ok(rows)
    }
}

#[cfg(test)]
mod tests;

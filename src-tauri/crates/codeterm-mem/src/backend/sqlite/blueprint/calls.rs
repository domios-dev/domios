use super::*;
use codeterm_blueprint_types::blueprint_document::calls::{self, CallPin, MemoryResolver};
use rusqlite::OptionalExtension;
use std::path::Path;

pub(crate) fn initialize(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS blueprint_call_scopes (item_id TEXT PRIMARY KEY, area TEXT NOT NULL, team TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS blueprint_call_revisions (item_id TEXT NOT NULL, hash TEXT NOT NULL, area TEXT NOT NULL, team TEXT NOT NULL, document TEXT NOT NULL, PRIMARY KEY(item_id, hash, area, team));").map_err(|e|e.to_string())
}
pub(crate) struct ScopedResolver<'a> {
    pub conn: &'a Connection,
    pub blobs: &'a Path,
    pub area: &'a str,
    pub team: &'a str,
}
impl ScopedResolver<'_> {
    fn id<'a>(&self, id: &'a str) -> Result<&'a str, String> {
        let id = id
            .strip_prefix("mem:")
            .ok_or("Expected mem: solution identity")?;
        let exists:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM blueprint_call_scopes s JOIN mem_items m ON m.id=s.item_id WHERE s.item_id=?1 AND s.area=?2 AND s.team=?3 AND m.area=s.area AND m.source=?4)",params![id,self.area,self.team,SOURCE],|r|r.get(0)).map_err(|e|e.to_string())?;
        if !exists {
            return Err("Callee missing or outside caller Team/workspace".into());
        }
        Ok(id)
    }
}
impl MemoryResolver for ScopedResolver<'_> {
    fn retain(&self, callee: &calls::FrozenCallee) -> Result<(), String> {
        let id = if callee.id.starts_with("catalog:") {
            crate::blueprint_library::catalog::document(&callee.id)?;
            callee.id.as_str()
        } else if callee.id.starts_with("mem:") {
            self.id(&callee.id)?
        } else {
            return Ok(());
        };
        callee.verify()?;
        let body = serde_json::to_string(&callee.document).map_err(|e| e.to_string())?;
        self.conn.execute("INSERT INTO blueprint_call_revisions(item_id,hash,area,team,document) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(item_id,hash,area,team) DO NOTHING",params![id,callee.hash,self.area,self.team,body]).map_err(|e|e.to_string())?;
        let prior:String=self.conn.query_row("SELECT document FROM blueprint_call_revisions WHERE item_id=?1 AND hash=?2 AND area=?3 AND team=?4",params![id,callee.hash,self.area,self.team],|r|r.get(0)).map_err(|e|e.to_string())?;
        if !same_revision(&prior, &body) {
            return Err("Immutable revision collision or corruption".into());
        }
        Ok(())
    }
    fn head(&self, id: &str) -> Result<Document, String> {
        if id.starts_with("catalog:") {
            return crate::blueprint_library::catalog::document(id);
        }
        let id = self.id(id)?;
        let item = store::get(self.conn, id, self.blobs)?.ok_or("Missing callee")?;
        if item.kind == "archived" {
            return Err("Archived callee cannot be freshly pinned; existing immutable pins remain available".into());
        }
        model::parse_document(&item.body)
    }
    fn head_for_drift(&self, id: &str) -> Result<(Document, Option<String>), String> {
        if id.starts_with("catalog:") {
            return self.head(id).map(|document| (document, None));
        }
        let raw_id = self.id(id)?;
        let item = store::get(self.conn, raw_id, self.blobs)?.ok_or("Missing callee")?;
        let document = model::parse_document(&item.body)?;
        if item.kind != "archived" {
            return Ok((document, None));
        }
        let reason: Option<String> = self.conn.query_row(
            "SELECT reason FROM mem_item_history WHERE item_id=?1 AND new_kind='archived' ORDER BY changed_at DESC, id DESC LIMIT 1",
            [raw_id], |row| row.get(0),
        ).optional().map_err(|e| e.to_string())?.flatten();
        let reason = reason.unwrap_or_else(|| "callee archived".into());
        if let Some(keeper) = reason.strip_prefix("blueprint solution merged into ") {
            let keeper_ref = format!("mem:{keeper}");
            if let Ok(keeper_id) = self.id(&keeper_ref) {
                if let Some(kept) = store::get(self.conn, keeper_id, self.blobs)? {
                    return Ok((model::parse_document(&kept.body)?, Some(reason)));
                }
            }
        }
        Ok((document, Some(reason)))
    }
    fn revision(&self, pin: &CallPin) -> Result<Document, String> {
        let id = if pin.id.starts_with("catalog:") {
            crate::blueprint_library::catalog::document(&pin.id)?;
            pin.id.as_str()
        } else {
            self.id(&pin.id)?
        };
        let raw:Option<String>=self.conn.query_row("SELECT document FROM blueprint_call_revisions WHERE item_id=?1 AND hash=?2 AND area=?3 AND team=?4",params![id,pin.hash,self.area,self.team],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        let document: Document = serde_json::from_str(
            &raw.ok_or_else(|| format!("Missing pinned hash {}@{}", pin.id, pin.hash))?,
        )
        .map_err(|e| e.to_string())?;
        if !codeterm_blueprint_types::blueprint_document::projection::matches_identity(
            &document, &pin.hash,
        )? {
            return Err("Pinned revision bytes are corrupted".into());
        }
        Ok(document)
    }
}
pub(crate) fn scope(conn: &Connection, id: &str, area: &str, team: &str) -> Result<(), String> {
    conn.execute("INSERT INTO blueprint_call_scopes(item_id,area,team) VALUES(?1,?2,?3) ON CONFLICT(item_id) DO NOTHING",params![id,area,team]).map_err(|e|e.to_string())?;
    let actual: (String, String) = conn
        .query_row(
            "SELECT area,team FROM blueprint_call_scopes WHERE item_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    if actual != (area.into(), team.into()) {
        return Err("Cannot change a saved solution's Team/workspace".into());
    }
    Ok(())
}
pub(crate) fn same_scope(
    conn: &Connection,
    id: &str,
    area: &str,
    team: Option<&str>,
) -> Result<bool, String> {
    let scope: Option<(String, String)> = conn
        .query_row(
            "SELECT area,team FROM blueprint_call_scopes WHERE item_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(match (scope, team) {
        (Some((a, t)), Some(team)) => a == area && t == team,
        (None, None) => true,
        _ => false,
    })
}
pub(crate) fn merge_allowed(conn: &Connection, first: &str, second: &str) -> Result<bool, String> {
    let scope = |id: &str| -> Result<Option<(String, String)>, String> {
        conn.query_row(
            "SELECT area,team FROM blueprint_call_scopes WHERE item_id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())
    };
    Ok(scope(first)? == scope(second)?)
}
pub(crate) fn retain(conn: &Connection, id: &str, document: &Document) -> Result<(), String> {
    let scoped: Option<(String, String)> = conn
        .query_row(
            "SELECT area,team FROM blueprint_call_scopes WHERE item_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((area, team)) = scoped else {
        return Ok(());
    };
    let mut body = document.clone();
    body.dependencies.clear();
    let hash = codeterm_blueprint_types::blueprint_document::projection::identity(&body)?;
    let body = serde_json::to_string(&body).map_err(|e| e.to_string())?;
    conn.execute("INSERT INTO blueprint_call_revisions(item_id,hash,area,team,document) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(item_id,hash,area,team) DO NOTHING",params![id,hash,area,team,body]).map_err(|e|e.to_string())?;
    let stored:String=conn.query_row("SELECT document FROM blueprint_call_revisions WHERE item_id=?1 AND hash=?2 AND area=?3 AND team=?4",params![id,hash,area,team],|r|r.get(0)).map_err(|e|e.to_string())?;
    if !same_revision(&stored, &body) {
        return Err("Immutable revision collision or corruption".into());
    }
    Ok(())
}
// macOS builds keep object key order (serde_json preserve_order), so equal revisions may differ in raw bytes.
fn same_revision(stored: &str, body: &str) -> bool {
    let sorted = |text: &str| {
        serde_json::from_str::<serde_json::Value>(text).map(|mut value| {
            value.sort_all_objects();
            value
        })
    };
    stored == body || matches!((sorted(stored), sorted(body)), (Ok(a), Ok(b)) if a == b)
}

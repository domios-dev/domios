use super::*;
use codeterm_blueprint_types::blueprint_document::calls::{
    self, CallPin, FrozenClosure, MemoryResolver,
};
use rusqlite::OptionalExtension;
use std::path::Path;

pub(crate) fn initialize(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS blueprint_call_scopes (item_id TEXT PRIMARY KEY, area TEXT NOT NULL, team TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS blueprint_call_revisions (item_id TEXT NOT NULL, hash TEXT NOT NULL, area TEXT NOT NULL, team TEXT NOT NULL, document TEXT NOT NULL, PRIMARY KEY(item_id, hash, area, team));").map_err(|e|e.to_string())
}
pub(super) struct ScopedResolver<'a> {
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
            super::catalog::document(&callee.id)?;
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
        if prior != body {
            return Err("Immutable revision collision or corruption".into());
        }
        Ok(())
    }
    fn head(&self, id: &str) -> Result<Document, String> {
        if id.starts_with("catalog:") {
            return super::catalog::document(id);
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
            super::catalog::document(&pin.id)?;
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
pub(super) fn scope(conn: &Connection, id: &str, area: &str, team: &str) -> Result<(), String> {
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
pub(super) fn same_scope(
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
pub(super) fn merge_allowed(conn: &Connection, first: &str, second: &str) -> Result<bool, String> {
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
pub(super) fn retain(conn: &Connection, id: &str, document: &Document) -> Result<(), String> {
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
    if stored != body {
        return Err("Immutable revision collision or corruption".into());
    }
    Ok(())
}
pub(super) fn freeze(
    conn: &Connection,
    blobs: &Path,
    area: &str,
    team: Option<&str>,
    document: Document,
) -> Result<Document, String> {
    let has_calls = document.nodes().any(|node| node.call.is_some());
    if !has_calls && document.dependencies.is_empty() {
        return Ok(document);
    }
    let team = team.ok_or("Call pins require a caller Team/workspace")?;
    let resolver = ScopedResolver {
        conn,
        blobs,
        area,
        team,
    };
    let closure = calls::resolve_saved(document, &resolver).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.message)
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let context = calls::check_context(&closure, &Default::default());
    let diagnostics = codeterm_blueprint_types::blueprint_document::checker::check_ir(
        &closure.root.ir(),
        &context,
    );
    if codeterm_blueprint_types::blueprint_document::checker::has_errors(&diagnostics) {
        return Err(
            codeterm_blueprint_types::blueprint_document::checker::render(&diagnostics, true),
        );
    }
    Ok(closure.document())
}
impl MemManager {
    pub fn save_blueprint_scoped(
        &self,
        area: &str,
        team: &str,
        document: Document,
        intents: &[String],
        evidence: Evidence,
    ) -> Result<SaveResult, String> {
        let mut prepared = self.qualify_blueprint(area, document, intents, evidence)?;
        prepared.team = Some(team.into());
        let saved = self.persist_blueprint(area, prepared, true, AREA_CAP)?;
        self.index_after_save(&saved.id);
        Ok(saved)
    }
    pub fn enqueue_blueprint_scoped(
        self: &std::sync::Arc<Self>,
        area: String,
        team: String,
        definition: serde_json::Value,
        intents: Vec<String>,
        evidence: Evidence,
    ) -> Result<SaveResult, String> {
        self.guard_commit()?;
        let mut prepared = self.qualify_blueprint(
            &area,
            Document::from_definition(&definition)?,
            &intents,
            evidence,
        )?;
        prepared.team = Some(team);
        let (reply, completion) = std::sync::mpsc::sync_channel(1);
        self.queue_blueprint(Pending::Keep {
            area,
            prepared,
            reply,
        })?;
        completion
            .recv()
            .map_err(|_| "Blueprint memory worker stopped before save completion")?
    }
    pub fn pin_blueprint_calls(
        &self,
        area: &str,
        team: &str,
        document: Document,
    ) -> Result<Document, String> {
        self.guard_commit()?;
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let document = freeze(&tx, &self.blobs_dir, area, Some(team), document)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(document)
    }
    pub fn pin_blueprint_bundle(
        &self,
        area: &str,
        team: &str,
        document: Document,
        local_callees: &[calls::LocalCallee],
    ) -> Result<Document, String> {
        self.guard_commit()?;
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let resolver = ScopedResolver {
            conn: &tx,
            blobs: &self.blobs_dir,
            area,
            team,
        };
        let closure =
            calls::resolve_bundle(document, local_callees, &resolver).map_err(|errors| {
                errors
                    .into_iter()
                    .map(|e| format!("{}:{}:{}: {}", e.file, e.line, e.column, e.message))
                    .collect::<Vec<_>>()
                    .join("; ")
            })?;
        let document = closure.document();
        tx.commit().map_err(|e| e.to_string())?;
        Ok(document)
    }
    pub fn blueprint_call_drift(
        &self,
        area: &str,
        team: &str,
        document: Document,
    ) -> Result<Vec<calls::Drift>, String> {
        let closure = FrozenClosure::from_document(document)?;
        let conn = self.conn.lock();
        closure.drift(&ScopedResolver {
            conn: &conn,
            blobs: &self.blobs_dir,
            area,
            team,
        })
    }
    pub fn blueprint_callable(&self, area: &str, team: &str, id: &str) -> Result<Document, String> {
        let conn = self.conn.lock();
        ScopedResolver {
            conn: &conn,
            blobs: &self.blobs_dir,
            area,
            team,
        }
        .head(id)
    }
    pub fn blueprint_call_revision(
        &self,
        area: &str,
        team: &str,
        pin: &CallPin,
    ) -> Result<Document, String> {
        let conn = self.conn.lock();
        ScopedResolver {
            conn: &conn,
            blobs: &self.blobs_dir,
            area,
            team,
        }
        .revision(pin)
    }
}
#[cfg(test)]
mod tests;

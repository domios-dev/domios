use super::SOURCE;
use crate::{db_error, items};
use codeterm_blueprint_types::{
    blueprint_document::{
        calls::{CallPin, FrozenCallee, MemoryResolver},
        projection, Document,
    },
    blueprint_memory::parse_document,
};
use codeterm_mem::blueprint_library::catalog;
use postgres::Transaction;
use std::cell::RefCell;

pub(super) struct ScopedResolver<'a, 'b> {
    tx: RefCell<&'a mut Transaction<'b>>,
    area: &'a str,
    team: &'a str,
}

impl<'a, 'b> ScopedResolver<'a, 'b> {
    pub(super) fn new(tx: &'a mut Transaction<'b>, area: &'a str, team: &'a str) -> Self {
        Self {
            tx: RefCell::new(tx),
            area,
            team,
        }
    }

    fn id<'c>(&self, id: &'c str) -> Result<&'c str, String> {
        let id = id
            .strip_prefix("mem:")
            .ok_or("Expected mem: solution identity")?;
        let exists: bool=self.tx.borrow_mut().query_one("SELECT EXISTS(SELECT 1 FROM blueprint_call_scopes s JOIN mem_items m ON m.id=s.item_id WHERE s.item_id=$1 AND s.area=$2 AND s.team=$3 AND m.area=s.area AND m.source=$4)", &[&id,&self.area,&self.team,&SOURCE]).map_err(db_error)?.get(0);
        if !exists {
            return Err("Callee missing or outside caller Team/workspace".into());
        }
        Ok(id)
    }
}

impl MemoryResolver for ScopedResolver<'_, '_> {
    fn retain(&self, callee: &FrozenCallee) -> Result<(), String> {
        let id = if callee.id.starts_with("catalog:") {
            catalog::document(&callee.id)?;
            callee.id.as_str()
        } else if callee.id.starts_with("mem:") {
            self.id(&callee.id)?
        } else {
            return Ok(());
        };
        callee.verify()?;
        revision_insert(
            &mut self.tx.borrow_mut(),
            id,
            &callee.hash,
            self.area,
            self.team,
            &callee.document,
        )
    }

    fn head(&self, id: &str) -> Result<Document, String> {
        if id.starts_with("catalog:") {
            return catalog::document(id);
        }
        let id = self.id(id)?;
        let item = items::get_in(&mut self.tx.borrow_mut(), id)?.ok_or("Missing callee")?;
        if item.kind == "archived" {
            return Err("Archived callee cannot be freshly pinned; existing immutable pins remain available".into());
        }
        parse_document(&item.body)
    }

    fn revision(&self, pin: &CallPin) -> Result<Document, String> {
        let id = if pin.id.starts_with("catalog:") {
            catalog::document(&pin.id)?;
            pin.id.as_str()
        } else {
            self.id(&pin.id)?
        };
        let raw=self.tx.borrow_mut().query_opt("SELECT document FROM blueprint_call_revisions WHERE item_id=$1 AND hash=$2 AND area=$3 AND team=$4", &[&id,&pin.hash,&self.area,&self.team]).map_err(db_error)?
            .ok_or_else(||format!("Missing pinned hash {}@{}",pin.id,pin.hash))?;
        let document: Document =
            serde_json::from_str(&raw.get::<_, String>(0)).map_err(|error| error.to_string())?;
        if !projection::matches_identity(&document, &pin.hash)? {
            return Err("Pinned revision bytes are corrupted".into());
        }
        Ok(document)
    }

    fn head_for_drift(&self, id: &str) -> Result<(Document, Option<String>), String> {
        if id.starts_with("catalog:") {
            return self.head(id).map(|document| (document, None));
        }
        let raw_id = self.id(id)?;
        let item = items::get_in(&mut self.tx.borrow_mut(), raw_id)?.ok_or("Missing callee")?;
        let document = parse_document(&item.body)?;
        if item.kind != "archived" {
            return Ok((document, None));
        }
        let reason=self.tx.borrow_mut().query_opt("SELECT reason FROM mem_item_history WHERE item_id=$1 AND new_kind='archived' ORDER BY changed_at DESC,id DESC LIMIT 1", &[&raw_id]).map_err(db_error)?
            .and_then(|row|row.get::<_,Option<String>>(0)).unwrap_or_else(||"callee archived".into());
        if let Some(keeper) = reason.strip_prefix("blueprint solution merged into ") {
            let keeper_ref = format!("mem:{keeper}");
            if let Ok(keeper_id) = self.id(&keeper_ref) {
                if let Some(kept) = items::get_in(&mut self.tx.borrow_mut(), keeper_id)? {
                    return Ok((parse_document(&kept.body)?, Some(reason)));
                }
            }
        }
        Ok((document, Some(reason)))
    }
}

fn revision_insert(
    tx: &mut Transaction<'_>,
    id: &str,
    hash: &str,
    area: &str,
    team: &str,
    document: &Document,
) -> Result<(), String> {
    let body = serde_json::to_string(document).map_err(|error| error.to_string())?;
    tx.execute("INSERT INTO blueprint_call_revisions(item_id,hash,area,team,document) VALUES ($1,$2,$3,$4,$5) ON CONFLICT(item_id,hash,area,team) DO NOTHING", &[&id,&hash,&area,&team,&body]).map_err(db_error)?;
    let stored: String=tx.query_one("SELECT document FROM blueprint_call_revisions WHERE item_id=$1 AND hash=$2 AND area=$3 AND team=$4", &[&id,&hash,&area,&team]).map_err(db_error)?.get(0);
    let stored: serde_json::Value = serde_json::from_str(&stored)
        .map_err(|_| "Immutable revision collision or corruption")?;
    let expected = serde_json::to_value(document).map_err(|error| error.to_string())?;
    if stored != expected {
        return Err("Immutable revision collision or corruption".into());
    }
    Ok(())
}

pub(super) fn scope(
    tx: &mut Transaction<'_>,
    id: &str,
    area: &str,
    team: &str,
) -> Result<(), String> {
    tx.execute("INSERT INTO blueprint_call_scopes(item_id,area,team) VALUES ($1,$2,$3) ON CONFLICT(item_id) DO NOTHING", &[&id,&area,&team]).map_err(db_error)?;
    if read_scope(tx, id)? != Some((area.into(), team.into())) {
        return Err("Cannot change a saved solution's Team/workspace".into());
    }
    Ok(())
}

fn read_scope(tx: &mut Transaction<'_>, id: &str) -> Result<Option<(String, String)>, String> {
    Ok(tx
        .query_opt(
            "SELECT area,team FROM blueprint_call_scopes WHERE item_id=$1",
            &[&id],
        )
        .map_err(db_error)?
        .map(|row| (row.get(0), row.get(1))))
}

pub(super) fn same_scope(
    tx: &mut Transaction<'_>,
    id: &str,
    area: &str,
    team: Option<&str>,
) -> Result<bool, String> {
    Ok(match (read_scope(tx, id)?, team) {
        (Some((a, t)), Some(team)) => a == area && t == team,
        (None, None) => true,
        _ => false,
    })
}

pub(super) fn merge_allowed(
    tx: &mut Transaction<'_>,
    first: &str,
    second: &str,
) -> Result<bool, String> {
    Ok(read_scope(tx, first)? == read_scope(tx, second)?)
}

pub(super) fn retain(
    tx: &mut Transaction<'_>,
    id: &str,
    document: &Document,
) -> Result<(), String> {
    let Some((area, team)) = read_scope(tx, id)? else {
        return Ok(());
    };
    let mut document = document.clone();
    document.dependencies.clear();
    let hash = projection::identity(&document)?;
    revision_insert(tx, id, &hash, &area, &team, &document)
}

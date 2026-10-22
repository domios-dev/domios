use crate::error::{sqlite_error, StorageError};
use crate::repositories::sentinel::records::NestedClaimParent;
use codeterm_blueprint_types::blueprint_document::calls::CallFrame;
use rusqlite::{params, Connection};

struct ActiveClaim {
    run: String,
    marker: i64,
    generation: Option<i64>,
    parent: Option<i64>,
    frames: Option<String>,
}
pub(super) fn admissible(
    conn: &Connection,
    id: &str,
    run: &str,
    generation: u64,
    marker: i64,
    parent: Option<&NestedClaimParent>,
) -> Result<bool, StorageError> {
    let duplicate:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM blueprint_step_journal WHERE blueprint_id=?1 AND run_id=?2 AND step_index=?3)",params![id,run,marker],|r|r.get(0)).map_err(|e|sqlite_error("inspect claim replay",e))?;
    if duplicate {
        return Ok(false);
    }
    let mut statement=conn.prepare("SELECT claim.run_id,claim.step_index,claim.claim_generation,claim.claim_parent_index,claim.call_stack_json FROM blueprint_step_journal AS claim WHERE claim.blueprint_id=?1 AND claim.state='running' AND NOT EXISTS (SELECT 1 FROM blueprint_step_journal AS resolved WHERE resolved.blueprint_id=claim.blueprint_id AND resolved.run_id=claim.run_id AND (resolved.step_index=claim.step_index OR (resolved.step_index=-1 AND resolved.step_name='$run')) AND resolved.entry_id>claim.entry_id AND resolved.state IN ('done','failed','skipped','held')) ORDER BY claim.entry_id DESC").map_err(|e|sqlite_error("inspect active claim ancestry",e))?;
    let active = statement
        .query_map([id], |r| {
            Ok(ActiveClaim {
                run: r.get(0)?,
                marker: r.get(1)?,
                generation: r.get(2)?,
                parent: r.get(3)?,
                frames: r.get(4)?,
            })
        })
        .map_err(|e| sqlite_error("query active claim ancestry", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| sqlite_error("read active claim ancestry", e))?;
    let Some(parent) = parent else {
        return Ok(active.is_empty());
    };
    if active.is_empty() || active.len() > 8 {
        return Ok(false);
    }
    let Ok(generation) = i64::try_from(generation) else {
        return Ok(false);
    };
    if active
        .iter()
        .any(|a| a.run != run || a.generation != Some(generation))
    {
        return Ok(false);
    }
    let Some(leaf) = active.iter().find(|a| a.marker == parent.marker.index()) else {
        return Ok(false);
    };
    let frames: Vec<CallFrame> = match leaf
        .frames
        .as_ref()
        .and_then(|raw| serde_json::from_str(raw).ok())
    {
        Some(frames) => frames,
        None => return Ok(false),
    };
    if frames.is_empty() || frames.len() > 8 || frames.last() != Some(&parent.frame) {
        return Ok(false);
    }
    let mut cursor = Some(parent.marker.index());
    let mut visited = std::collections::BTreeSet::new();
    while let Some(marker) = cursor {
        if !visited.insert(marker) || visited.len() > 8 {
            return Ok(false);
        }
        let Some(claim) = active.iter().find(|a| a.marker == marker) else {
            return Ok(false);
        };
        cursor = claim.parent;
    }
    Ok(visited.len() == active.len())
}

#[cfg(test)]
mod tests;

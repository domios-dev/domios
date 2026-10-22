use super::*;

pub(super) fn queue_changed(root: &Path, changes: Result<Vec<Unit>, String>) -> Result<(), String> {
    for unit in changes? {
        index_queue().enqueue(IndexJob::Embed(root.to_path_buf(), unit));
    }
    Ok(())
}

pub(super) fn embed_current(root: &Path, unit: &Unit) -> Result<(), String> {
    embed_current_with(root, unit, |conn, unit| {
        port().embed(conn, &unit.id, &unit.body)
    })
}

fn embed_current_with(
    root: &Path,
    unit: &Unit,
    embed: impl FnOnce(&Connection, &Unit) -> Result<(), String>,
) -> Result<(), String> {
    let conn = open_index(root)?;
    let current = indexed_units_like(&conn, &unit.id)?;
    if current.get(&unit.id) != Some(&unit_signature(unit)) {
        return Ok(());
    }
    embed(&conn, unit).map_err(|error| format!("embedding {}: {error}", unit.id))
}

#[cfg(test)]
mod tests;

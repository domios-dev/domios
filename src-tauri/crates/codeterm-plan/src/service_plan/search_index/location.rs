use super::*;

pub(in crate::service_plan) fn enqueue_plan_move(
    _turn: &MutationWriterTurn<'_>,
    id: &str,
    old: PathBuf,
    new: PathBuf,
) {
    if project_index_path(&old) != project_index_path(&new) {
        index_queue().enqueue(IndexJob::MovePlan {
            id: id.into(),
            old,
            new,
        });
    }
}

pub(super) fn move_plan_units(id: &str, old: &Path, new: &Path) -> Result<(), String> {
    let old_path = project_index_path(old);
    if !old_path.is_file() || old_path == project_index_path(new) {
        return Ok(());
    }
    drop(open_index(old)?);
    let mut connection = open_index(new)?;
    connection
        .execute(
            "ATTACH DATABASE ?1 AS previous_plan_index",
            [old_path.display().to_string()],
        )
        .map_err(|error| error.to_string())?;
    let prefix = format!("planunit_{id}_%");
    let tx = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    delete_units_tx(&tx, &prefix)?;
    tx.execute(
        "INSERT INTO mem_items SELECT * FROM previous_plan_index.mem_items WHERE id LIKE ?1",
        [&prefix],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "UPDATE mem_items SET scope=?2 WHERE id LIKE ?1",
        params![prefix, project_scope(new)],
    )
    .map_err(|error| error.to_string())?;
    tx.execute("INSERT INTO mem_fts(mem_id,title,summary,body,tags) SELECT mem_id,title,summary,body,tags FROM previous_plan_index.mem_fts WHERE mem_id LIKE ?1",[&prefix]).map_err(|error|error.to_string())?;
    let vectors:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='mem_vectors') AND EXISTS(SELECT 1 FROM previous_plan_index.sqlite_master WHERE name='mem_vectors')",[],|row|row.get(0)).map_err(|error|error.to_string())?;
    if vectors {
        tx.execute("INSERT INTO mem_vectors SELECT * FROM previous_plan_index.mem_vectors WHERE mem_id LIKE ?1",[&prefix]).map_err(|error|error.to_string())?;
        tx.execute(
            "DELETE FROM previous_plan_index.mem_vectors WHERE mem_id LIKE ?1",
            [&prefix],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.execute(
        "DELETE FROM previous_plan_index.mem_fts WHERE mem_id LIKE ?1",
        [&prefix],
    )
    .map_err(|error| error.to_string())?;
    tx.execute(
        "DELETE FROM previous_plan_index.mem_items WHERE id LIKE ?1",
        [&prefix],
    )
    .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;

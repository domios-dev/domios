use crate::db_error;
use crate::executor::Transaction;
use codeterm_mem::solution_metadata::SolutionMetadata;

pub(super) fn read(tx: &mut Transaction<'_>, id: &str) -> Result<Option<SolutionMetadata>, String> {
    tx.query_opt(
        "SELECT metadata_json FROM solution_metadata WHERE item_id=$1",
        &[&id],
    )
    .map_err(db_error)?
    .map(|row| {
        serde_json::from_str(&row.get::<_, String>(0))
            .map_err(|error| format!("invalid solution metadata for {id}: {error}"))
    })
    .transpose()
}

pub(super) fn write(
    tx: &mut Transaction<'_>,
    id: &str,
    record: &SolutionMetadata,
) -> Result<(), String> {
    let json = serde_json::to_string(record).map_err(|error| error.to_string())?;
    tx.execute("INSERT INTO solution_metadata(item_id,metadata_json) VALUES ($1,$2) ON CONFLICT(item_id) DO UPDATE SET metadata_json=excluded.metadata_json", &[&id,&json]).map_err(db_error)?;
    Ok(())
}

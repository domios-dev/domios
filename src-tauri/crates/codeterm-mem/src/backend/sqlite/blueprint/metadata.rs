use crate::{solution_metadata::SolutionMetadata, store};
use rusqlite::{params, Connection, OptionalExtension};
pub(crate) fn initialize(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS solution_metadata (item_id TEXT PRIMARY KEY REFERENCES mem_items(id) ON DELETE CASCADE, metadata_json TEXT NOT NULL);").map_err(|error| error.to_string())?;
    let ids = {
        let mut statement = conn.prepare("SELECT id FROM mem_items WHERE kind IN ('skill','archived') AND (source='blueprint-library' OR (EXISTS (SELECT 1 FROM json_each(tags_json) WHERE value='curator') AND EXISTS (SELECT 1 FROM json_each(tags_json) WHERE value LIKE 'confidence:%'))) AND NOT EXISTS (SELECT 1 FROM solution_metadata WHERE item_id=mem_items.id)").map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };
    let single_intent = {
        let mut statement = conn.prepare("SELECT item_id FROM solution_metadata WHERE json_extract(metadata_json,'$.blueprint') IS NOT NULL AND json_extract(metadata_json,'$.blueprint.intents') IS NULL").map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?
    };
    for id in ids.into_iter().chain(single_intent) {
        store::delete_with_reason_with_context(
            &crate::backend::WriteContext::new(store::MemAuthor::system("unscoped"), "user"),
            conn,
            &id,
            Some("obsolete solution metadata shape"),
        )?;
    }
    super::initialize(conn)
}

pub(crate) fn read(conn: &Connection, id: &str) -> Result<Option<SolutionMetadata>, String> {
    let json = conn
        .query_row(
            "SELECT metadata_json FROM solution_metadata WHERE item_id=?1",
            [id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    json.map(|json| {
        serde_json::from_str(&json)
            .map_err(|error| format!("invalid solution metadata for {id}: {error}"))
    })
    .transpose()
}

pub(crate) fn write(
    conn: &Connection,
    id: &str,
    metadata: &SolutionMetadata,
) -> Result<(), String> {
    conn.execute("INSERT INTO solution_metadata(item_id,metadata_json) VALUES (?1,?2) ON CONFLICT(item_id) DO UPDATE SET metadata_json=excluded.metadata_json",params![id,serde_json::to_string(metadata).map_err(|error| error.to_string())?]).map_err(|error| error.to_string())?;
    Ok(())
}

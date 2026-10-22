use super::*;

pub(crate) fn scoped_vectors(
    conn: &Connection,
    vector: &StampedVector,
    limit: usize,
    query: &SearchQuery,
    cfg: &SearchConfig,
) -> Result<Vec<(String, f64)>, String> {
    let Some(source) = &cfg.source_filter else {
        return crate::embeddings::recall_vector(conn, vector, limit);
    };
    crate::embedder::index::check(conn, &vector.identity)?;
    let mut statement = conn
        .prepare(
            "SELECT m.id, MIN(vec_distance_cosine(v.embedding, ?1)) AS distance
         FROM mem_vectors v JOIN mem_items m
         ON m.id = CASE
             WHEN substr(rtrim(v.mem_id,'0123456789'),-7)='_chunk_'
             AND length(rtrim(v.mem_id,'0123456789')) < length(v.mem_id)
             THEN substr(rtrim(v.mem_id,'0123456789'),1,length(rtrim(v.mem_id,'0123456789'))-7)
             ELSE v.mem_id END
         WHERE m.kind != 'archived' AND m.source IS ?2
         AND (?3 IS NULL OR m.kind=?3) AND (?4 IS NULL OR m.area=?4)
         AND (?5 IS NULL OR m.scope=?5)
         GROUP BY m.id ORDER BY distance, m.id LIMIT ?6",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params![
                crate::embeddings::encode_vector(&vector.vector),
                source,
                query.kind,
                query.area,
                query.scope,
                limit as i64
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

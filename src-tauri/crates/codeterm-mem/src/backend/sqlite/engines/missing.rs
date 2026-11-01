use crate::reindex::{body_is_embeddable, compose_item_embed_text};
pub(crate) fn query_missing_vectors(
    conn: &rusqlite::Connection,
    blobs: &std::path::Path,
) -> Vec<(String, String)> {
    let Ok(_read) = super::targets::UntouchedRead::begin(conn) else {
        return Vec::new();
    };
    let Ok(mut statement) = conn.prepare(
        "SELECT m.id FROM mem_items m LEFT JOIN mem_vectors v ON v.mem_id=m.id
         WHERE v.mem_id IS NULL AND m.kind != 'archived'",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = statement.query_map([], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };
    rows.flatten()
        .filter_map(|id| {
            let item = crate::store::get(conn, &id, blobs).ok()??;
            body_is_embeddable(&item.body).then(|| {
                let text = compose_item_embed_text(
                    &item.kind,
                    &item.title,
                    item.area.as_deref(),
                    &item.tags,
                    &item.body,
                    item.source.as_deref(),
                    item.summary.as_deref(),
                );
                (id, text)
            })
        })
        .collect()
}

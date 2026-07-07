//! Catch-up vector reindexing for items that were inserted before the embeddings
//! feature was active or while the embed worker was down.

#[cfg(feature = "embeddings")]
use std::sync::Arc;

use super::embeddings;
use super::MemManager;

/// Minimum trimmed body length worth embedding. Below this the item has no real
/// content for vector search — only its title/metadata header — so we skip the
/// vector and let FTS handle it. Keeps the index free of empty/thin nodes.
pub const MIN_EMBED_BODY_CHARS: usize = 12;

/// Single source of truth for "is this body worth a vector?", shared by the live
/// save path and the catch-up reindex so they never disagree.
pub fn body_is_embeddable(body: &str) -> bool {
    body.trim().chars().count() >= MIN_EMBED_BODY_CHARS
}

/// Build the embedding input text for an item.
/// Format: "kind: title [area] #tag1 #tag2\n\nbody" — compact header so
/// the embedder captures title/kind/area/tags alongside body content.
#[cfg(feature = "embeddings")]
pub fn compose_embed_text(
    kind: &str,
    title: &str,
    area: Option<&str>,
    tags: &[String],
    body: &str,
) -> String {
    let mut header = format!("{}: {}", kind, title);
    if let Some(a) = area {
        if !a.is_empty() {
            header.push_str(&format!(" [{}]", a));
        }
    }
    for tag in tags.iter().filter(|t| !t.is_empty()) {
        header.push_str(&format!(" #{}", tag));
    }
    format!("{}\n\n{}", header, body)
}

#[cfg(feature = "embeddings")]
fn query_missing_vectors(conn: &rusqlite::Connection) -> Vec<(String, String)> {
    let sql = r#"SELECT m.id, m.kind, m.title, m.area, m.tags_json, COALESCE(m.body_inline, '')
                 FROM mem_items m
                 LEFT JOIN mem_vectors v ON v.mem_id = m.id
                 WHERE v.mem_id IS NULL AND m.body_inline IS NOT NULL AND LENGTH(TRIM(m.body_inline)) >= 12"#;
    let mut out = Vec::new();
    if let Ok(mut stmt) = conn.prepare(sql) {
        if let Ok(rows) = stmt.query_map([], |r| {
            let id: String = r.get(0)?;
            let kind: String = r.get(1)?;
            let title: String = r.get(2)?;
            let area: Option<String> = r.get(3)?;
            let tags_json: String = r.get(4)?;
            let body: String = r.get(5)?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            Ok((
                id,
                compose_embed_text(&kind, &title, area.as_deref(), &tags, &body),
            ))
        }) {
            for row in rows.flatten() {
                out.push(row);
            }
        }
    }
    out
}

impl MemManager {
    /// Catch-up: direct index() call (bypasses embed_queue) for any items
    /// missing vectors. Spawns a background thread; returns immediately.
    #[cfg(feature = "embeddings")]
    pub fn reindex_missing(self: &Arc<Self>) {
        let db_path = self.db_path.clone();
        let blobs_dir = self.blobs_dir.clone();
        std::thread::spawn(move || {
            embeddings::ensure_extension_registered();
            let conn = match rusqlite::Connection::open(&db_path) {
                Ok(c) => {
                    let _ = c.busy_timeout(std::time::Duration::from_secs(5));
                    c
                }
                Err(e) => {
                    log::warn!("[mem] reindex catch-up: conn: {}", e);
                    return;
                }
            };
            let missing = query_missing_vectors(&conn);
            if missing.is_empty() {
                log::info!("[mem] reindex catch-up: nothing missing");
                return;
            }
            log::info!(
                "[mem] reindex catch-up: DIRECT index for {} item(s)",
                missing.len()
            );
            for (id, embed_text) in missing {
                // embed_text already has title prefixed (from query_missing_vectors). If body was empty (blob storage), re-read blob and re-prefix title.
                let actual_text = if embed_text.trim_end().ends_with("\n\n") {
                    let row: Option<(String, Option<String>)> = conn
                        .query_row(
                            "SELECT title, body_blob_path FROM mem_items WHERE id = ?1",
                            rusqlite::params![&id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .ok();
                    let Some((title, Some(rel))) = row else {
                        continue;
                    };
                    let body = std::fs::read_to_string(blobs_dir.join(rel)).unwrap_or_default();
                    format!("{}\n\n{}", title, body)
                } else {
                    embed_text
                };
                match embeddings::index(&conn, &id, &actual_text) {
                    Ok(()) => log::info!("[mem] reindex direct: indexed {}", id),
                    Err(e) => log::warn!("[mem] reindex direct {}: {}", id, e),
                }
            }
        });
    }

    #[cfg(not(feature = "embeddings"))]
    pub fn reindex_missing(self: &std::sync::Arc<Self>) {}
}

#[cfg(test)]
mod tests {
    use super::body_is_embeddable;

    #[test]
    fn embeddable_requires_real_body() {
        assert!(!body_is_embeddable(""));
        assert!(!body_is_embeddable("   \n  "));
        assert!(!body_is_embeddable("too short"));
        assert!(body_is_embeddable("a genuinely meaningful body of content"));
    }
}

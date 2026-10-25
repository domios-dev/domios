//! Catch-up vector reindexing for items that were inserted before the embeddings
//! feature was active or while the embed worker was down.

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

pub(crate) fn compose_item_embed_text(
    kind: &str,
    title: &str,
    area: Option<&str>,
    tags: &[String],
    body: &str,
    source: Option<&str>,
    summary: Option<&str>,
) -> String {
    if source == Some("blueprint-library") {
        if let Ok(document) = codeterm_blueprint_types::blueprint_memory::parse_document(body) {
            return blueprint_description(&document, summary.unwrap_or_default());
        }
    }
    compose_embed_text(kind, title, area, tags, body)
}

fn blueprint_description(
    document: &codeterm_blueprint_types::blueprint_document::Document,
    when_to_use: &str,
) -> String {
    use std::collections::BTreeSet;
    let mut kinds = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut sinks = BTreeSet::new();
    use codeterm_blueprint_types::blueprint_document::{NodeKind, Trigger};
    if let Some(trigger) = &document.trigger {
        sources.insert(
            match trigger {
                Trigger::Webhook { .. } => "webhook",
                Trigger::Schedule { .. } => "schedule",
                Trigger::Event { .. } => "event",
                Trigger::Input { .. } => "input",
            }
            .to_owned(),
        );
    }
    for node in document.nodes() {
        kinds.insert(node.kind().to_owned());
        if matches!(
            node.uses,
            Some(NodeKind::Probe | NodeKind::Observe | NodeKind::Input | NodeKind::Terminal)
        ) {
            sources.insert(node.kind().to_owned());
        }
        if node.uses.as_ref().is_some_and(NodeKind::effect) {
            sinks.insert(node.kind().to_owned());
        }
    }
    if let Some(to) = document
        .result
        .to
        .as_ref()
        .and_then(codeterm_blueprint_types::blueprint_document::ResultTarget::fixed)
    {
        sinks.insert(to.to_owned());
    }
    let join = |values: BTreeSet<String>| values.into_iter().collect::<Vec<_>>().join(", ");
    format!(
        "{}\nNode kinds: {}\nSources: {}\nSinks: {}",
        when_to_use.trim(),
        join(kinds),
        join(sources),
        join(sinks)
    )
}

/// Build the embedding input text for an item.
/// Format: "kind: title [area] #tag1 #tag2\n\nbody" — compact header so
/// the embedder captures title/kind/area/tags alongside body content.
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

fn query_missing_vectors(
    conn: &rusqlite::Connection,
    blobs: &std::path::Path,
) -> Vec<(String, String)> {
    let Ok(_read) = super::manager_search::UntouchedRead::begin(conn) else {
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
            let item = super::store::get(conn, &id, blobs).ok()??;
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

impl MemManager {
    /// Catch-up for items missing vectors and items left vector pending.
    /// Spawns a background thread; returns immediately.
    pub fn reindex_missing(self: &Arc<Self>) {
        self.backfill_pending();
        let db_path = self.db_path.clone();
        let blobs_dir = self.blobs_dir.clone();
        let embedder = self.embedder.clone();
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
            let missing = query_missing_vectors(&conn, &blobs_dir);
            if missing.is_empty() {
                log::info!("[mem] reindex catch-up: nothing missing");
                return;
            }
            log::info!(
                "[mem] reindex catch-up: DIRECT index for {} item(s)",
                missing.len()
            );
            for (id, embed_text) in missing {
                match embeddings::index_with(&conn, &embedder, &id, &embed_text) {
                    Ok(()) => log::info!("[mem] reindex direct: indexed {}", id),
                    Err(e) => log::warn!("[mem] reindex direct {}: {}", id, e),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests;

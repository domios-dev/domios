use super::*;
use codeterm_mem::{
    blueprint_library::{enrichment::blueprint_vector_texts, index::IndexRow, SOURCE},
    compose_item_embed_text, embeddings,
};

fn item_text(item: &codeterm_mem::MemItem) -> Option<String> {
    codeterm_mem::body_is_embeddable(&item.body).then(|| {
        compose_item_embed_text(
            &item.kind,
            &item.title,
            item.area.as_deref(),
            &item.tags,
            &item.body,
            item.source.as_deref(),
            item.summary.as_deref(),
        )
    })
}

pub(super) fn items(
    tx: &mut Transaction<'_>,
    missing: bool,
) -> Result<Vec<(String, String)>, String> {
    if missing && vectors::stamp_in(tx)?.is_none() {
        return Ok(Vec::new());
    }
    let filter = if missing {
        " AND NOT EXISTS(SELECT 1 FROM mem_vectors v WHERE v.mem_id=m.id)"
    } else {
        ""
    };
    let rows = tx
        .query(
            &format!("SELECT m.id FROM mem_items m WHERE m.kind<>'archived'{filter} ORDER BY m.id"),
            &[],
        )
        .map_err(db_error)?;
    tx.batch_execute("SAVEPOINT engine_targets")
        .map_err(db_error)?;
    let result = (|| {
        let mut out = Vec::new();
        for row in rows {
            let id: String = row.get(0);
            if let Some(item) = crate::items::get_in(tx, &id)? {
                if let Some(text) = item_text(&item) {
                    out.push((id, text));
                }
            }
        }
        Ok(out)
    })();
    tx.batch_execute("ROLLBACK TO engine_targets; RELEASE engine_targets")
        .map_err(db_error)?;
    result
}
pub(super) fn entities(
    tx: &mut Transaction<'_>,
    missing: bool,
) -> Result<Vec<(String, String)>, String> {
    if missing && vectors::stamp_in(tx)?.is_none() {
        return Err("vector index is not initialized".into());
    }
    let filter = if missing {
        " WHERE NOT EXISTS(SELECT 1 FROM mem_vectors v WHERE v.mem_id=e.id)"
    } else {
        ""
    };
    Ok(tx
        .query(
            &format!(
                "SELECT e.id,e.entity_type,e.properties_json FROM entities e{filter} ORDER BY e.id"
            ),
            &[],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|r| {
            let id: String = r.get(0);
            let text = embeddings::embeddable_text_for_entity(
                &id,
                &r.get::<_, String>(1),
                &r.get::<_, String>(2),
            );
            (id, text)
        })
        .collect())
}
pub(super) fn blueprints(tx: &mut Transaction<'_>) -> Result<Vec<ShadowUnit>, String> {
    tx.query("SELECT b.*,s.metadata_json FROM blueprint_index b JOIN mem_items m ON m.id=b.item_id LEFT JOIN solution_metadata s ON s.item_id=b.item_id WHERE m.kind='skill' AND m.source=$1 ORDER BY b.item_id", &[&SOURCE]).map_err(db_error)?.into_iter().map(|r| {
        let row = IndexRow {
            document_hash:r.get("document_hash"), canon:r.get("canon"), gists:r.get("gists"),
            signature:serde_json::from_str(&r.get::<_,String>("signature_json")).map_err(|e|e.to_string())?,
            enrichment:r.get::<_,Option<String>>("enrichment_json").map(|s|serde_json::from_str(&s)).transpose().map_err(|e|e.to_string())?
        };
        let intents = r.get::<_,Option<String>>("metadata_json").and_then(|s|serde_json::from_str::<codeterm_mem::solution_metadata::SolutionMetadata>(&s).ok()).and_then(|m|m.blueprint).map(|b|b.intents).unwrap_or_default();
        Ok(ShadowUnit { id:r.get("item_id"), blueprint:true, texts:blueprint_vector_texts(&row, &intents) })
    }).collect()
}
pub(super) fn units(tx: &mut Transaction<'_>) -> Result<Vec<ShadowUnit>, String> {
    let mut units = blueprints(tx)?;
    let mut targets = items(tx, false)?;
    targets.extend(entities(tx, false)?);
    units.extend(targets.into_iter().map(|(id, text)| {
        ShadowUnit {
            id,
            blueprint: false,
            texts: embeddings::chunk_text(&text)
                .into_iter()
                .enumerate()
                .map(|(i, t)| (String::new(), i, t))
                .collect(),
        }
    }));
    Ok(units)
}
pub(super) fn plan(
    tx: &mut Transaction<'_>,
    docs: u64,
) -> Result<codeterm_mem::embedder::SwitchPlan, String> {
    Ok(codeterm_mem::embedder::SwitchPlan {
        blueprints: blueprints(tx)?.len() as u64,
        items: items(tx, false)?.len() as u64,
        entities: entities(tx, false)?.len() as u64,
        docs,
    })
}
pub(super) fn pending_text(tx: &mut Transaction<'_>, id: &str) -> Result<Option<String>, String> {
    if let Some(item) = crate::items::get_in(tx, id)? {
        return Ok(item_text(&item));
    }
    Ok(tx
        .query_opt(
            "SELECT entity_type,properties_json FROM entities WHERE id=$1",
            &[&id],
        )
        .map_err(db_error)?
        .map(|r| {
            embeddings::embeddable_text_for_entity(
                id,
                &r.get::<_, String>(0),
                &r.get::<_, String>(1),
            )
        }))
}

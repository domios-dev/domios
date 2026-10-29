use super::search_in;
use crate::{db_error, items, vectors, StoreResult};
use codeterm_mem::{
    embedder::StampedVector,
    search::{extract_keywords, ranking, SearchConfig, SearchMode},
    MemItem, SearchHit, SearchQuery,
};
use postgres::Transaction;
use std::collections::HashSet;

pub(crate) fn recall_with_embedding_in(
    tx: &mut Transaction<'_>,
    query: &SearchQuery,
    cfg: &SearchConfig,
    embedding: Option<&StampedVector>,
) -> StoreResult<Vec<SearchHit>> {
    let limit = query.limit.unwrap_or(5).clamp(1, 50);
    let vector_first = cfg.mode == SearchMode::VectorFirstRerank && embedding.is_some();
    let pool = limit
        * if vector_first {
            cfg.vector_rerank_pool_multiplier.max(1)
        } else {
            5
        };
    let mut vectors = Vec::new();
    let stamp = if embedding.is_some() {
        vectors::stamp_in(tx)?
    } else {
        None
    };
    if let Some(vector) = embedding.filter(|vector| stamp.as_ref() == Some(&vector.identity)) {
        for (id, score) in super::scoped_vectors_in(tx, vector, pool, query, cfg)? {
            if let Some(item) = candidate_in(tx, &id)? {
                if ranking::matches_filters(&item, query, cfg) {
                    vectors.push(SearchHit { item, score });
                }
            }
        }
    }
    if vector_first {
        let scorer = ranking::Scorer::new(&query.query, cfg, chrono::Utc::now());
        for hit in &mut vectors {
            hit.score = scorer.vector(&hit.item, hit.score);
        }
        ranking::sort_hits(&mut vectors);
        vectors.truncate(limit);
        return Ok(vectors);
    }
    let wide_query = SearchQuery {
        limit: Some(limit * 5),
        ..query.clone()
    };
    let lexical = search_in(tx, &wide_query, cfg)?;
    let mut graph = Vec::new();
    if !lexical.is_empty()
        && !vectors.is_empty()
        && (cfg.graph_entity_boost > 1.0 || cfg.graph_as_rrf_source)
    {
        let entities = matched_entities_in(tx, &extract_keywords(&query.query))?;
        for row in tx
            .query(
                "SELECT DISTINCT from_id FROM relations WHERE to_id=ANY($1)
             UNION SELECT DISTINCT to_id FROM relations WHERE from_id=ANY($1)",
                &[&entities],
            )
            .map_err(db_error)?
        {
            let id: String = row.get(0);
            let item = if cfg.graph_as_rrf_source {
                candidate_in(tx, &id)?
            } else {
                None
            };
            graph.push((id, item));
        }
    }
    Ok(ranking::fuse(query, cfg, lexical, vectors, graph))
}

pub(crate) fn scoped_vectors_in(
    tx: &mut Transaction<'_>,
    vector: &StampedVector,
    limit: usize,
    query: &SearchQuery,
    cfg: &SearchConfig,
) -> StoreResult<Vec<(String, f64)>> {
    let Some(source) = &cfg.source_filter else {
        return vectors::nearest_in(tx, vector, limit);
    };
    vectors::check_in(tx, &vector.identity)?;
    codeterm_mem::embedder::provider::validate_vector(&vector.vector, vector.identity.dimension)
        .map_err(|error| error.to_string())?;
    let rows = tx
        .query(
            "SELECT m.id,MIN(v.embedding <=> $1) AS distance
         FROM mem_vectors v JOIN mem_items m ON m.id=regexp_replace(v.mem_id,'_chunk_[0-9]+$','')
         WHERE m.kind!='archived' AND m.source IS NOT DISTINCT FROM $2::text
         AND ($3::text IS NULL OR m.kind=$3) AND ($4::text IS NULL OR m.area=$4)
         AND ($5::text IS NULL OR m.scope=$5)
         GROUP BY m.id ORDER BY distance,m.id LIMIT $6",
            &[
                &pgvector::Vector::from(vector.vector.clone()),
                &source,
                &query.kind,
                &query.area,
                &query.scope,
                &(limit as i64),
            ],
        )
        .map_err(db_error)?;
    Ok(rows
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect())
}

pub(crate) fn matched_entities_in(
    tx: &mut Transaction<'_>,
    keywords: &HashSet<String>,
) -> StoreResult<Vec<String>> {
    if keywords.is_empty() {
        return Ok(Vec::new());
    }
    let patterns: Vec<_> = keywords.iter().map(|word| format!("%{word}%")).collect();
    Ok(tx
        .query(
            "SELECT id FROM entities WHERE lower(id) LIKE ANY($1) ORDER BY id",
            &[&patterns],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|row| row.get(0))
        .collect())
}

fn candidate_in(tx: &mut Transaction<'_>, id: &str) -> StoreResult<Option<MemItem>> {
    if let Some(item) = items::get_in(tx, id)? {
        return Ok(Some(item));
    }
    Ok(tx.query_opt(
        "SELECT id,entity_type,scope,properties_json,created_at,updated_at FROM entities WHERE id=$1",
        &[&id],
    ).map_err(db_error)?.map(|row| codeterm_mem::store::synthesize_entity(
        row.get(0), row.get(1), row.get(2), row.get(3), row.get(4), row.get(5),
    )))
}

mod recall;
pub(crate) use recall::{matched_entities_in, recall_with_embedding_in, scoped_vectors_in};

use crate::{db_error, items, vectors, StoreResult};
use codeterm_mem::{
    store::{GraphDump, GraphDumpStats, GraphEdge, GraphNode, TailEntry},
    MemItem, MemStats, SearchHit, SearchQuery,
};
use postgres::{types::ToSql, Transaction};
use std::collections::{HashMap, HashSet};

pub(crate) fn items_in(
    tx: &mut Transaction<'_>,
    sql: &str,
    params: &[&(dyn ToSql + Sync)],
) -> StoreResult<Vec<MemItem>> {
    let mut items = Vec::new();
    for row in tx.query(sql, params).map_err(db_error)? {
        if let Some(item) = items::get_in(tx, &row.get::<_, String>(0))? {
            items.push(item);
        }
    }
    Ok(items)
}

pub(crate) fn touch_in(tx: &mut Transaction<'_>, ids: &[String]) -> StoreResult<()> {
    let mut seen = HashSet::new();
    for id in ids.iter().filter(|id| seen.insert(id.as_str())) {
        tx.execute(
            "UPDATE mem_items SET accessed_at=$2,access_count=access_count+1 WHERE id=$1",
            &[id, &items::now()],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

pub(crate) fn stats_in(tx: &mut Transaction<'_>) -> StoreResult<MemStats> {
    let mut by_kind = HashMap::new();
    for row in tx
        .query(
            "SELECT kind,count(*) FROM mem_items WHERE kind!='archived' GROUP BY kind",
            &[],
        )
        .map_err(db_error)?
    {
        by_kind.insert(row.get(0), row.get::<_, i64>(1) as u64);
    }
    let counts = crate::transfer::counts_in(tx)?;
    use codeterm_mem::backend::StoreTable;
    Ok(MemStats {
        total_items: counts[&StoreTable::Items],
        by_kind,
        total_entities: counts[&StoreTable::Entities],
        total_vectors: counts[&StoreTable::Vectors],
        total_relations: tx
            .query_one("SELECT count(*) FROM relations WHERE valid_to IS NULL", &[])
            .map_err(db_error)?
            .get::<_, i64>(0) as u64,
    })
}

pub(crate) fn taxonomy_in(
    tx: &mut Transaction<'_>,
    workspace: Option<&str>,
) -> StoreResult<HashMap<String, HashMap<String, u64>>> {
    let scope = workspace.map(|id| format!("workspace:{id}"));
    let mut out = HashMap::<String, HashMap<String, u64>>::new();
    for row in tx.query("SELECT coalesce(nullif(area,''),'(none)') AS area_k,kind,count(*) FROM mem_items WHERE kind!='archived' AND ($1::text IS NULL OR scope=$1) GROUP BY area_k,kind", &[&scope]).map_err(db_error)? {
        out.entry(row.get(0)).or_default().insert(row.get(1),row.get::<_,i64>(2) as u64);
    }
    Ok(out)
}

pub(crate) fn tail_in(
    tx: &mut Transaction<'_>,
    since: Option<&str>,
    limit: usize,
) -> StoreResult<Vec<TailEntry>> {
    Ok(tx.query("SELECT m.id,m.kind,m.title,m.area,m.updated_at,coalesce((SELECT changed_by FROM mem_item_history h WHERE h.item_id=m.id ORDER BY changed_at DESC,id DESC LIMIT 1),'user') FROM mem_items m WHERE ($1::text IS NULL OR m.updated_at>$1) ORDER BY m.updated_at DESC LIMIT $2", &[&since,&crate::pg_limit(limit)]).map_err(db_error)?.into_iter().map(|row|TailEntry { id:row.get(0),kind:row.get(1),title:row.get(2),area:row.get(3),updated_at:row.get(4),changed_by:row.get(5) }).collect())
}

pub(crate) fn search_in(
    tx: &mut Transaction<'_>,
    query: &SearchQuery,
    cfg: &codeterm_mem::search::SearchConfig,
) -> StoreResult<Vec<SearchHit>> {
    let (words, conjunctive) =
        codeterm_mem::search::fts_terms(&query.query, cfg.fts_drop_stopwords);
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let expression = words
        .iter()
        .map(|word| format!("'{word}'"))
        .collect::<Vec<_>>()
        .join(if conjunctive { " & " } else { " | " });
    let limit = query.limit.unwrap_or(5).clamp(1, 50);
    let source = cfg
        .source_filter
        .as_ref()
        .and_then(|value| value.as_deref());
    let filter_source = cfg.source_filter.is_some();
    let rows=tx.query("SELECT m.id,ts_rank_cd(f.document,to_tsquery('simple',$1))::float8 FROM mem_fts f JOIN mem_items m ON m.id=f.mem_id WHERE f.document @@ to_tsquery('simple',$1) AND m.kind!='archived' AND ($2::text IS NULL OR m.kind=$2) AND ($3::text IS NULL OR m.scope=$3) AND ($4::text IS NULL OR m.area=$4) AND (NOT $5::boolean OR m.source IS NOT DISTINCT FROM $6::text) ORDER BY 2 DESC,m.id LIMIT $7",
        &[&expression,&query.kind,&query.scope,&query.area,&filter_source,&source,&crate::pg_limit(limit.saturating_mul(5))]).map_err(db_error)?;
    let scorer = codeterm_mem::search::ranking::Scorer::new(&query.query, cfg, chrono::Utc::now());
    let mut hits = Vec::new();
    for row in rows {
        let Some(item) = items::get_in(tx, &row.get::<_, String>(0))? else {
            continue;
        };
        let score = scorer.lexical(&item, row.get(1));
        hits.push(SearchHit { item, score });
    }
    codeterm_mem::search::ranking::sort_lexical_hits(&mut hits);
    hits.truncate(limit);
    Ok(hits)
}

pub(crate) fn graph_dump_in(tx: &mut Transaction<'_>) -> StoreResult<GraphDump> {
    let vector_ids: HashSet<String> = if vectors::stamp_in(tx)?.is_some() {
        tx.query("SELECT mem_id FROM mem_vectors", &[])
            .map_err(db_error)?
            .into_iter()
            .map(|row| row.get(0))
            .collect()
    } else {
        HashSet::new()
    };
    let mut nodes = Vec::new();
    for item in items_in(
        tx,
        "SELECT id FROM mem_items WHERE kind!='archived' ORDER BY id",
        &[],
    )? {
        nodes.push(GraphNode {
            has_vector: vector_ids.contains(&item.id),
            has_body: !item.body.is_empty(),
            id: item.id,
            kind: item.kind,
            title: item.title,
            summary: item.summary,
            area: item.area,
            importance: item.importance,
            tags: item.tags,
            access_count: item.access_count,
            recurrence_count: item.recurrence_count,
        });
    }
    let ids: HashSet<_> = nodes.iter().map(|node| node.id.clone()).collect();
    for row in tx
        .query(
            "SELECT id,entity_type,properties_json FROM entities ORDER BY id",
            &[],
        )
        .map_err(db_error)?
    {
        let id: String = row.get(0);
        if ids.contains(&id) {
            continue;
        }
        let props: serde_json::Value = serde_json::from_str(&row.get::<_, String>(2))
            .map_err(|_| "invalid entity properties")?;
        nodes.push(GraphNode {
            title: props["label"].as_str().unwrap_or(&id).to_owned(),
            id,
            kind: row.get::<_, String>(1).to_lowercase(),
            summary: None,
            area: None,
            importance: 3.0,
            tags: vec!["entity".into()],
            has_vector: false,
            has_body: false,
            access_count: 0,
            recurrence_count: 0,
        });
    }
    let ids: HashSet<_> = nodes.iter().map(|node| node.id.clone()).collect();
    let edges=tx.query("SELECT from_id,to_id,relation_type,confidence,valid_from,valid_to FROM relations WHERE valid_to IS NULL ORDER BY id", &[]).map_err(db_error)?.into_iter().map(|row|GraphEdge {from:row.get(0),to:row.get(1),rel:row.get(2),confidence:row.get(3),valid_from:row.get(4),valid_to:row.get(5) }).filter(|edge|ids.contains(&edge.from)&&ids.contains(&edge.to)).collect::<Vec<_>>();
    let stats = GraphDumpStats {
        node_count: nodes.len(),
        edge_count: edges.len(),
    };
    Ok(GraphDump {
        nodes,
        edges,
        stats,
    })
}

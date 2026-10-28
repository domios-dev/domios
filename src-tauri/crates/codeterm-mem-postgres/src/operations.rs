use crate::{db_error, items, queries, PostgresMemStore, StoreResult};
use codeterm_mem::{
    backend::{Deletion, WriteContext},
    embedder::{ActiveEmbedder, EmbedError, StampedVector},
    search::SearchConfig,
    store::{HistorySnapshot, MemUpdate},
    NewMemItem, SearchHit, SearchQuery,
};
use std::{sync::Arc, time::Duration};

impl PostgresMemStore {
    pub(crate) fn deletion_targets_in(
        &self,
        id: &str,
    ) -> StoreResult<(Vec<String>, Option<String>)> {
        self.transaction(|tx| {
            let source=tx.query_opt("SELECT coalesce(source_path,source) FROM mem_items WHERE id=$1 AND kind='skill' AND (source_path LIKE 'file:%' OR source LIKE 'file:%')", &[&id]).map_err(db_error)?.and_then(|row|row.get::<_,Option<String>>(0));
            let ids=if let Some(source)=&source {
                tx.query("SELECT id FROM mem_items WHERE kind='skill' AND (source_path=$1 OR source=$1) ORDER BY id", &[source]).map_err(db_error)?.into_iter().map(|row|row.get(0)).collect()
            } else {vec![id.to_owned()]};
            Ok((ids,source))
        })
    }

    pub(crate) fn delete_batch_in(
        &self,
        ctx: &WriteContext,
        ids: &[String],
    ) -> StoreResult<Deletion> {
        self.transaction(|tx| {
            let mut report = Deletion::default();
            for id in ids {
                let item = items::get_in(tx, id)?;
                let source = tx
                    .query_opt(
                        "SELECT source_path FROM mem_items WHERE id=$1 AND kind='skill'",
                        &[id],
                    )
                    .map_err(db_error)?
                    .and_then(|row| row.get::<_, Option<String>>(0));
                if items::delete_in(tx, ctx, id)? {
                    report.ids.push(id.clone());
                    report.library_areas.extend(
                        item.filter(|item| item.source.as_deref() == Some("blueprint-library"))
                            .and_then(|item| item.area),
                    );
                    report.skill_sources.push((id.clone(), source));
                }
            }
            Ok(report)
        })
    }

    pub(crate) fn sync_status_in(&self, timeout: Duration) -> Option<serde_json::Value> {
        let _gate = self.gate.try_lock_for(timeout)?;
        let mut conn = self.pool.get_timeout(timeout).ok()?;
        let row=conn.query_one("SELECT count(*) FILTER (WHERE source_path IS NOT NULL),count(*) FILTER (WHERE source_path IS NOT NULL AND tags_json LIKE '%source-missing%') FROM mem_items", &[]).ok()?;
        Some(
            serde_json::json!({"tracked_files":row.get::<_,i64>(0),"stale_files":row.get::<_,i64>(1)}),
        )
    }

    pub(crate) fn cas_refresh_in(
        &self,
        ctx: &WriteContext,
        id: &str,
        expected: Option<&str>,
        item: &NewMemItem,
        fingerprint: &str,
    ) -> StoreResult<bool> {
        self.transaction(|tx| {
            let Some((_,stamp))=items::payload_in(tx,id)? else {return Ok(false);};
            if stamp.as_deref()!=expected {return Ok(false);}
            let previous=items::get_in(tx,id)?.ok_or("memory item disappeared")?;
            let author=if previous.author.is_neighbor() {previous.author} else {ctx.author.clone()};
            codeterm_mem::store::guard_instruction(&author,item.kind.as_str(),item.pinned)?;
            let (inline,blob)=items::body_in(tx,&item.body)?;
            tx.execute("UPDATE mem_items SET kind=$2,scope=$3,title=$4,summary=$5,area=$6,tags_json=$7,body_inline=$8,body_blob_path=$9,importance=$10,source_path=$11,is_pinned_wakeup=$12,updated_at=$13,seed_fingerprint=$14,author_kind=$15,author_uid=$16,author_friendship=$17,author_label=$18 WHERE id=$1", &[&id,&item.kind.as_str(),&item.scope.as_str(),&item.title,&item.summary,&item.area,&serde_json::to_string(&item.tags).map_err(|_|"serialize memory tags")?,&inline,&blob,&item.importance,&item.source_path,&i64::from(item.pinned),&items::now(),&fingerprint,&author.kind.as_str(),&author.account_uid,&author.friendship_id,&author.label]).map_err(db_error)?;
            items::fts_in(tx,id,item)?; Ok(true)
        })
    }

    pub(crate) fn restore_kind_in(
        &self,
        ctx: &WriteContext,
        id: &str,
        restore: &codeterm_mem::curation::merge::KindRestore,
        body: Option<&str>,
        actor: &str,
    ) -> StoreResult<serde_json::Value> {
        self.transaction(|tx| {
            let item=items::get_in(tx,id)?.ok_or("memory item missing")?;
            if item.kind!="archived" {return Err("item is not archived".into());}
            codeterm_mem::store::guard_instruction(&item.author,&restore.kind,false)?;
            codeterm_mem::store::guard_instruction(&ctx.author,&restore.kind,false)?;
            tx.execute("UPDATE mem_items SET kind=$2,updated_at=$3 WHERE id=$1", &[&id,&restore.kind,&items::now()]).map_err(db_error)?;
            items::history_in(tx,&ctx.with_changed_by(actor),id,"restore",&HistorySnapshot {kind:Some("archived".into()),..Default::default()},&HistorySnapshot {kind:Some(restore.kind.clone()),..Default::default()},Some(&format!("undo history row {}",restore.history_id)))?;
            if let Some(body)=body {items::update_in(tx,&ctx.with_changed_by(actor),id,&MemUpdate {body:Some(body.into()),..Default::default()})?;}
            let count=tx.execute("UPDATE relations SET valid_to=$2 WHERE valid_to IS NULL AND reason='mem-merge' AND ((from_id=$1 AND relation_type='merged_into') OR (to_id=$1 AND relation_type='supersedes'))", &[&id,&items::now()]).map_err(db_error)?;
            Ok(serde_json::json!({"restored":id,"kind":restore.kind,"body_restored":body.is_some(),"invalidated_relations":count}))
        })
    }

    pub(crate) fn store_vectors_in(
        &self,
        id: &str,
        values: Vec<Result<StampedVector, EmbedError>>,
    ) -> StoreResult<()> {
        for (chunk, value) in values.into_iter().enumerate() {
            match value {
                Ok(vector) => {
                    self.insert_vector(id, chunk, &vector)?;
                }
                Err(EmbedError::Unavailable(reason)) => {
                    self.transaction(|tx| {tx.execute("INSERT INTO mem_vector_pending(mem_id,reason,queued_at) VALUES ($1,$2,$3) ON CONFLICT(mem_id) DO UPDATE SET reason=excluded.reason", &[&id,&reason,&items::now()]).map_err(db_error)?;Ok(())})?;
                    return Err(EmbedError::Unavailable(reason).to_string());
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        self.transaction(|tx| {
            tx.execute("DELETE FROM mem_vector_pending WHERE mem_id=$1", &[&id])
                .map_err(db_error)?;
            Ok(())
        })
    }

    pub(crate) fn recall_in(
        &self,
        query: &SearchQuery,
        cfg: &SearchConfig,
        embedder: &Arc<ActiveEmbedder>,
        record_access: bool,
    ) -> StoreResult<Vec<SearchHit>> {
        let vector = embedder.query(&query.query);
        self.transaction(|tx| {
            let hits = queries::recall_with_embedding_in(tx, query, cfg, vector.as_ref())?;
            if record_access {
                queries::touch_in(
                    tx,
                    &hits
                        .iter()
                        .map(|hit| hit.item.id.clone())
                        .collect::<Vec<_>>(),
                )?;
            }
            Ok(hits)
        })
    }

    pub(crate) fn diary_in(&self, offset: usize, limit: usize) -> StoreResult<serde_json::Value> {
        self.transaction(|tx| {
            let total:i64=tx.query_one("SELECT count(*) FROM mem_items WHERE kind='daily' AND area='_dreams'", &[]).map_err(db_error)?.get(0);
            let items=queries::items_in(tx,"SELECT id FROM mem_items WHERE kind='daily' AND area='_dreams' ORDER BY created_at DESC,id DESC LIMIT $1 OFFSET $2", &[&(limit as i64),&(offset as i64)])?;
            let next=(offset+items.len()<total as usize).then_some(offset+items.len());
            Ok(serde_json::json!({"items":items,"total":total,"next_offset":next}))
        })
    }

    pub(crate) fn type_counts_in(&self) -> (Vec<(String, u64)>, Vec<(String, u64)>) {
        self.transaction(|tx| {
            let entities=tx.query("SELECT entity_type,count(*) FROM entities GROUP BY entity_type ORDER BY count(*) DESC LIMIT 20", &[]).map_err(db_error)?.into_iter().map(|row|(row.get(0),row.get::<_,i64>(1) as u64)).collect();
            let relations=tx.query("SELECT relation_type,count(*) FROM relations GROUP BY relation_type ORDER BY count(*) DESC LIMIT 30", &[]).map_err(db_error)?.into_iter().map(|row|(row.get(0),row.get::<_,i64>(1) as u64)).collect();
            Ok((entities,relations))
        }).unwrap_or_default()
    }

    pub(crate) fn tunnels_in(
        &self,
        include_invalidated: bool,
    ) -> StoreResult<Vec<serde_json::Value>> {
        self.transaction(|tx| Ok(tx.query("SELECT e.id,e.entity_type,count(DISTINCT m.area),array_agg(DISTINCT m.area) FROM entities e JOIN relations r ON r.to_id=e.id OR r.from_id=e.id JOIN mem_items m ON m.id=r.from_id OR m.id=r.to_id WHERE m.area IS NOT NULL AND m.area!='' AND left(e.id,5)!='kind_' AND left(e.id,5)!='area_' AND ($1 OR r.valid_to IS NULL) GROUP BY e.id,e.entity_type HAVING count(DISTINCT m.area)>=2 ORDER BY count(DISTINCT m.area) DESC LIMIT 50", &[&include_invalidated]).map_err(db_error)?.into_iter().map(|row|serde_json::json!({"id":row.get::<_,String>(0),"entity_type":row.get::<_,String>(1),"area_count":row.get::<_,i64>(2),"areas":row.get::<_,Vec<String>>(3)})).collect()))
    }

    pub(crate) fn graph_candidates_in(
        &self,
        query: &SearchQuery,
        limit: usize,
    ) -> StoreResult<Vec<SearchHit>> {
        let terms = codeterm_mem::search::extract_keywords(&query.query);
        if terms.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        self.transaction(|tx| {
            let matched = queries::matched_entities_in(tx, &terms)?;
            Ok(queries::items_in(tx,"WITH links(item_id,entity_id) AS (SELECT from_id,to_id FROM relations WHERE to_id=ANY($1) AND valid_to IS NULL UNION SELECT to_id,from_id FROM relations WHERE from_id=ANY($1) AND valid_to IS NULL) SELECT m.id FROM links JOIN mem_items m ON m.id=links.item_id WHERE m.kind!='archived' AND ($2::text IS NULL OR m.kind=$2) AND ($3::text IS NULL OR m.scope=$3) AND ($4::text IS NULL OR m.area=$4) GROUP BY m.id ORDER BY count(DISTINCT entity_id) DESC,m.importance DESC,m.updated_at DESC,m.id LIMIT $5", &[&matched,&query.kind,&query.scope,&query.area,&(limit as i64)])?.into_iter().map(|item|SearchHit {item,score:0.0}).collect())
        })
    }
}


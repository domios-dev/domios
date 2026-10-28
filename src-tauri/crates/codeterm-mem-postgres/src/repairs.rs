use crate::{db_error, graph, items, vectors, StoreResult};
use codeterm_mem::graph_repair::*;
use postgres::Transaction;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn column(column: RelationColumn) -> &'static str {
    match column {
        RelationColumn::FromId => "from_id",
        RelationColumn::RelationType => "relation_type",
        RelationColumn::ToId => "to_id",
        RelationColumn::Scope => "scope",
        RelationColumn::ValidFrom => "valid_from",
    }
}

pub(crate) fn duplicate_relations(
    tx: &mut Transaction<'_>,
    columns: &[RelationColumn],
    limit: usize,
) -> StoreResult<Vec<DuplicateRelationGroup>> {
    if columns.is_empty() {
        return Err("group_by must contain at least one column".into());
    }
    let mut groups =
        BTreeMap::<String, (Value, Vec<RelationMember>, codeterm_mem::Relation)>::new();
    for row in tx
        .query(
            "SELECT * FROM relations WHERE valid_to IS NULL ORDER BY id",
            &[],
        )
        .map_err(db_error)?
    {
        let key = columns
            .iter()
            .map(|c| (column(*c).to_owned(), Value::String(row.get(column(*c)))))
            .collect::<serde_json::Map<_, _>>();
        let key = Value::Object(key);
        let member = RelationMember {
            rowid: row.get("id"),
            relation_type: row.get("relation_type"),
        };
        groups
            .entry(key.to_string())
            .or_insert((key, Vec::new(), graph::relation(row)?))
            .1
            .push(member);
    }
    let mut groups = groups
        .into_values()
        .filter(|(_, members, _)| members.len() > 1)
        .map(|(key, members, sample)| DuplicateRelationGroup {
            key,
            rowids: members.iter().map(|m| m.rowid).collect(),
            count: members.len(),
            members,
            sample,
        })
        .collect::<Vec<_>>();
    groups.sort_by(|a, b| b.count.cmp(&a.count));
    groups.truncate(limit);
    Ok(groups)
}

pub(crate) fn delete_relations(
    tx: &mut Transaction<'_>,
    ids: &[i64],
    actor: &str,
) -> StoreResult<u64> {
    let mut payload = Vec::new();
    for row in tx
        .query(
            "SELECT id,row_to_json(r)::text FROM relations r WHERE id=ANY($1) ORDER BY id",
            &[&ids],
        )
        .map_err(db_error)?
    {
        payload.push(
            serde_json::from_str::<Value>(&row.get::<_, String>(1))
                .map_err(|_| "invalid relation audit")?,
        );
    }
    let count = tx
        .execute("DELETE FROM relations WHERE id=ANY($1)", &[&ids])
        .map_err(db_error)?;
    if !ids.is_empty() {
        graph::audit_in(
            tx,
            actor,
            count,
            json!({"op":"delete_relations_by_rowids","payload":payload}),
        )?;
    }
    Ok(count)
}

pub(crate) fn merge_relations(
    tx: &mut Transaction<'_>,
    keep: i64,
    ids: &[i64],
    actor: &str,
) -> StoreResult<MergeReport> {
    if ids.is_empty() || ids.contains(&keep) {
        return Err("merge list must be nonempty and exclude keep rowid".into());
    }
    let mut all = ids.to_vec();
    all.push(keep);
    let rows = tx
        .query(
            "SELECT id,confidence,properties_json FROM relations WHERE id=ANY($1) ORDER BY id",
            &[&all],
        )
        .map_err(db_error)?;
    if !rows.iter().any(|row| row.get::<_, i64>(0) == keep)
        || ids
            .iter()
            .any(|id| !rows.iter().any(|row| row.get::<_, i64>(0) == *id))
    {
        return Err("relation rowid not found".into());
    }
    let mut confidence = 0.0f64;
    let mut props = serde_json::Map::new();
    for row in rows {
        confidence = confidence.max(row.get(1));
        let value: Value = serde_json::from_str(&row.get::<_, String>(2))
            .map_err(|_| "invalid relation properties")?;
        if let Some(map) = value.as_object() {
            for (key, value) in map {
                props.entry(key.clone()).or_insert(value.clone());
            }
        }
    }
    tx.execute(
        "UPDATE relations SET confidence=$2,properties_json=$3 WHERE id=$1",
        &[&keep, &confidence, &Value::Object(props).to_string()],
    )
    .map_err(db_error)?;
    tx.execute("DELETE FROM relations WHERE id=ANY($1)", &[&ids])
        .map_err(db_error)?;
    graph::audit_in(
        tx,
        actor,
        ids.len() as u64,
        json!({"op":"merge_relations","keep":keep,"merged":ids,"final_confidence":confidence}),
    )?;
    Ok(MergeReport {
        merged_into: keep,
        deleted_rowids: ids.to_vec(),
        final_confidence: confidence,
    })
}

pub(crate) fn delete_entity(
    tx: &mut Transaction<'_>,
    id: &str,
    force: bool,
    purge_history: bool,
    actor: &str,
) -> StoreResult<DeleteEntityReport> {
    let entity = tx
        .query_opt(
            "SELECT row_to_json(e)::text FROM entities e WHERE id=$1",
            &[&id],
        )
        .map_err(db_error)?
        .ok_or("entity not found")?;
    let rows=tx.query("SELECT row_to_json(r)::text FROM relations r WHERE (from_id=$1 OR to_id=$1) AND valid_to IS NULL", &[&id]).map_err(db_error)?;
    if !force && !rows.is_empty() {
        return Err(format!(
            "entity {id} has {} active relations; pass force=true to cascade-delete them",
            rows.len()
        ));
    }
    let payload = rows
        .into_iter()
        .map(|row| {
            serde_json::from_str::<Value>(&row.get::<_, String>(0))
                .map_err(|_| "invalid relation audit".to_owned())
        })
        .collect::<StoreResult<Vec<_>>>()?;
    let active = tx
        .execute(
            "DELETE FROM relations WHERE (from_id=$1 OR to_id=$1) AND valid_to IS NULL",
            &[&id],
        )
        .map_err(db_error)?;
    let history = if purge_history {
        tx.execute(
            "DELETE FROM relations WHERE (from_id=$1 OR to_id=$1) AND valid_to IS NOT NULL",
            &[&id],
        )
        .map_err(db_error)?
    } else {
        0
    };
    tx.execute("DELETE FROM entities WHERE id=$1", &[&id])
        .map_err(db_error)?;
    tx.execute(
        "DELETE FROM entity_aliases WHERE alias_id=$1 OR canonical_id=$1",
        &[&id],
    )
    .map_err(db_error)?;
    vectors::clear_in(tx, id)?;
    graph::audit_in(
        tx,
        actor,
        1 + active + history,
        json!({"op":"delete_entity_by_id","entity":serde_json::from_str::<Value>(&entity.get::<_,String>(0)).map_err(|_|"invalid entity audit")?,"cascaded_relations":payload,"cascaded_history":history,"force":force,"purge_history":purge_history}),
    )?;
    Ok(DeleteEntityReport {
        entity_id: id.into(),
        cascaded_relations: active,
        cascaded_history: history,
    })
}

pub(crate) fn augment_entity(
    tx: &mut Transaction<'_>,
    id: &str,
    patch: serde_json::Map<String, Value>,
    actor: &str,
) -> StoreResult<()> {
    let row = tx
        .query_opt("SELECT properties_json FROM entities WHERE id=$1", &[&id])
        .map_err(db_error)?
        .ok_or("entity not found")?;
    let value: Value =
        serde_json::from_str(&row.get::<_, String>(0)).map_err(|_| "invalid entity properties")?;
    let mut props = value.as_object().cloned().unwrap_or_default();
    props.extend(patch.clone());
    tx.execute(
        "UPDATE entities SET properties_json=$2,updated_at=$3 WHERE id=$1",
        &[
            &id,
            &Value::Object(props.clone()).to_string(),
            &items::now(),
        ],
    )
    .map_err(db_error)?;
    graph::audit_in(
        tx,
        actor,
        0,
        json!({"op":"augment_entity","entity_id":id,"patch":patch,"result":props}),
    )
}

pub(crate) fn duplicate_entities(
    tx: &mut Transaction<'_>,
    strategy: EntitySimilarityStrategy,
    limit: usize,
) -> StoreResult<Vec<DuplicateEntityGroup>> {
    let mut out = Vec::new();
    match strategy {
        EntitySimilarityStrategy::ExactName => {
            for row in tx.query("SELECT entity_type,lower(trim(properties_json::jsonb->>'name')),array_agg(id ORDER BY id) FROM entities WHERE properties_json::jsonb->>'name' IS NOT NULL GROUP BY entity_type,lower(trim(properties_json::jsonb->>'name')) HAVING count(*)>1 ORDER BY count(*) DESC LIMIT $1", &[&(limit as i64)]).map_err(db_error)? {
                out.push(DuplicateEntityGroup {entity_type:row.get(0),canonical_hint:row.get(1),member_ids:row.get(2),reason:"exact_name".into()});
            }
        }
        EntitySimilarityStrategy::AliasChain => {
            for row in tx.query("SELECT canonical_id,array_agg(alias_id ORDER BY alias_id) FROM entity_aliases GROUP BY canonical_id ORDER BY count(*) DESC LIMIT $1", &[&(limit as i64)]).map_err(db_error)? {
                let id:String=row.get(0); let mut members:Vec<String>=row.get(1); members.push(id.clone());
                out.push(DuplicateEntityGroup {canonical_hint:id,member_ids:members,entity_type:"Unknown".into(),reason:"alias_chain".into()});
            }
        }
        EntitySimilarityStrategy::Vector {threshold,entity_type} => {
            if !(0.0..=1.0).contains(&threshold) {return Err("threshold must be in [0,1]".into());}
            if vectors::stamp_in(tx)?.is_none() {return Ok(out);}
            for row in tx.query("SELECT a.id,b.id,a.entity_type,va.embedding <-> vb.embedding FROM entities a JOIN entities b ON a.id<b.id AND a.entity_type=b.entity_type JOIN mem_vectors va ON va.mem_id=a.id JOIN mem_vectors vb ON vb.mem_id=b.id WHERE ($1::text IS NULL OR a.entity_type=$1) AND va.embedding <-> vb.embedding <= $2 ORDER BY 4,a.id,b.id LIMIT $3", &[&entity_type,&(2.0*(1.0-threshold)).sqrt(),&(limit as i64)]).map_err(db_error)? {
                let first:String=row.get(0); let second:String=row.get(1); let distance:f64=row.get(3);
                out.push(DuplicateEntityGroup {canonical_hint:first.clone(),member_ids:vec![first,second],entity_type:row.get(2),reason:format!("vector_similarity:{:.4}",1.0-distance*distance/2.0)});
            }
        }
    }
    Ok(out)
}

pub(crate) fn vector_cleanup(
    tx: &mut Transaction<'_>,
    thin: bool,
    dry_run: bool,
    actor: &str,
) -> StoreResult<RepairOrphanVectorsReport> {
    if vectors::stamp_in(tx)?.is_none() {
        return Ok(RepairOrphanVectorsReport {
            orphan_ids: Vec::new(),
            deleted: 0,
            dry_run,
        });
    }
    let predicate = if thin {
        "EXISTS(SELECT 1 FROM mem_items m WHERE (m.id=v.mem_id OR m.id=split_part(v.mem_id,'_chunk_',1)) AND m.body_blob_path IS NULL AND length(trim(coalesce(m.body_inline,'')))<$1)"
    } else {
        "NOT EXISTS(SELECT 1 FROM mem_items m WHERE m.id=v.mem_id OR m.id=split_part(v.mem_id,'_chunk_',1)) AND NOT EXISTS(SELECT 1 FROM entities e WHERE e.id=v.mem_id OR e.id=split_part(v.mem_id,'_chunk_',1))"
    };
    let sql = format!("SELECT mem_id FROM mem_vectors v WHERE {predicate} ORDER BY mem_id");
    let min = codeterm_mem::MIN_EMBED_BODY_CHARS as i64;
    let params: Vec<&(dyn postgres::types::ToSql + Sync)> = if thin { vec![&min] } else { vec![] };
    let ids: Vec<String> = tx
        .query(&sql, &params)
        .map_err(db_error)?
        .into_iter()
        .map(|row| row.get(0))
        .collect();
    let deleted = if dry_run || ids.is_empty() {
        0
    } else {
        let count = tx
            .execute("DELETE FROM mem_vectors WHERE mem_id=ANY($1)", &[&ids])
            .map_err(db_error)?;
        graph::audit_in(
            tx,
            actor,
            count,
            json!({"op":if thin {"prune_thin_vectors"} else {"repair_orphan_vectors"},"orphan_ids":ids}),
        )?;
        count
    };
    Ok(RepairOrphanVectorsReport {
        orphan_ids: ids,
        deleted,
        dry_run,
    })
}

pub(crate) fn edge_cleanup(
    tx: &mut Transaction<'_>,
    dream: bool,
    dry_run: bool,
    actor: &str,
) -> StoreResult<RepairOrphanVectorsReport> {
    let sql = if dream {
        "SELECT r.id FROM relations r JOIN mem_items m ON m.id=r.from_id AND m.kind='daily' JOIN entities e ON e.id=r.to_id WHERE r.valid_to IS NULL AND r.relation_type IN ('about','applies_to','works_on','postmortem_for','analyzes','mentions','discusses') AND r.to_id NOT IN ('project_dreams','scope_global') ORDER BY r.id"
    } else {
        "SELECT id FROM (SELECT id,row_number() OVER (PARTITION BY from_id,to_id ORDER BY CASE relation_type WHEN 'about' THEN 0 WHEN 'applies_to' THEN 1 WHEN 'belongs_to' THEN 2 WHEN 'part_of' THEN 3 WHEN 'documented_in' THEN 4 ELSE 9 END,id) AS rn FROM relations WHERE valid_to IS NULL AND relation_type IN ('about','applies_to','belongs_to','part_of','documented_in')) AS ranked WHERE rn>1 ORDER BY id"
    };
    let ids: Vec<i64> = tx
        .query(sql, &[])
        .map_err(db_error)?
        .into_iter()
        .map(|row| row.get(0))
        .collect();
    let deleted = if dry_run || ids.is_empty() {
        0
    } else {
        let count = tx
            .execute(
                "UPDATE relations SET valid_to=$2 WHERE id=ANY($1) AND valid_to IS NULL",
                &[&ids, &items::now()],
            )
            .map_err(db_error)?;
        graph::audit_in(
            tx,
            actor,
            count,
            json!({"op":if dream {"prune_dream_hub_edges"} else {"collapse_synonym_edges"},"rowids":ids}),
        )?;
        count
    };
    Ok(RepairOrphanVectorsReport {
        orphan_ids: ids.iter().map(ToString::to_string).collect(),
        deleted,
        dry_run,
    })
}

pub(crate) fn blob_cleanup(
    tx: &mut Transaction<'_>,
    dry_run: bool,
    actor: &str,
) -> StoreResult<RepairOrphanBlobsReport> {
    let names:Vec<String>=tx.query("SELECT name FROM mem_blobs b WHERE name LIKE 'sha256_%' AND NOT EXISTS(SELECT 1 FROM mem_items m WHERE m.body_blob_path=b.name) ORDER BY name", &[]).map_err(db_error)?.into_iter().map(|row|row.get(0)).collect();
    let deleted = if dry_run || names.is_empty() {
        0
    } else {
        let count = tx
            .execute("DELETE FROM mem_blobs WHERE name=ANY($1)", &[&names])
            .map_err(db_error)?;
        graph::audit_in(
            tx,
            actor,
            count,
            json!({"op":"repair_orphan_blobs","orphan_files":names}),
        )?;
        count
    };
    Ok(RepairOrphanBlobsReport {
        orphan_files: names,
        deleted,
        dry_run,
    })
}

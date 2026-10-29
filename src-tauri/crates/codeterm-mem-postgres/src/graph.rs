use crate::{db_error, items, vectors, StoreResult};
use codeterm_mem::{graph::TraversalResult, Entity, Relation};
use postgres::{Row, Transaction};
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) fn relation(row: Row) -> StoreResult<Relation> {
    Ok(Relation {
        from_id: row.get("from_id"),
        relation_type: row.get("relation_type"),
        to_id: row.get("to_id"),
        scope: row.get("scope"),
        valid_from: row.get("valid_from"),
        valid_to: row.get("valid_to"),
        confidence: row.get("confidence"),
        properties: serde_json::from_str(&row.get::<_, String>("properties_json"))
            .map_err(|_| "invalid relation properties")?,
        reason: row.get("reason"),
    })
}

fn merge_patch(target: &mut serde_json::Value, patch: serde_json::Value) {
    match patch {
        serde_json::Value::Object(patch) => {
            if !target.is_object() {
                *target = serde_json::json!({});
            }
            if let Some(object) = target.as_object_mut() {
                for (key, value) in patch {
                    if value.is_null() {
                        object.remove(&key);
                    } else {
                        merge_patch(object.entry(key).or_insert(serde_json::Value::Null), value);
                    }
                }
            }
        }
        value => *target = value,
    }
}

pub(crate) fn entity_in(tx: &mut Transaction<'_>, entity: &Entity) -> StoreResult<()> {
    let mut properties = tx
        .query_opt(
            "SELECT properties_json FROM entities WHERE id=$1",
            &[&entity.id],
        )
        .map_err(db_error)?
        .map(|r| serde_json::from_str::<serde_json::Value>(&r.get::<_, String>(0)))
        .transpose()
        .map_err(|_| "invalid entity properties")?
        .unwrap_or(serde_json::Value::Null);
    merge_patch(&mut properties, entity.properties.clone());
    tx.execute("INSERT INTO entities(id,entity_type,scope,properties_json,created_at,updated_at) VALUES ($1,$2,$3,$4,$5,$5) ON CONFLICT(id) DO UPDATE SET properties_json=excluded.properties_json,updated_at=excluded.updated_at",
        &[&entity.id,&entity.entity_type,&entity.scope,&properties.to_string(),&items::now()]).map_err(db_error)?;
    Ok(())
}

pub(crate) fn insert_in(
    tx: &mut Transaction<'_>,
    relation: &Relation,
    replace: bool,
) -> StoreResult<()> {
    if replace {
        tx.execute("UPDATE relations SET valid_to=$1 WHERE from_id=$2 AND relation_type=$3 AND to_id=$4 AND valid_to IS NULL",
            &[&items::now(),&relation.from_id,&relation.relation_type,&relation.to_id]).map_err(db_error)?;
    }
    tx.execute("INSERT INTO relations(from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at,reason) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT DO NOTHING",
        &[&relation.from_id,&relation.relation_type,&relation.to_id,&relation.scope,&relation.valid_from,&relation.valid_to,&relation.confidence,&relation.properties.to_string(),&items::now(),&relation.reason]).map_err(db_error)?;
    Ok(())
}

pub(crate) fn related_in(
    tx: &mut Transaction<'_>,
    id: &str,
    kind: Option<&str>,
    at: Option<&str>,
    include_invalidated: bool,
    incoming: bool,
) -> StoreResult<Vec<Relation>> {
    let sql = if incoming {
        "SELECT * FROM relations WHERE (from_id=$1 OR to_id=$1) AND ($2::text IS NULL OR relation_type=$2) AND ($3::boolean OR ($4::text IS NULL AND valid_to IS NULL) OR ($4 IS NOT NULL AND valid_from::timestamptz <= $4::timestamptz AND (valid_to IS NULL OR valid_to::timestamptz > $4::timestamptz))) ORDER BY valid_from DESC,created_at DESC,id"
    } else {
        "SELECT * FROM relations WHERE from_id=$1 AND ($2::text IS NULL OR relation_type=$2) AND ($3::boolean OR ($4::text IS NULL AND valid_to IS NULL) OR ($4 IS NOT NULL AND valid_from::timestamptz <= $4::timestamptz AND (valid_to IS NULL OR valid_to::timestamptz > $4::timestamptz))) ORDER BY valid_from DESC,created_at DESC,id"
    };
    tx.query(sql, &[&id, &kind, &include_invalidated, &at])
        .map_err(db_error)?
        .into_iter()
        .map(relation)
        .collect()
}

pub(crate) fn traverse_in(
    tx: &mut Transaction<'_>,
    start: &str,
    max_hops: usize,
    max_nodes: usize,
    at: Option<&str>,
    include_invalidated: bool,
) -> StoreResult<TraversalResult> {
    let mut visited = HashSet::from([start.to_owned()]);
    let mut frontier = VecDeque::from([(start.to_owned(), 0)]);
    let mut edges = Vec::new();
    let mut hops_used = 0;
    while let Some((node, hop)) = frontier.pop_front() {
        if hop >= max_hops || visited.len() >= max_nodes {
            continue;
        }
        hops_used = hops_used.max(hop + 1);
        for relation in related_in(tx, &node, None, at, include_invalidated, true)? {
            let neighbor = if relation.from_id == node {
                &relation.to_id
            } else {
                &relation.from_id
            };
            if visited.insert(neighbor.clone()) {
                frontier.push_back((neighbor.clone(), hop + 1));
            }
            edges.push(relation);
        }
    }
    Ok(TraversalResult {
        nodes: visited.into_iter().collect(),
        edges,
        hops_used,
    })
}

pub(crate) fn path_in(
    tx: &mut Transaction<'_>,
    from: &str,
    to: &str,
    max_hops: usize,
    include_invalidated: bool,
) -> StoreResult<Option<Vec<Relation>>> {
    if from == to {
        return Ok(Some(Vec::new()));
    }
    let mut visited = HashMap::<String, (String, Relation)>::new();
    let mut frontier = VecDeque::from([(from.to_owned(), 0)]);
    while let Some((node, hop)) = frontier.pop_front() {
        if hop >= max_hops {
            continue;
        }
        for relation in related_in(tx, &node, None, None, include_invalidated, true)? {
            let neighbor = if relation.from_id == node {
                relation.to_id.clone()
            } else {
                relation.from_id.clone()
            };
            if neighbor == from || visited.contains_key(&neighbor) {
                continue;
            }
            visited.insert(neighbor.clone(), (node.clone(), relation));
            if neighbor == to {
                let mut path = Vec::new();
                let mut current = to.to_owned();
                while let Some((previous, relation)) = visited.remove(&current) {
                    path.push(relation);
                    if previous == from {
                        break;
                    }
                    current = previous;
                }
                path.reverse();
                return Ok(Some(path));
            }
            frontier.push_back((neighbor, hop + 1));
        }
    }
    Ok(None)
}

pub(crate) fn audit_in(
    tx: &mut Transaction<'_>,
    actor: &str,
    count: u64,
    details: serde_json::Value,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO graph_purge_log(purged_at,count,actor,details_json) VALUES ($1,$2,$3,$4)",
        &[&items::now(), &(count as i64), &actor, &details.to_string()],
    )
    .map_err(db_error)?;
    Ok(())
}

pub(crate) fn alias_in(tx: &mut Transaction<'_>, alias: &str, canonical: &str) -> StoreResult<()> {
    if alias == canonical {
        return Err("alias cannot equal canonical".into());
    }
    tx.execute("INSERT INTO entity_aliases(alias_id,canonical_id,created_at) VALUES ($1,$2,$3) ON CONFLICT(alias_id) DO UPDATE SET canonical_id=excluded.canonical_id,created_at=excluded.created_at", &[&alias,&canonical,&items::now()]).map_err(db_error)?;
    let rows = tx
        .query(
            "SELECT * FROM relations WHERE from_id=$1 OR to_id=$1 ORDER BY id",
            &[&alias],
        )
        .map_err(db_error)?;
    let mut rewired_from = 0;
    let mut rewired_to = 0;
    let mut dropped = 0;
    for row in rows {
        let id: i64 = row.get("id");
        let mut edge = relation(row)?;
        if edge.from_id == alias {
            edge.from_id = canonical.into();
            rewired_from += 1;
        }
        if edge.to_id == alias {
            edge.to_id = canonical.into();
            rewired_to += 1;
        }
        let conflict: bool=tx.query_one("SELECT EXISTS(SELECT 1 FROM relations WHERE id<>$1 AND from_id=$2 AND relation_type=$3 AND to_id=$4 AND (valid_from=$5 OR (scope=$6 AND valid_to IS NULL AND $7::text IS NULL)))",
            &[&id,&edge.from_id,&edge.relation_type,&edge.to_id,&edge.valid_from,&edge.scope,&edge.valid_to]).map_err(db_error)?.get(0);
        if conflict {
            tx.execute("DELETE FROM relations WHERE id=$1", &[&id])
                .map_err(db_error)?;
            dropped += 1;
        } else {
            tx.execute(
                "UPDATE relations SET from_id=$2,to_id=$3 WHERE id=$1",
                &[&id, &edge.from_id, &edge.to_id],
            )
            .map_err(db_error)?;
        }
    }
    let props = |tx: &mut Transaction<'_>, id: &str| -> StoreResult<Option<serde_json::Value>> {
        tx.query_opt("SELECT properties_json FROM entities WHERE id=$1", &[&id])
            .map_err(db_error)?
            .map(|row| {
                serde_json::from_str(&row.get::<_, String>(0))
                    .map_err(|_| "invalid entity properties".into())
            })
            .transpose()
    };
    if let (Some(a), Some(mut c)) = (props(tx, alias)?, props(tx, canonical)?) {
        if let (Some(alias), Some(canon)) = (a.as_object(), c.as_object_mut()) {
            for (key, value) in alias {
                canon.entry(key.clone()).or_insert(value.clone());
            }
        }
        tx.execute(
            "UPDATE entities SET properties_json=$2,updated_at=$3 WHERE id=$1",
            &[&canonical, &c.to_string(), &items::now()],
        )
        .map_err(db_error)?;
    }
    tx.execute("DELETE FROM entities WHERE id=$1", &[&alias])
        .map_err(db_error)?;
    vectors::clear_in(tx, alias)?;
    audit_in(
        tx,
        "add_alias",
        rewired_from + rewired_to + dropped,
        serde_json::json!({"op":"add_alias","alias_id":alias,"canonical_id":canonical,"rewired_from":rewired_from,"rewired_to":rewired_to,"dropped_duplicate_relations":dropped}),
    )
}

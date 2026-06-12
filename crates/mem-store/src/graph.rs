//! Typed graph mutation API. All graph writes go through this module.

use rusqlite::Connection;
use serde::Serialize;

use crate::error::{MemStoreError, MemStoreResult};

#[derive(Debug, Clone)]
pub struct PurgeOptions {
    pub before: Option<String>,
    pub reason_filter: Option<String>,
    pub dry_run: bool,
    pub actor: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PurgeReport {
    pub count: u64,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct RouteOptions {
    pub from: String,
    pub to: String,
    pub via: String,
    pub rel_in: Option<String>,
    pub rel_out: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteReport {
    pub direct_invalidated: u64,
    pub edges_created: u64,
}

#[derive(Debug, Clone)]
pub struct ReviveBrokenOptions {
    pub since: Option<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviveBrokenReport {
    pub revived: u64,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct IntroduceTopicOptions {
    pub topic_id: String,
    pub entity_type: String,
    pub sources: Vec<String>,
    pub anchor: String,
    pub rel_source: String,
    pub rel_anchor: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntroduceTopicReport {
    pub topic_created: bool,
    pub edges_created: u64,
    pub edges_invalidated: u64,
}

pub fn purge_invalidated(conn: &Connection, opts: PurgeOptions) -> MemStoreResult<PurgeReport> {
    let mut sql = String::from("WHERE valid_to IS NOT NULL");
    let mut binds: Vec<String> = vec![];
    if let Some(b) = &opts.before {
        sql.push_str(&format!(
            " AND datetime(valid_to) < datetime(?{})",
            binds.len() + 1
        ));
        binds.push(b.clone());
    }
    if let Some(r) = &opts.reason_filter {
        sql.push_str(&format!(" AND reason LIKE ?{}", binds.len() + 1));
        binds.push(format!("%{}%", r));
    }

    let count_sql = format!("SELECT COUNT(*) FROM relations {}", sql);
    let count: u64 = {
        let mut stmt = conn.prepare(&count_sql)?;
        let bind_refs: Vec<&dyn rusqlite::ToSql> =
            binds.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        stmt.query_row(rusqlite::params_from_iter(bind_refs), |r| {
            r.get::<_, i64>(0)
        })? as u64
    };

    if !opts.dry_run && count > 0 {
        let tx = conn.unchecked_transaction()?;
        let del_sql = format!("DELETE FROM relations {}", sql);
        let bind_refs: Vec<&dyn rusqlite::ToSql> =
            binds.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        tx.execute(&del_sql, rusqlite::params_from_iter(bind_refs))?;
        crate::audit::record_purge(
            &tx,
            count,
            opts.before.as_deref(),
            opts.reason_filter.as_deref(),
            &opts.actor,
        )?;
        tx.commit()?;
    }

    Ok(PurgeReport {
        count,
        dry_run: opts.dry_run,
    })
}

pub fn route(conn: &Connection, opts: RouteOptions) -> MemStoreResult<RouteReport> {
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;

    let direct: Option<(String, String)> = tx
        .query_row(
            "SELECT relation_type, scope FROM relations
             WHERE from_id=?1 AND to_id=?2 AND valid_to IS NULL LIMIT 1",
            rusqlite::params![opts.from, opts.to],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .ok();

    let (orig_rel, scope) = direct.ok_or_else(|| {
        MemStoreError::NotFound(format!("no active direct edge {}->{}", opts.from, opts.to))
    })?;

    let rel_in = opts.rel_in.unwrap_or_else(|| orig_rel.clone());
    let rel_out = opts.rel_out.unwrap_or_else(|| orig_rel.clone());

    let direct_invalidated = tx.execute(
        "UPDATE relations SET valid_to=?1, reason=?2
         WHERE from_id=?3 AND to_id=?4 AND valid_to IS NULL",
        rusqlite::params![now, opts.reason, opts.from, opts.to],
    )? as u64;

    tx.execute(
        "INSERT OR IGNORE INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at, reason)
         VALUES (?1,?2,?3,?4,?5,NULL,1.0,'{}',?5,?6)",
        rusqlite::params![opts.from, rel_in, opts.via, scope, now, opts.reason],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at, reason)
         VALUES (?1,?2,?3,?4,?5,NULL,1.0,'{}',?5,?6)",
        rusqlite::params![opts.via, rel_out, opts.to, scope, now, opts.reason],
    )?;

    tx.commit()?;
    Ok(RouteReport {
        direct_invalidated,
        edges_created: 2,
    })
}

pub fn revive_broken(
    conn: &Connection,
    opts: ReviveBrokenOptions,
) -> MemStoreResult<ReviveBrokenReport> {
    let mut sql = String::from(
        "valid_to IS NOT NULL \
         AND from_id IN (SELECT id FROM entities UNION SELECT id FROM mem_items) \
         AND to_id IN (SELECT id FROM entities UNION SELECT id FROM mem_items)",
    );
    let mut binds: Vec<String> = vec![];
    let since_param = if opts.since.is_some() {
        Some(format!("?{}", binds.len() + 1))
    } else {
        None
    };
    if let Some(s) = &opts.since {
        sql.push_str(&format!(
            " AND datetime(valid_to) >= datetime(?{})",
            binds.len() + 1
        ));
        binds.push(s.clone());
    }
    // Never revive a closed relation when an active twin already exists for the
    // same (from, rel, to, scope): re-opening it (valid_to = NULL) would create
    // a second active row and violate the `idx_relations_active_unique` partial
    // index, crashing the dreamer normalize phase (#129/#116). One predicate,
    // shared by the COUNT and the UPDATE below, so the dry-run preview and the
    // real run always agree.
    sql.push_str(
        " AND NOT EXISTS (SELECT 1 FROM relations a WHERE a.valid_to IS NULL \
          AND a.from_id = relations.from_id AND a.relation_type = relations.relation_type \
          AND a.to_id = relations.to_id AND a.scope = relations.scope)",
    );
    // Prod can contain clusters of multiple CLOSED duplicate rows with the
    // same (from, rel, to, scope). Reviving every row in such a cluster would
    // make the closed copies collide with each other on the active-unique
    // partial index. Pick one deterministic winner per cluster: latest
    // valid_to, then latest valid_from, then highest rowid.
    sql.push_str(
        " AND relations.rowid = (SELECT c.rowid FROM relations c \
          WHERE c.valid_to IS NOT NULL \
          AND c.from_id = relations.from_id AND c.relation_type = relations.relation_type \
          AND c.to_id = relations.to_id AND c.scope = relations.scope \
          AND c.from_id IN (SELECT id FROM entities UNION SELECT id FROM mem_items) \
          AND c.to_id IN (SELECT id FROM entities UNION SELECT id FROM mem_items)",
    );
    if let Some(param) = &since_param {
        sql.push_str(&format!(" AND datetime(c.valid_to) >= datetime({})", param));
    }
    sql.push_str(
        " ORDER BY datetime(c.valid_to) DESC, datetime(c.valid_from) DESC, c.rowid DESC LIMIT 1)",
    );

    let count_sql = format!("SELECT COUNT(*) FROM relations WHERE {}", sql);
    let count: u64 = {
        let mut stmt = conn.prepare(&count_sql)?;
        let bind_refs: Vec<&dyn rusqlite::ToSql> =
            binds.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        stmt.query_row(rusqlite::params_from_iter(bind_refs), |r| {
            r.get::<_, i64>(0)
        })? as u64
    };

    if !opts.dry_run && count > 0 {
        let tx = conn.unchecked_transaction()?;
        let upd_sql = format!(
            "UPDATE relations SET valid_to = NULL, reason = COALESCE(reason, 'revive-broken') WHERE {}",
            sql
        );
        let bind_refs: Vec<&dyn rusqlite::ToSql> =
            binds.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        tx.execute(&upd_sql, rusqlite::params_from_iter(bind_refs))?;
        tx.commit()?;
    }

    Ok(ReviveBrokenReport {
        revived: count,
        dry_run: opts.dry_run,
    })
}

/// NOTE on embeddings: this function creates the topic entity row but does NOT
/// embed it into `mem_vectors` (this crate has no access to the embedder).
/// Callers running outside the daemon (CLI local dispatch) should follow up with
/// `codeterm mem reembed --scope entities` to backfill the new topic's vector,
/// or rely on the daemon's startup `backfill_entity_embeddings`. The Dreamer's
/// A.2 playbook does this automatically at the end of each cycle.
pub fn introduce_topic(
    conn: &Connection,
    opts: IntroduceTopicOptions,
) -> MemStoreResult<IntroduceTopicReport> {
    let now = chrono::Utc::now().to_rfc3339();
    let reason = opts
        .reason
        .clone()
        .unwrap_or_else(|| format!("introduce-topic-{}", opts.topic_id));

    let tx = conn.unchecked_transaction()?;

    let existing_type: Option<String> = tx
        .query_row(
            "SELECT entity_type FROM entities WHERE id=?1",
            rusqlite::params![opts.topic_id],
            |r| r.get::<_, String>(0),
        )
        .ok();
    let topic_created = existing_type.is_none();
    if let Some(et) = &existing_type {
        if et != &opts.entity_type {
            return Err(MemStoreError::Conflict(format!(
                "entity {} exists with type {} but caller requested {}",
                opts.topic_id, et, opts.entity_type
            )));
        }
    } else {
        tx.execute(
            "INSERT INTO entities (id, entity_type, scope, properties_json, created_at, updated_at)
             VALUES (?1, ?2, 'global', '{}', ?3, ?3)",
            rusqlite::params![opts.topic_id, opts.entity_type, now],
        )?;
    }

    let mut edges_invalidated: u64 = 0;
    let mut edges_created: u64 = 0;

    let scope: String = tx
        .query_row(
            "SELECT scope FROM entities WHERE id=?1",
            rusqlite::params![opts.anchor],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| "global".to_string());

    for source in &opts.sources {
        let invalidated = tx.execute(
            "UPDATE relations SET valid_to=?1, reason=?2
             WHERE from_id=?3 AND to_id=?4 AND valid_to IS NULL",
            rusqlite::params![now, reason, source, opts.anchor],
        )? as u64;
        edges_invalidated += invalidated;

        let created_src = tx.execute(
            "INSERT OR IGNORE INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, 1.0, '{}', ?5, ?6)",
            rusqlite::params![source, opts.rel_source, opts.topic_id, scope, now, reason],
        )? as u64;
        edges_created += created_src;
    }

    let topic_anchor_exists: i64 = tx.query_row(
        "SELECT COUNT(*) FROM relations WHERE from_id=?1 AND to_id=?2 AND valid_to IS NULL",
        rusqlite::params![opts.topic_id, opts.anchor],
        |r| r.get(0),
    )?;
    if topic_anchor_exists == 0 {
        let created_anchor = tx.execute(
            "INSERT OR IGNORE INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, 1.0, '{}', ?5, ?6)",
            rusqlite::params![opts.topic_id, opts.rel_anchor, opts.anchor, scope, now, reason],
        )? as u64;
        edges_created += created_anchor;
    }

    tx.commit()?;
    Ok(IntroduceTopicReport {
        topic_created,
        edges_created,
        edges_invalidated,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityDegree {
    pub id: String,
    pub entity_type: String,
    pub degree: u32,
}

/// Rename active relations matching `from` to `to` for each pair, then drop
/// any leftover `from`-typed rows (collisions skipped by UPDATE OR IGNORE).
/// Returns total rows touched (renamed + dropped).
pub fn canonicalize_relation_types(
    conn: &Connection,
    synonyms: &[(&str, &str)],
) -> MemStoreResult<u64> {
    let tx = conn.unchecked_transaction()?;
    let mut total: u64 = 0;
    for (from, to) in synonyms {
        let updated = tx.execute(
            "UPDATE OR IGNORE relations SET relation_type = ?1 \
             WHERE relation_type = ?2 AND valid_to IS NULL",
            rusqlite::params![to, from],
        )? as u64;
        let dropped = tx.execute(
            "DELETE FROM relations WHERE relation_type = ?1 AND valid_to IS NULL",
            rusqlite::params![from],
        )? as u64;
        total += updated + dropped;
    }
    tx.commit()?;
    Ok(total)
}

/// All entities with their active-relation degree (UNION counts both
/// from_id and to_id). Invalidated rows excluded.
pub fn list_entities_with_degree(conn: &Connection) -> MemStoreResult<Vec<EntityDegree>> {
    let mut stmt = conn.prepare(
        "SELECT id, entity_type, \
            (SELECT COUNT(*) FROM relations \
             WHERE valid_to IS NULL AND (from_id = entities.id OR to_id = entities.id)) \
         FROM entities ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(EntityDegree {
            id: r.get::<_, String>(0)?,
            entity_type: r.get::<_, String>(1)?,
            degree: r.get::<_, u32>(2)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Delete entities whose entity_type is NOT in `protected_types`, created_at
/// before `cutoff_iso`, and not referenced by any active relation.
pub fn cleanup_orphan_entities(
    conn: &Connection,
    protected_types: &[&str],
    cutoff_iso: &str,
) -> MemStoreResult<u64> {
    let placeholders = protected_types
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "DELETE FROM entities WHERE entity_type NOT IN ({}) \
         AND created_at < ?{} \
         AND id NOT IN (SELECT from_id FROM relations WHERE valid_to IS NULL) \
         AND id NOT IN (SELECT to_id FROM relations WHERE valid_to IS NULL)",
        placeholders,
        protected_types.len() + 1,
    );
    let mut sql_params: Vec<&dyn rusqlite::ToSql> = protected_types
        .iter()
        .map(|s| s as &dyn rusqlite::ToSql)
        .collect();
    sql_params.push(&cutoff_iso);
    let n = conn.execute(&sql, sql_params.as_slice())? as u64;
    Ok(n)
}

/// Mark active relations as invalid when either endpoint is missing from
/// both `entities` and `mem_items`. Endpoint may legitimately be a mem_item
/// id (fact_*, learning_*, ...) — only treat as broken when missing from both.
pub fn invalidate_broken_relations(conn: &Connection, now_iso: &str) -> MemStoreResult<u64> {
    let n = conn.execute(
        "UPDATE relations SET valid_to = ?1 \
         WHERE valid_to IS NULL \
         AND (from_id NOT IN (SELECT id FROM entities UNION SELECT id FROM mem_items) \
              OR to_id NOT IN (SELECT id FROM entities UNION SELECT id FROM mem_items))",
        rusqlite::params![now_iso],
    )? as u64;
    Ok(n)
}

/// Bumps `evidence_count` for the given mem_item by 1. Returns `Ok(true)`
/// when a row was updated, `Ok(false)` when no row matched. Best-effort —
/// callers are observer code that must not crash when the row went away.
pub fn bump_evidence_count(conn: &Connection, mem_id: &str) -> MemStoreResult<bool> {
    let n = conn.execute(
        "UPDATE mem_items SET evidence_count = evidence_count + 1 WHERE id = ?1",
        rusqlite::params![mem_id],
    )?;
    Ok(n > 0)
}

/// Bumps `false_fire_count` for the given mem_item by 1. Same semantics
/// as `bump_evidence_count` — returns `Ok(false)` on missing row.
pub fn bump_false_fire_count(conn: &Connection, mem_id: &str) -> MemStoreResult<bool> {
    let n = conn.execute(
        "UPDATE mem_items SET false_fire_count = false_fire_count + 1 WHERE id = ?1",
        rusqlite::params![mem_id],
    )?;
    Ok(n > 0)
}

/// Record `alias_id -> canonical_id`, rewire any relations pointing at the
/// alias (UPDATE OR IGNORE so collisions get dropped), then delete leftover
/// alias-referencing relations and the alias entity row.
pub fn add_alias_with_rewire(
    conn: &Connection,
    alias_id: &str,
    canonical_id: &str,
) -> MemStoreResult<()> {
    if alias_id == canonical_id {
        return Err(MemStoreError::InvalidArgument(
            "alias cannot equal canonical".into(),
        ));
    }
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT OR REPLACE INTO entity_aliases (alias_id, canonical_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![alias_id, canonical_id, &now],
    )?;
    let rewired_from = tx.execute(
        "UPDATE OR IGNORE relations SET from_id = ?1 WHERE from_id = ?2",
        rusqlite::params![canonical_id, alias_id],
    )? as u64;
    let rewired_to = tx.execute(
        "UPDATE OR IGNORE relations SET to_id = ?1 WHERE to_id = ?2",
        rusqlite::params![canonical_id, alias_id],
    )? as u64;
    let dropped_dupes = tx.execute(
        "DELETE FROM relations WHERE from_id = ?1 OR to_id = ?1",
        rusqlite::params![alias_id],
    )? as u64;

    let alias_props: Option<String> = tx
        .query_row(
            "SELECT properties_json FROM entities WHERE id = ?1",
            rusqlite::params![alias_id],
            |r| r.get(0),
        )
        .ok();
    let canonical_props: Option<String> = tx
        .query_row(
            "SELECT properties_json FROM entities WHERE id = ?1",
            rusqlite::params![canonical_id],
            |r| r.get(0),
        )
        .ok();
    if let (Some(a), Some(c)) = (alias_props, canonical_props) {
        let alias_obj = serde_json::from_str::<serde_json::Value>(&a)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let mut canon_obj = serde_json::from_str::<serde_json::Value>(&c)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        for (k, v) in alias_obj {
            canon_obj.entry(k).or_insert(v);
        }
        let merged = serde_json::to_string(&serde_json::Value::Object(canon_obj))
            .expect("Map<String, Value> serialization is infallible");
        tx.execute(
            "UPDATE entities SET properties_json = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![merged, &now, canonical_id],
        )?;
    }

    tx.execute(
        "DELETE FROM entities WHERE id = ?1",
        rusqlite::params![alias_id],
    )?;
    let _ = tx.execute(
        "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
        rusqlite::params![alias_id, format!("{}_chunk_%", alias_id)],
    );

    let details = serde_json::json!({
        "op": "add_alias",
        "alias_id": alias_id,
        "canonical_id": canonical_id,
        "rewired_from": rewired_from,
        "rewired_to": rewired_to,
        "dropped_duplicate_relations": dropped_dupes,
    })
    .to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, NULL, NULL, 'add_alias', ?3)",
        rusqlite::params![&now, (rewired_from + rewired_to + dropped_dupes) as i64, details],
    )?;
    tx.commit()?;
    Ok(())
}

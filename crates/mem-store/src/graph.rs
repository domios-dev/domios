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
        sql.push_str(&format!(" AND datetime(valid_to) < datetime(?{})", binds.len() + 1));
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
        stmt.query_row(rusqlite::params_from_iter(bind_refs), |r| r.get::<_, i64>(0))? as u64
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

    Ok(PurgeReport { count, dry_run: opts.dry_run })
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
        MemStoreError::NotFound(format!(
            "no active direct edge {}->{}",
            opts.from, opts.to
        ))
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
    Ok(RouteReport { direct_invalidated, edges_created: 2 })
}

pub fn revive_broken(conn: &Connection, opts: ReviveBrokenOptions) -> MemStoreResult<ReviveBrokenReport> {
    let mut sql = String::from(
        "valid_to IS NOT NULL \
         AND from_id IN (SELECT id FROM entities UNION SELECT id FROM mem_items) \
         AND to_id IN (SELECT id FROM entities UNION SELECT id FROM mem_items)",
    );
    let mut binds: Vec<String> = vec![];
    if let Some(s) = &opts.since {
        sql.push_str(&format!(" AND datetime(valid_to) >= datetime(?{})", binds.len() + 1));
        binds.push(s.clone());
    }

    let count_sql = format!("SELECT COUNT(*) FROM relations WHERE {}", sql);
    let count: u64 = {
        let mut stmt = conn.prepare(&count_sql)?;
        let bind_refs: Vec<&dyn rusqlite::ToSql> =
            binds.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        stmt.query_row(rusqlite::params_from_iter(bind_refs), |r| r.get::<_, i64>(0))? as u64
    };

    if !opts.dry_run && count > 0 {
        let upd_sql = format!(
            "UPDATE relations SET valid_to = NULL, reason = COALESCE(reason, 'revive-broken') WHERE {}",
            sql
        );
        let bind_refs: Vec<&dyn rusqlite::ToSql> =
            binds.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        conn.execute(&upd_sql, rusqlite::params_from_iter(bind_refs))?;
    }

    Ok(ReviveBrokenReport { revived: count, dry_run: opts.dry_run })
}

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

    let already: i64 = tx.query_row(
        "SELECT COUNT(*) FROM entities WHERE id=?1",
        rusqlite::params![opts.topic_id],
        |r| r.get(0),
    )?;
    let topic_created = already == 0;
    if topic_created {
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

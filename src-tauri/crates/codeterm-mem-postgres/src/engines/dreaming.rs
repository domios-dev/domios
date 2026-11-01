use super::*;
use chrono::{DateTime, Duration as Days, Utc};
use serde_json::{json, Value};

pub(super) fn candidates(
    tx: &mut Transaction<'_>,
    before: &str,
) -> Result<Vec<LightCandidate>, String> {
    Ok(tx.query("SELECT id,access_count,importance,created_at,accessed_at FROM mem_items WHERE kind='learning' AND access_count>=3 AND created_at<$1", &[&before]).map_err(db_error)?.into_iter().map(|r|(r.get(0),r.get(1),r.get(2),r.get(3),r.get(4))).collect())
}
pub(super) fn promote(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    c: &LightCandidate,
    now: DateTime<Utc>,
    refuse: bool,
) -> Result<bool, String> {
    let neighbor = tx
        .query_opt("SELECT author_kind FROM mem_items WHERE id=$1", &[&c.0])
        .map_err(db_error)?
        .is_some_and(|r| r.get::<_, String>(0) == "neighbor");
    if neighbor {
        return if refuse {
            Err("neighbor content cannot be promoted".into())
        } else {
            Ok(false)
        };
    }
    let frequency = (c.1 as f64 / 10.0).min(1.0);
    let recency =
        c.4.as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| {
                let days = (now - t.with_timezone(&Utc)).num_hours().max(0) as f64 / 24.0;
                (-days * 0.693 / 14.0).exp()
            })
            .unwrap_or(0.0);
    let age = DateTime::parse_from_rfc3339(&c.3)
        .ok()
        .map(|t| {
            let days = (now - t.with_timezone(&Utc)).num_hours().max(0) as f64 / 24.0;
            (days / 7.0).min(1.0)
        })
        .unwrap_or(0.5);
    if frequency * 0.30 + recency * 0.25 + (c.2 / 5.0).min(1.0) * 0.25 + age * 0.20 < 0.5 {
        return Ok(false);
    }
    let changed=tx.execute("UPDATE mem_items SET kind='fact',importance=greatest(importance,4.0),updated_at=$1 WHERE id=$2 AND author_kind<>'neighbor'", &[&crate::items::now(),&c.0]).map_err(db_error)?;
    if changed > 0 {
        let ctx = ctx.with_author(ctx.default_system_author("dreaming:rem"));
        crate::items::history_in(
            tx,
            &ctx,
            &c.0,
            "promote",
            &codeterm_mem::store::HistorySnapshot {
                importance: Some(c.2),
                kind: Some("learning".into()),
                ..Default::default()
            },
            &codeterm_mem::store::HistorySnapshot {
                importance: Some(c.2.max(4.0)),
                kind: Some("fact".into()),
                ..Default::default()
            },
            None,
        )?;
    }
    Ok(changed > 0)
}
pub(super) fn expire(tx: &mut Transaction<'_>, kind: &str, days: i64) -> Result<u64, String> {
    let cutoff = (Utc::now() - Days::days(days)).to_rfc3339();
    tx.execute("UPDATE mem_items SET importance=0.0,updated_at=$1 WHERE kind=$2 AND access_count=0 AND created_at<$3 AND importance>0.0 AND importance<4.0", &[&crate::items::now(),&kind,&cutoff]).map_err(db_error)
}
pub(super) fn worker(
    tx: &mut Transaction<'_>,
    _ctx: &WriteContext,
    kind: &str,
    machine: &str,
    timeout: Duration,
) -> Result<Value, String> {
    let configured: i64 = tx
        .query_one(
            "SELECT setting::bigint FROM pg_settings WHERE name='statement_timeout'",
            &[],
        )
        .map_err(db_error)?
        .get(0);
    let timeout = timeout
        .as_millis()
        .min(configured.max(1) as u128)
        .max(1)
        .to_string();
    tx.query_one(
        "SELECT set_config('statement_timeout',$1,true)",
        &[&timeout],
    )
    .map_err(db_error)?;
    match kind {
        "dreaming-light" => {
            let now = Utc::now();
            let candidates = candidates(tx, &(now - Days::days(1)).to_rfc3339())?;
            let mut promoted = 0;
            for c in &candidates {
                if promote(tx, &WriteContext::system("dreaming:rem"), c, now, false)? {
                    promoted += 1;
                }
            }
            let mut lessons = 0;
            let mut already = 0;
            for r in tx.query("SELECT id,tags_json FROM mem_items WHERE kind='lesson' AND author_kind<>'neighbor' AND false_fire_count=0 AND evidence_count>=coalesce(promotion_threshold,10)", &[]).map_err(db_error)? {
                let id:String=r.get(0); let mut tags=serde_json::from_str::<Vec<String>>(&r.get::<_,String>(1)).unwrap_or_default();
                if tags.iter().any(|t|t=="high-confidence") { already+=1; continue; }
                tags.push("high-confidence".into());
                tx.execute("UPDATE mem_items SET tags_json=$1,updated_at=$2 WHERE id=$3", &[&serde_json::to_string(&tags).map_err(|e|e.to_string())?,&crate::items::now(),&id]).map_err(db_error)?;
                tx.execute("INSERT INTO lesson_audit(lesson_uid,machine_id,action,ts,reason) VALUES ($1,$2,'promoted',$3,'auto: evidence_count threshold met')", &[&id,&machine,&crate::items::now()]).map_err(db_error)?;
                lessons+=1;
            }
            Ok(
                json!({"considered":candidates.len(),"promoted":promoted,"dropped":0,"phase":"light","lessons_promoted":lessons,"lessons_already_high_confidence":already}),
            )
        }
        "dreaming-deep" => {
            let mut dropped = 0;
            for (kind, days) in codeterm_mem_store::maintenance::DEEP_TIERS {
                dropped += expire(tx, kind, *days)?;
            }
            Ok(
                json!({"considered":dropped,"promoted":0,"dropped":dropped,"phase":"deep","lessons_promoted":0,"lessons_already_high_confidence":0}),
            )
        }
        "dreaming-normalize" => normalize(tx),
        other => Err(format!("unknown worker kind: {other}")),
    }
}
pub(super) fn normalize(tx: &mut Transaction<'_>) -> Result<Value, String> {
    let started = std::time::Instant::now();
    let millis: i64 = tx
        .query_one(
            "SELECT setting::bigint FROM pg_settings WHERE name='statement_timeout'",
            &[],
        )
        .map_err(db_error)?
        .get(0);
    let mut canonicalized = 0;
    for (from, to) in [
        ("references", "mentions"),
        ("refers_to", "mentions"),
        ("contributes_to", "works_on"),
        ("at_location", "located_at"),
    ] {
        let rows=tx.query("SELECT id,from_id,to_id,scope,valid_from FROM relations WHERE relation_type=$1 AND valid_to IS NULL ORDER BY id", &[&from]).map_err(db_error)?;
        for r in rows {
            let conflict:bool=tx.query_one("SELECT EXISTS(SELECT 1 FROM relations WHERE from_id=$1 AND to_id=$2 AND relation_type=$3 AND (valid_from=$4 OR (scope=$5 AND valid_to IS NULL)))", &[&r.get::<_,String>(1),&r.get::<_,String>(2),&to,&r.get::<_,String>(4),&r.get::<_,String>(3)]).map_err(db_error)?.get(0);
            let id: i64 = r.get(0);
            canonicalized += if conflict {
                tx.execute("DELETE FROM relations WHERE id=$1", &[&id])
            } else {
                tx.execute(
                    "UPDATE relations SET relation_type=$1 WHERE id=$2",
                    &[&to, &id],
                )
            }
            .map_err(db_error)?;
        }
    }
    let mut buckets =
        std::collections::HashMap::<(String, String), Vec<(String, String, i64)>>::new();
    for r in tx.query("SELECT id,entity_type,(SELECT count(*) FROM relations WHERE valid_to IS NULL AND (from_id=e.id OR to_id=e.id)) FROM entities e ORDER BY id", &[]).map_err(db_error)? {
        let id:String=r.get(0);
        if let Some((prefix,suffix))=id.split_once('_') {
            if suffix.chars().count()>=2 { buckets.entry((r.get(1),prefix.into())).or_default().push((id.clone(),suffix.into(),r.get(2))); }
        }
    }
    let mut merged = 0;
    let mut flagged = Vec::new();
    for entries in buckets.values() {
        for i in 0..entries.len() {
            for j in i + 1..entries.len() {
                if started.elapsed() >= Duration::from_millis(millis.max(1) as u64) {
                    return Err("Postgres memory maintenance timed out".into());
                }
                let (a, sa, da) = &entries[i];
                let (b, sb, db) = &entries[j];
                let ratio = ratio(sa, sb);
                if ratio < 0.70 {
                    continue;
                }
                if ratio >= 0.85 && da != db {
                    let (alias, canonical) = if da < db { (a, b) } else { (b, a) };
                    tx.batch_execute("SAVEPOINT normalize_alias")
                        .map_err(db_error)?;
                    if crate::graph::alias_in(tx, alias, canonical).is_ok() {
                        merged += 1;
                    } else {
                        tx.batch_execute("ROLLBACK TO normalize_alias")
                            .map_err(db_error)?;
                    }
                    tx.batch_execute("RELEASE normalize_alias")
                        .map_err(db_error)?;
                } else {
                    flagged.push(json!({"a":a,"b":b,"ratio":ratio}));
                }
            }
        }
    }
    let cutoff = (Utc::now() - Days::days(14)).to_rfc3339();
    let orphans=tx.execute("DELETE FROM entities WHERE entity_type NOT IN ('kind','area','agent','workspace') AND created_at<$1 AND id NOT IN(SELECT from_id FROM relations WHERE valid_to IS NULL) AND id NOT IN(SELECT to_id FROM relations WHERE valid_to IS NULL)", &[&cutoff]).map_err(db_error)?;
    let broken=tx.execute("UPDATE relations SET valid_to=$1 WHERE valid_to IS NULL AND (from_id NOT IN(SELECT id FROM entities UNION SELECT id FROM mem_items) OR to_id NOT IN(SELECT id FROM entities UNION SELECT id FROM mem_items))", &[&crate::items::now()]).map_err(db_error)?;
    let revived=tx.execute("WITH candidates AS (SELECT r.id,row_number() OVER (PARTITION BY r.from_id,r.relation_type,r.to_id,r.scope ORDER BY CASE WHEN pg_input_is_valid(r.valid_to,'timestamp with time zone') THEN r.valid_to::timestamptz END DESC NULLS LAST,CASE WHEN pg_input_is_valid(r.valid_from,'timestamp with time zone') THEN r.valid_from::timestamptz END DESC NULLS LAST,r.id DESC) AS rank FROM relations r WHERE r.valid_to IS NOT NULL AND r.from_id IN(SELECT id FROM entities UNION SELECT id FROM mem_items) AND r.to_id IN(SELECT id FROM entities UNION SELECT id FROM mem_items) AND NOT EXISTS(SELECT 1 FROM relations a WHERE a.valid_to IS NULL AND a.from_id=r.from_id AND a.to_id=r.to_id AND a.relation_type=r.relation_type AND a.scope=r.scope)) UPDATE relations SET valid_to=NULL,reason=coalesce(reason,'revive-broken') WHERE id IN(SELECT id FROM candidates WHERE rank=1)", &[]).map_err(db_error)?;
    Ok(
        json!({"phase":"normalize","merged":merged,"flagged_entity_pairs":flagged,"relations_canonicalized":canonicalized,"orphans_deleted":orphans,"broken_relations_invalidated":broken,"broken_relations_revived":revived}),
    )
}
fn ratio(a: &str, b: &str) -> f64 {
    let a: Vec<_> = a.chars().collect();
    let b: Vec<_> = b.chars().collect();
    let mut prev: Vec<_> = (0..=b.len()).collect();
    let mut next = prev.clone();
    for (i, a) in a.iter().enumerate() {
        next[0] = i + 1;
        for (j, b) in b.iter().enumerate() {
            next[j + 1] = (prev[j + 1] + 1)
                .min(next[j] + 1)
                .min(prev[j] + usize::from(a != b));
        }
        std::mem::swap(&mut prev, &mut next);
    }
    let len = a.len().max(b.len());
    if len == 0 {
        1.0
    } else {
        1.0 - prev[b.len()] as f64 / len as f64
    }
}

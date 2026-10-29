use crate::db_error;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use codeterm_mem::{
    backend::WriteContext,
    curation::calibration::stale::{utc_day_start, StaleSelection, STALE_ACTOR},
};
use codeterm_types::config::DreamHygieneConfig;
use postgres::Transaction;
use serde_json::{json, Value};
const PREDICATE: &str = include_str!("stale_predicate.sql");
pub(super) fn run(
    tx: &mut Transaction<'_>,
    cfg: &DreamHygieneConfig,
    now: DateTime<Utc>,
    dry: bool,
    limit: Option<usize>,
) -> Result<Value, String> {
    if !dry && !cfg.stale_archive {
        return Ok(
            json!({"enabled":false,"dry_run":false,"skipped_reason":"disabled","archived":[]}),
        );
    }
    let cutoff = now - Duration::days(365);
    let cutoff_text = cutoff.to_rfc3339_opts(SecondsFormat::Secs, true);
    let cap = i64::from(cfg.stale_archive_cap);
    let done:i64=tx.query_one("SELECT count(*) FROM mem_item_history WHERE changed_by=$1 AND CASE WHEN pg_input_is_valid(changed_at,'timestamp with time zone') THEN changed_at::timestamptz END >=$2::text::timestamptz", &[&STALE_ACTOR,&utc_day_start(now).to_rfc3339()]).map_err(db_error)?.get(0);
    let remaining = (cap - done).max(0) as usize;
    let matching: i64 = tx
        .query_one(
            &format!("SELECT count(*) FROM mem_items m WHERE {PREDICATE}"),
            &[&cutoff_text],
        )
        .map_err(db_error)?
        .get(0);
    let mut report = json!({"enabled":cfg.stale_archive,"dry_run":dry,"cutoff":cutoff_text,"cap":cap,"archived_today":done,"matching":matching});
    if !dry && remaining == 0 {
        report["skipped_reason"] = "nightly_cap".into();
        report["archived"] = json!([]);
        return Ok(report);
    }
    let limit = if dry {
        limit.unwrap_or(remaining).min(1000)
    } else {
        remaining
    };
    let sql=format!("SELECT m.id,m.kind,m.importance,m.created_at,greatest(m.updated_at,coalesce(m.accessed_at,'')),(SELECT count(*) FROM relations r WHERE (r.from_id=m.id OR r.to_id=m.id) AND r.valid_to IS NULL) FROM mem_items m WHERE {PREDICATE} ORDER BY m.created_at::timestamptz,m.id LIMIT $2");
    let selected:Vec<_>=tx.query(&sql,&[&cutoff_text,&(limit as i64)]).map_err(db_error)?.into_iter().map(|r| {
        let created_at:String=r.get(3);let last_touch:String=r.get(4);let active_neighbours:i64=r.get(5);
        let reason=format!("stale: created {created_at}, last direct touch {last_touch}, no relation change and no touched neighbour (of {active_neighbours}) since {cutoff_text}");
        StaleSelection {id:r.get(0),kind:r.get(1),importance:r.get(2),created_at,last_touch,active_neighbours,reason}
    }).collect();
    if dry {
        report["selected"] = serde_json::to_value(selected).map_err(|e| e.to_string())?;
        return Ok(report);
    }
    for item in &selected {
        super::calibration::archive(
            tx,
            &WriteContext::system(STALE_ACTOR),
            &item.id,
            &item.kind,
            Some(&item.reason),
        )?;
    }
    report["archived"] = selected
        .iter()
        .map(|i| json!({"id":i.id,"reason":i.reason}))
        .collect();
    Ok(report)
}

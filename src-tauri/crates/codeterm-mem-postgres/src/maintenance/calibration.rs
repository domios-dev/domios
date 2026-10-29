use crate::{db_error, items};
use chrono::{Datelike, Duration, Utc};
use codeterm_mem::{backend::WriteContext, store::HistorySnapshot};
use postgres::Transaction;
use serde_json::{json, Value};
fn sqlite_time(days: i64) -> String {
    (Utc::now() - Duration::days(days))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}
pub(super) fn archive(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    id: &str,
    kind: &str,
    reason: Option<&str>,
) -> Result<(), String> {
    let changed = tx
        .execute(
            "UPDATE mem_items SET kind='archived',updated_at=$1 WHERE id=$2 AND kind<>'archived'",
            &[&items::now(), &id],
        )
        .map_err(db_error)?;
    if changed == 0 {
        return Err(format!("archive {id}: item missing or already archived"));
    }
    items::history_in(
        tx,
        ctx,
        id,
        "archive",
        &HistorySnapshot {
            kind: Some(kind.into()),
            ..Default::default()
        },
        &HistorySnapshot {
            kind: Some("archived".into()),
            ..Default::default()
        },
        reason,
    )
}
pub(super) fn sweep(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    force: bool,
) -> Result<Value, String> {
    let dow = Utc::now().weekday().num_days_from_sunday().to_string();
    let skipped = |reason| json!({"day_of_week":dow,"examined":0,"applied":0,"archived":0,"skipped_reason":reason});
    if !force && dow != "0" {
        return Ok(skipped("not_sunday"));
    }
    let since = (Utc::now() - Duration::hours(6))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let recent:i64=tx.query_one("SELECT count(*) FROM mem_item_history WHERE changed_by LIKE 'dreamer-calibration%' AND changed_by<>'dreamer-calibration:stale' AND changed_at>=$1", &[&since]).map_err(db_error)?.get(0);
    if recent > 0 {
        return Ok(skipped("recent_run"));
    }
    let rows=tx.query("SELECT id,kind,importance,CASE WHEN evidence_count=1 AND importance<=2 AND created_at<$1 AND kind IN ('fact','note') THEN 'archive' WHEN author_kind<>'neighbor' AND evidence_count>=10 AND importance<4 AND accessed_at>$2 THEN 'promote' WHEN evidence_count<=1 AND importance>=4 AND created_at<$3 THEN 'demote' ELSE 'none' END AS action FROM mem_items WHERE is_pinned_wakeup=0 AND updated_at<$4 AND ((author_kind<>'neighbor' AND evidence_count>=10 AND importance<4 AND accessed_at>$2) OR (evidence_count<=1 AND importance>=4 AND created_at<$3) OR (evidence_count=1 AND importance<=2 AND created_at<$1 AND kind IN ('fact','note'))) LIMIT 200", &[&sqlite_time(90),&sqlite_time(30),&sqlite_time(60),&sqlite_time(14)]).map_err(db_error)?;
    if rows.len() > 100 {
        let mut report = skipped("too_many_candidates");
        report["candidate_count"] = json!(rows.len());
        return Ok(report);
    }
    let mut applied = 0;
    let mut archived = Vec::new();
    let mut examined = 0;
    for row in rows {
        if applied + archived.len() >= 30 {
            break;
        }
        examined += 1;
        let id: String = row.get(0);
        let kind: String = row.get(1);
        let importance: f64 = row.get(2);
        let action: String = row.get(3);
        match action.as_str() {
            "promote" | "demote" => {
                let next = if action == "promote" {
                    (importance + 0.5).min(5.0)
                } else {
                    (importance - 0.5).max(0.0)
                };
                tx.execute(
                    "UPDATE mem_items SET importance=$1,updated_at=$2 WHERE id=$3",
                    &[&next, &sqlite_time(0), &id],
                )
                .map_err(db_error)?;
                items::history_in(
                    tx,
                    &WriteContext::system(&format!("dreamer-calibration:{action}")),
                    &id,
                    "update",
                    &HistorySnapshot {
                        importance: Some(importance),
                        ..Default::default()
                    },
                    &HistorySnapshot {
                        importance: Some(next),
                        ..Default::default()
                    },
                    None,
                )?;
                applied += 1;
            }
            "archive" => {
                let ctx = ctx
                    .with_author(ctx.default_system_author("calibration"))
                    .with_changed_by("dreamer-calibration:archive");
                archive(tx, &ctx, &id, &kind, None)?;
                archived.push(id);
            }
            _ => {}
        }
    }
    Ok(
        json!({"day_of_week":dow,"examined":examined,"applied":applied,"archived":archived.len(),"archived_ids":archived}),
    )
}
pub(super) fn log(
    tx: &mut Transaction<'_>,
    since: &str,
    limit: usize,
    count_only: bool,
) -> Result<Value, String> {
    if count_only {
        let count:i64=tx.query_one("SELECT count(*) FROM mem_item_history WHERE changed_by LIKE 'dreamer-calibration%' AND changed_at>=$1", &[&since]).map_err(db_error)?.get(0);
        return Ok(json!({"count":count}));
    }
    let rows=tx.query("SELECT item_id,changed_at,changed_by,change_type,old_importance,new_importance,old_kind,new_kind,reason FROM mem_item_history WHERE changed_by LIKE 'dreamer-calibration%' AND changed_at>=$1 ORDER BY changed_at DESC LIMIT $2", &[&since,&(limit as i64)]).map_err(db_error)?;
    let entries:Vec<_>=rows.into_iter().map(|r|json!({"item_id":r.get::<_,String>(0),"changed_at":r.get::<_,String>(1),"changed_by":r.get::<_,String>(2),"change_type":r.get::<_,String>(3),"old_importance":r.get::<_,Option<f64>>(4),"new_importance":r.get::<_,Option<f64>>(5),"old_kind":r.get::<_,Option<String>>(6),"new_kind":r.get::<_,Option<String>>(7),"reason":r.get::<_,Option<String>>(8)})).collect();
    Ok(json!({"entries":entries}))
}

use crate::{db_error, graph, items, PostgresMemStore, StoreResult};
use codeterm_mem::{
    backend::WriteContext,
    curation::merge::{self, MergeError, MergeRequest},
    store::{area_merge::AreaMergeOutcome, HistorySnapshot, MemUpdate},
    MemKind,
};
use postgres::Transaction;
use std::collections::BTreeMap;

pub(crate) fn archive_in(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    id: &str,
    actor: &str,
    change: &str,
    reason: Option<&str>,
) -> StoreResult<String> {
    let item = items::get_in(tx, id)?.ok_or("memory item not found")?;
    if item.kind == "archived" {
        return Err("item is already archived".into());
    }
    tx.execute(
        "UPDATE mem_items SET kind='archived',updated_at=$2 WHERE id=$1",
        &[&id, &items::now()],
    )
    .map_err(db_error)?;
    items::history_in(
        tx,
        &ctx.with_changed_by(actor),
        id,
        change,
        &HistorySnapshot {
            kind: Some(item.kind.clone()),
            ..Default::default()
        },
        &HistorySnapshot {
            kind: Some("archived".into()),
            ..Default::default()
        },
        reason,
    )?;
    Ok(item.kind)
}

pub(crate) fn area_in(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    id: &str,
    into: &str,
    reason: Option<&str>,
) -> StoreResult<bool> {
    let Some(item) = items::get_in(tx, id)? else {
        return Ok(false);
    };
    if item.area.as_deref() == Some(into) {
        return Ok(false);
    }
    tx.execute(
        "UPDATE mem_items SET area=$2,updated_at=$3,seed_fingerprint=NULL WHERE id=$1",
        &[&id, &into, &items::now()],
    )
    .map_err(db_error)?;
    let encoded = format!(
        "area_update={}",
        serde_json::json!({"from":item.area,"into":into})
    );
    items::history_in(
        tx,
        ctx,
        id,
        "update",
        &HistorySnapshot::default(),
        &HistorySnapshot::default(),
        Some(reason.unwrap_or(&encoded)),
    )?;
    Ok(true)
}

pub(crate) fn merge_area_in(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    from: &str,
    into: &str,
    dry_run: bool,
    dream_run: Option<&str>,
) -> StoreResult<AreaMergeOutcome> {
    codeterm_mem::store::area_merge::validate_areas(from, into)?;
    let rows = tx
        .query(
            "SELECT id,kind FROM mem_items WHERE area=$1 ORDER BY id",
            &[&from],
        )
        .map_err(db_error)?;
    if rows.is_empty() {
        return Err(format!("source area {from} is empty or does not exist"));
    }
    let unsafe_target: bool=tx.query_one("SELECT EXISTS(SELECT 1 FROM mem_items WHERE area=$1 AND author_kind='neighbor') AND EXISTS(SELECT 1 FROM mem_items WHERE area=$2 AND (kind IN ('rule','preference','skill') OR is_pinned_wakeup!=0))", &[&from,&into]).map_err(db_error)?.get(0);
    if unsafe_target {
        return Err("neighbor area cannot merge into an instruction or pinned target".into());
    }
    let mut counts = BTreeMap::new();
    let mut item_ids = Vec::new();
    for row in rows {
        *counts.entry(row.get::<_, String>(1)).or_insert(0) += 1;
        item_ids.push(row.get::<_, String>(0));
    }
    if !dry_run {
        if let Some(run) = dream_run {
            let count: i64=tx.query_one("SELECT count(DISTINCT (substr(reason,12)::jsonb)->>'operation') FROM mem_item_history WHERE left(reason,11)='area_merge=' AND (substr(reason,12)::jsonb)->>'dream_run'=$1", &[&run]).map_err(db_error)?.get(0);
            if count >= 3 {
                return Err("dream run area merge cap reached (3)".into());
            }
        }
        let reason = format!(
            "area_merge={}",
            serde_json::json!({"from":from,"into":into,"dream_run":dream_run,"operation":ulid::Ulid::new().to_string()})
        );
        for id in &item_ids {
            area_in(tx, ctx, id, into, Some(&reason))?;
        }
        if let Some(run) = dream_run {
            let summary=format!("Area merge {from:?} -> {into:?}: {} items; undo each with domios mem restore <id> --version <history-row>.",item_ids.len());
            items::insert_in(
                tx,
                ctx,
                &codeterm_mem::NewMemItem {
                    kind: MemKind::Daily,
                    scope: codeterm_mem::MemScope::Global,
                    title: format!("Area merge {}", ulid::Ulid::new()),
                    summary: Some(summary.clone()),
                    area: Some("_dreams".into()),
                    tags: vec!["dream-diary".into()],
                    body: summary,
                    source: Some(format!("dreamer-area-merge:{run}")),
                    importance: 2.0,
                    source_path: None,
                    pinned: false,
                },
                None,
            )?;
        }
    }
    Ok(AreaMergeOutcome {
        from: from.into(),
        into: into.into(),
        dry_run,
        counts,
        item_ids,
    })
}

pub(crate) fn restore_area_in(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    id: &str,
    version: Option<i64>,
) -> StoreResult<Option<String>> {
    let history = items::histories_in(tx, id)?;
    let row = match version {
        Some(version) => history.iter().find(|row| row.id == version),
        None => history.last(),
    };
    let Some(encoded) = row
        .and_then(|row| row.reason.as_deref())
        .and_then(|reason| reason.strip_prefix("area_merge="))
    else {
        return Ok(None);
    };
    let change: serde_json::Value =
        serde_json::from_str(encoded).map_err(|_| "invalid area merge history")?;
    let from = change["from"]
        .as_str()
        .ok_or("area merge history has no source")?;
    if !area_in(tx, ctx, id, from, None)? {
        return Err("area restore made no change; item missing or already restored".into());
    }
    Ok(Some(from.into()))
}

impl PostgresMemStore {
    pub(crate) fn archive_item(
        &self,
        ctx: &WriteContext,
        id: &str,
        reason: &str,
        actor: &str,
    ) -> Result<serde_json::Value, MergeError> {
        self.transaction(|tx| {
            let Some(item) = items::get_in(tx, id)? else {
                return Ok(Err(MergeError::NotFound(format!(
                    "mem item {id} not found"
                ))));
            };
            let pinned: i64 = tx
                .query_one("SELECT is_pinned_wakeup FROM mem_items WHERE id=$1", &[&id])
                .map_err(db_error)?
                .get(0);
            if pinned != 0
                || matches!(
                    item.kind.as_str(),
                    "skill" | "rule" | "preference" | "archived"
                )
            {
                return Ok(Err(MergeError::Refused(
                    "protected or already archived item cannot be archived".into(),
                )));
            }
            let previous_kind = archive_in(tx, ctx, id, actor, "archive", Some(reason))?;
            Ok(Ok(
                serde_json::json!({"archived":id,"previous_kind":previous_kind,"reason":reason}),
            ))
        })
        .map_err(MergeError::Internal)?
    }

    pub(crate) fn merge_items_in(
        &self,
        ctx: &WriteContext,
        req: &MergeRequest<'_>,
        actor: &str,
        cap: u32,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<serde_json::Value, MergeError> {
        if req.keeper.trim().is_empty() || req.lesser.trim().is_empty() || req.keeper == req.lesser
        {
            return Err(MergeError::Invalid(
                "keeper and lesser must be present and differ".into(),
            ));
        }
        if req
            .body
            .is_some_and(|body| body.trim().is_empty() || body.len() > 1_048_576)
        {
            return Err(MergeError::Invalid(
                "body must be nonempty and at most 1 MB".into(),
            ));
        }
        self.transaction(|tx| {
            let Some(keeper)=items::get_in(tx,req.keeper)? else {return Ok(Err(MergeError::NotFound(req.keeper.into())));};
            let Some(lesser)=items::get_in(tx,req.lesser)? else {return Ok(Err(MergeError::NotFound(req.lesser.into())));};
            let pin=|tx:&mut Transaction<'_>,id:&str| -> StoreResult<bool> {Ok(tx.query_one("SELECT is_pinned_wakeup!=0 FROM mem_items WHERE id=$1", &[&id]).map_err(db_error)?.get(0))};
            if pin(tx,req.lesser)? || matches!(lesser.kind.as_str(),"skill"|"rule"|"preference"|"archived") || keeper.kind=="archived" {return Ok(Err(MergeError::Refused("protected or archived item cannot be merged away".into())));}
            let author=if lesser.author.is_neighbor() {lesser.author.clone()} else {keeper.author.clone()};
            if let Err(error)=codeterm_mem::store::guard_instruction(&author,&keeper.kind,pin(tx,req.keeper)?) {return Ok(Err(MergeError::Refused(error)));}
            if actor==merge::DREAMER_MERGE_ACTOR {
                let count:i64=tx.query_one("SELECT count(*) FROM mem_item_history WHERE change_type='merge' AND changed_by=$1 AND changed_at::timestamptz >= $2::text::timestamptz", &[&actor,&now.date_naive().to_string()]).map_err(db_error)?.get(0);
                if count>=i64::from(cap) {return Ok(Err(MergeError::CapReached("nightly merge cap reached".into())));}
            }
            let body_updated=if let Some(body)=req.body {items::update_in(tx,&ctx.with_changed_by(actor),req.keeper,&MemUpdate {body:Some(body.into()),..Default::default()})?} else {false};
            if author.is_neighbor() {
                tx.execute("UPDATE mem_items SET author_kind=$2,author_uid=$3,author_friendship=$4,author_label=$5 WHERE id=$1", &[&req.keeper,&author.kind.as_str(),&author.account_uid,&author.friendship_id,&author.label]).map_err(db_error)?;
                if !body_updated && keeper.author!=author {items::history_in(tx,&ctx.with_author(keeper.author),req.keeper,"update",&Default::default(),&Default::default(),Some("fields=author"))?;}
            }
            let (kind,from,to)=if req.conflict {(merge::SUPERSEDES,req.keeper,req.lesser)} else {(merge::MERGED_INTO,req.lesser,req.keeper)};
            archive_in(tx,ctx,req.lesser,actor,"merge",Some(&format!("{kind} {from} -> {to}")))?;
            graph::insert_in(tx,&codeterm_mem::Relation {from_id:from.into(),to_id:to.into(),relation_type:kind.into(),scope:lesser.scope,valid_from:now.to_rfc3339(),valid_to:None,confidence:1.0,properties:serde_json::json!({}),reason:Some(merge::MERGE_RELATION_REASON.into())},false)?;
            Ok(Ok(serde_json::json!({"merged":req.lesser,"keeper":req.keeper,"archived_kind":lesser.kind,"relation":{"from":from,"type":kind,"to":to},"body_updated":body_updated,"changed_by":actor})))
        }).map_err(MergeError::Internal)?
    }
}

use super::*;
pub(super) fn tracked(tx: &mut Transaction<'_>) -> Result<Vec<TrackedSource>, String> {
    Ok(tx.query("SELECT id,source_path,source_size,source_mtime,tags_json FROM mem_items WHERE source_path IS NOT NULL", &[]).map_err(db_error)?.into_iter().map(|r|TrackedSource { id:r.get(0),source_path:r.get(1),source_size:r.get(2),source_mtime:r.get(3),tags_json:r.get(4) }).collect())
}
pub(super) fn refresh(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    expected: &TrackedSource,
    refresh: &SourceRefresh<'_>,
) -> Result<bool, String> {
    let mut tags = serde_json::from_str::<Vec<String>>(&expected.tags_json).unwrap_or_default();
    tags.retain(|t| t != "source-missing");
    if refresh.missing {
        tags.push("source-missing".into());
    }
    let tags = serde_json::to_string(&tags).map_err(|e| e.to_string())?;
    let author = ctx.default_system_author("file-sync");
    let changed=tx.execute("UPDATE mem_items SET tags_json=$1,updated_at=$2,body_inline=CASE WHEN $3::text IS NULL THEN body_inline ELSE $3 END,body_blob_path=CASE WHEN $3::text IS NULL THEN body_blob_path ELSE NULL END,source_size=CASE WHEN $3::text IS NULL THEN source_size ELSE $4 END,source_mtime=CASE WHEN $3::text IS NULL THEN source_mtime ELSE $5 END,author_uid=CASE WHEN $3::text IS NULL OR author_kind='neighbor' THEN author_uid ELSE $6 END,author_friendship=CASE WHEN $3::text IS NULL OR author_kind='neighbor' THEN author_friendship ELSE $7 END,author_label=CASE WHEN $3::text IS NULL OR author_kind='neighbor' THEN author_label ELSE $8 END,author_kind=CASE WHEN $3::text IS NULL OR author_kind='neighbor' THEN author_kind ELSE $9 END WHERE id=$10 AND source_path=$11 AND source_size IS NOT DISTINCT FROM $12::bigint AND source_mtime IS NOT DISTINCT FROM $13::text AND tags_json=$14", &[&tags,&crate::items::now(),&refresh.body,&refresh.size,&refresh.mtime,&author.account_uid,&author.friendship_id,&author.label,&author.kind.as_str(),&expected.id,&expected.source_path,&expected.source_size,&expected.source_mtime,&expected.tags_json]).map_err(db_error)?;
    if changed == 0 {
        return Ok(false);
    }
    if let Some(body) = refresh.body {
        tx.execute(
            "UPDATE mem_fts SET body=$1 WHERE mem_id=$2",
            &[&body, &expected.id],
        )
        .map_err(db_error)?;
        vectors::clear_in(tx, &expected.id)?;
    }
    Ok(true)
}
pub(super) fn extraction(
    tx: &mut Transaction<'_>,
    limit: usize,
) -> Result<Vec<PendingExtraction>, String> {
    tx.query("SELECT m.id,m.kind,m.title,coalesce(m.body_inline,'') FROM mem_items m WHERE m.kind IN ('conversation','note','daily','learning','fact') AND m.tags_json NOT LIKE '%auto-extracted%' AND NOT EXISTS(SELECT 1 FROM relations r WHERE r.from_id=m.id OR r.to_id=m.id) ORDER BY m.created_at DESC LIMIT $1", &[&(limit as i64)]).map_err(db_error)?.into_iter().map(|r|Ok((r.get(0),r.get(2),r.get(3),codeterm_mem::MemKind::from_str(&r.get::<_,String>(1)).ok_or("invalid extraction kind")?))).collect()
}

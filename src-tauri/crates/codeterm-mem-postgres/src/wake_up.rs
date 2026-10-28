use crate::{db_error, items, StoreResult};
use codeterm_mem::{
    layers::{self, ScoreParts},
    WakeUpOptions,
};
use postgres::Transaction;

pub(crate) fn generate_in(tx: &mut Transaction<'_>, opts: &WakeUpOptions) -> StoreResult<String> {
    let identity = if opts.include_l0 {
        items::get_in(tx, "preference_identity")?
            .filter(|item| !item.author.is_neighbor())
            .map(|item| item.body)
    } else {
        None
    };
    let mut pinned = Vec::new();
    let mut ranked = Vec::new();
    let mut breakdown = Vec::new();
    if opts.include_l1 {
        let scope = opts
            .current_workspace
            .as_ref()
            .map(|workspace| format!("workspace:{workspace}"));
        for row in tx.query("SELECT id FROM mem_items WHERE is_pinned_wakeup=1 AND kind!='archived' AND author_kind!='neighbor' AND id!='preference_identity' AND (scope='global' OR ($1::text IS NOT NULL AND scope=$1)) ORDER BY importance DESC,evidence_count DESC,updated_at DESC LIMIT 50", &[&scope]).map_err(db_error)? {
            if let Some(item)=items::get_in(tx,&row.get::<_,String>(0))? { pinned.push(item); }
        }
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(30))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let mut scored = Vec::new();
        for row in tx.query("SELECT id,importance,evidence_count,scope,updated_at FROM mem_items WHERE is_pinned_wakeup=0 AND author_kind!='neighbor' AND id!='preference_identity' AND kind IN ('rule','preference','peer','fact','summary','skill','learning') AND (scope='global' OR ($1::text IS NOT NULL AND scope=$1)) AND tags_json NOT LIKE '%auto-extracted%'", &[&scope]).map_err(db_error)? {
            let id: String=row.get(0);
            if pinned.iter().any(|item|item.id==id) { continue; }
            let importance=row.get(1);
            let evidence_count=row.get(2);
            let ws_match=scope.as_deref()==Some(row.get::<_,String>(3).as_str());
            let recent_30d=row.get::<_,String>(4)>cutoff;
            scored.push(ScoreParts { id,kind:String::new(),importance,evidence_count,ws_match,recent_30d,score:layers::score::score_for(importance,evidence_count,ws_match,recent_30d) });
        }
        scored.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        scored.truncate(100);
        for mut score in scored {
            if let Some(item) = items::get_in(tx, &score.id)? {
                score.kind = item.kind.clone();
                ranked.push(item);
            }
            if opts.explain {
                breakdown.push(score);
            }
        }
    }
    Ok(layers::render_selection(
        identity.as_deref(),
        opts,
        &pinned,
        &ranked,
        &breakdown,
    ))
}

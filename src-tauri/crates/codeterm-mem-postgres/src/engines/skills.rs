use super::*;
use codeterm_mem_store::skill_folders::{
    normalize_file_source, skill_id_for_title, source_key, MergedSkill, SkillFolderReport,
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
fn frontmatter(body: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    body.lines()
        .take(20)
        .find(|l| l.trim_start().starts_with(&prefix))
        .map(|l| {
            l.trim_start()
                .trim_start_matches(&prefix)
                .trim()
                .trim_matches('"')
                .to_owned()
        })
        .filter(|s| !s.is_empty())
}
pub(super) fn sync(store: &PostgresMemStore, root: &Path) -> SkillFolderReport {
    let result=store.transaction(|tx| {
        let mut report=SkillFolderReport::default();
        let mut groups=HashMap::<String,Vec<(String,String,String,String)>>::new();
        for r in tx.query("SELECT id,source_path,source,tags_json,created_at FROM mem_items WHERE kind='skill' AND source_path LIKE 'file:%' ORDER BY created_at,id", &[]).map_err(db_error)? {
            let id:String=r.get(0); let path=normalize_file_source(&r.get::<_,String>(1));
            let source=r.get::<_,Option<String>>(2).as_deref().map(normalize_file_source);
            tx.execute("UPDATE mem_items SET source=$1,source_path=$2 WHERE id=$3", &[&source,&path,&id]).map_err(db_error)?;
            groups.entry(source_key(&path)).or_default().push((id,path,r.get(3),r.get(4)));
        }
        for group in groups.values_mut() {
            group.sort_by(|a,b|a.2.contains("auto-registered").cmp(&b.2.contains("auto-registered")).then_with(||a.3.cmp(&b.3)).then_with(||a.0.cmp(&b.0)));
            let kept=&group[0].0;
            for (id,path,_,_) in group.iter().skip(1) {
                tx.execute("INSERT INTO mem_item_history(item_id,changed_at,changed_by,change_type,old_title,old_body,old_kind,reason,author_kind,author_uid,author_friendship,author_label) SELECT id,$2,'librarian','merge',title,body_inline,kind,$3,author_kind,author_uid,author_friendship,author_label FROM mem_items WHERE id=$1", &[&id,&crate::items::now(),&format!("merge_into={kept}")]).map_err(db_error)?;
                tx.execute("DELETE FROM mem_items WHERE id=$1", &[&id]).map_err(db_error)?;
                tx.execute("DELETE FROM mem_fts WHERE mem_id=$1", &[&id]).map_err(db_error)?;
                vectors::clear_in(tx,id)?;
                tx.execute("DELETE FROM relations WHERE from_id=$1 OR to_id=$1", &[&id]).map_err(db_error)?;
                report.merged.push(MergedSkill {removed:id.clone(),kept:kept.clone(),source:path.clone()});
            }
        }
        let tombstones:HashSet<String>=tx.query("SELECT source_key FROM mem_source_tombstones", &[]).map_err(db_error)?.into_iter().map(|r|r.get(0)).collect();
        let known:HashSet<_>=groups.keys().cloned().collect();
        let Ok(entries)=std::fs::read_dir(root) else {return Ok(report);};
        let mut folders:Vec<_>=entries.flatten().map(|e|e.path()).collect();folders.sort();
        for folder in folders {
            let skill=folder.join("SKILL.md");
            if !folder.is_dir()||!skill.is_file() {continue;}
            let source=format!("file:{}",skill.display());let key=source_key(&source);
            if known.contains(&key) {continue;}
            if tombstones.contains(&key) {report.skipped_tombstoned.push(source);continue;}
            tx.batch_execute("SAVEPOINT register_skill").map_err(db_error)?;
            match register(tx,&folder,&skill) {
                Ok(id)=>report.registered.push(id),
                Err(e)=>{tx.batch_execute("ROLLBACK TO register_skill").map_err(db_error)?;report.errors.push(format!("register skill {source}: {e}"));}
            }
            tx.batch_execute("RELEASE register_skill").map_err(db_error)?;
        }
        Ok(report)
    });
    match result {
        Ok(report) => report,
        Err(e) => SkillFolderReport {
            errors: vec![e],
            ..Default::default()
        },
    }
}
fn register(tx: &mut Transaction<'_>, folder: &Path, skill: &Path) -> Result<String, String> {
    let body = std::fs::read_to_string(skill).map_err(|e| e.to_string())?;
    let folder_name = folder
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "skill".into());
    let title = frontmatter(&body, "title")
        .or_else(|| {
            body.lines()
                .find(|l| l.trim_start().starts_with("# "))
                .map(|l| l.trim_start().trim_start_matches("# ").trim().into())
        })
        .unwrap_or_else(|| folder_name.clone());
    let mut id = skill_id_for_title(&title);
    if id == "skill_" {
        id = skill_id_for_title(&folder_name);
    }
    if tx
        .query_opt("SELECT id FROM mem_items WHERE id=$1", &[&id])
        .map_err(db_error)?
        .is_some()
    {
        id = format!(
            "{id}_{}",
            skill_id_for_title(&folder_name).trim_start_matches("skill_")
        );
    }
    let source = normalize_file_source(&format!("file:{}", skill.display()));
    let uid = format!(
        "folder-{id}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let tags = r#"["folder-skill","auto-registered"]"#;
    let changed=tx.execute("INSERT INTO mem_items(id,uid,kind,scope,title,summary,area,tags_json,body_inline,source,importance,created_at,updated_at,source_path,author_kind,author_label) VALUES ($1,$2,'skill','global',$3,NULL,$4,$5,$6,$7,4.0,$8,$8,$7,'system','skill-folders') ON CONFLICT DO NOTHING", &[&id,&uid,&title,&frontmatter(&body,"area"),&tags,&body,&source,&crate::items::now()]).map_err(db_error)?;
    if changed == 0 {
        return Err(format!("skill id {id} already taken"));
    }
    tx.execute(
        "INSERT INTO mem_fts(mem_id,title,summary,body,tags) VALUES ($1,$2,'',$3,$4)",
        &[&id, &title, &body, &tags],
    )
    .map_err(db_error)?;
    Ok(id)
}

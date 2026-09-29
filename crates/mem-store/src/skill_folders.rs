//! Skill folder registration shared by the daemon and the CLI librarian:
//! one folder maps to one mem item, and a deleted skill stays deleted.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::error::{MemStoreError, MemStoreResult};

#[derive(Debug, Clone, Default, Serialize)]
pub struct MergedSkill {
    pub removed: String,
    pub kept: String,
    pub source: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SkillFolderReport {
    pub registered: Vec<String>,
    pub merged: Vec<MergedSkill>,
    pub skipped_tombstoned: Vec<String>,
    pub errors: Vec<String>,
}

pub(crate) fn create_tombstone_table(conn: &Connection) -> MemStoreResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS mem_source_tombstones (
            source_key  TEXT PRIMARY KEY,
            deleted_at  TEXT NOT NULL
        );",
    )?;
    Ok(())
}

/// Path identity for a `file:` source: separators unified, `.`/`..` resolved,
/// case folded on Windows. Two spellings of one file share one key.
pub fn source_key(source: &str) -> String {
    let raw = source
        .strip_prefix("file:")
        .unwrap_or(source)
        .replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let lead = if raw.starts_with('/') { "/" } else { "" };
    let key = format!("{lead}{}", parts.join("/"));
    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    }
}

pub fn record_tombstone(conn: &Connection, source_path: &str) -> MemStoreResult<()> {
    create_tombstone_table(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO mem_source_tombstones (source_key, deleted_at) VALUES (?1, ?2)",
        params![source_key(source_path), chrono::Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub fn clear_tombstone(conn: &Connection, source_path: &str) -> MemStoreResult<()> {
    create_tombstone_table(conn)?;
    conn.execute(
        "DELETE FROM mem_source_tombstones WHERE source_key = ?1",
        params![source_key(source_path)],
    )?;
    Ok(())
}

fn tombstoned_keys(conn: &Connection) -> MemStoreResult<HashSet<String>> {
    create_tombstone_table(conn)?;
    let keys = conn
        .prepare("SELECT source_key FROM mem_source_tombstones")?
        .query_map([], |r| r.get::<_, String>(0))?
        .filter_map(Result::ok)
        .collect();
    Ok(keys)
}

struct RegisteredSkill {
    id: String,
    source_path: String,
    tags_json: String,
    created_at: String,
}

fn registered_skills(conn: &Connection) -> MemStoreResult<Vec<RegisteredSkill>> {
    let rows = conn
        .prepare(
            "SELECT id, source_path, tags_json, created_at FROM mem_items \
             WHERE kind = 'skill' AND source_path LIKE 'file:%' ORDER BY created_at, id",
        )?
        .query_map([], |r| {
            Ok(RegisteredSkill {
                id: r.get(0)?,
                source_path: r.get(1)?,
                tags_json: r.get(2)?,
                created_at: r.get(3)?,
            })
        })?
        .filter_map(Result::ok)
        .collect();
    Ok(rows)
}

fn is_auto_registered(skill: &RegisteredSkill) -> bool {
    skill.tags_json.contains("auto-registered")
}

/// Collapse items that point at the same file into one: an explicitly
/// registered item outranks an auto-registered one, then the oldest wins.
fn merge_duplicate_registrations(
    conn: &Connection,
    report: &mut SkillFolderReport,
) -> MemStoreResult<()> {
    let mut groups: HashMap<String, Vec<RegisteredSkill>> = HashMap::new();
    for skill in registered_skills(conn)? {
        groups
            .entry(source_key(&skill.source_path))
            .or_default()
            .push(skill);
    }
    for (_, mut group) in groups {
        if group.len() < 2 {
            continue;
        }
        group.sort_by(|a, b| {
            is_auto_registered(a)
                .cmp(&is_auto_registered(b))
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        let kept = group.remove(0);
        for loser in group {
            remove_item(conn, &loser.id, &kept.id)?;
            report.merged.push(MergedSkill {
                removed: loser.id,
                kept: kept.id.clone(),
                source: loser.source_path,
            });
        }
    }
    Ok(())
}

fn remove_item(conn: &Connection, id: &str, merged_into: &str) -> MemStoreResult<()> {
    conn.execute(
        "INSERT INTO mem_item_history
           (item_id, changed_at, changed_by, change_type, old_title, old_body, old_kind, reason)
           SELECT id, ?2, 'librarian', 'merge', title, body_inline, kind, ?3
           FROM mem_items WHERE id = ?1",
        params![
            id,
            chrono::Utc::now().to_rfc3339(),
            format!("merge_into={merged_into}")
        ],
    )?;
    conn.execute("DELETE FROM mem_items WHERE id = ?1", params![id])?;
    conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![id])?;
    let _ = conn.execute(
        "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
        params![id, format!("{id}_chunk_%")],
    );
    conn.execute(
        "DELETE FROM relations WHERE from_id = ?1 OR to_id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Same slug rules as the daemon's `ids::slugify` for ASCII titles, so the CLI
/// scaffold and the librarian agree on one id per folder.
pub fn skill_id_for_title(title: &str) -> String {
    let mut slug = String::new();
    let mut last_underscore = false;
    for c in title.chars() {
        let lower = c.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            last_underscore = false;
        } else if !last_underscore && !slug.is_empty() {
            slug.push('_');
            last_underscore = true;
        }
    }
    let short: String = slug.trim_end_matches('_').chars().take(60).collect();
    format!("skill_{}", short.trim_end_matches('_'))
}

fn frontmatter_value(body: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    body.lines()
        .take(20)
        .find(|l| l.trim_start().starts_with(&prefix))
        .map(|l| {
            l.trim_start()
                .trim_start_matches(&prefix)
                .trim()
                .trim_matches('"')
                .to_string()
        })
        .filter(|s| !s.is_empty())
}

fn title_from_skill_md(body: &str) -> Option<String> {
    frontmatter_value(body, "title").or_else(|| {
        body.lines()
            .find(|l| l.trim_start().starts_with("# "))
            .map(|l| l.trim_start().trim_start_matches("# ").trim().to_string())
    })
}

fn item_exists(conn: &Connection, id: &str) -> MemStoreResult<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM mem_items WHERE id = ?1", params![id], |_| {
            Ok(())
        })
        .optional()?
        .is_some())
}

fn register_folder(conn: &Connection, folder: &Path, skill_md: &Path) -> MemStoreResult<String> {
    let body = std::fs::read_to_string(skill_md)
        .map_err(|e| MemStoreError::InvalidArgument(format!("read {}: {e}", skill_md.display())))?;
    let folder_name = folder
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".into());
    let title = title_from_skill_md(&body).unwrap_or_else(|| folder_name.clone());
    let mut id = skill_id_for_title(&title);
    if id == "skill_" {
        id = skill_id_for_title(&folder_name);
    }
    if item_exists(conn, &id)? {
        let suffix = skill_id_for_title(&folder_name);
        id = format!("{id}_{}", suffix.trim_start_matches("skill_"));
    }
    let source = format!("file:{}", skill_md.display());
    let now = chrono::Utc::now().to_rfc3339();
    let uid = format!("folder-{id}-{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let tags_json = r#"["folder-skill","auto-registered"]"#;
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO mem_items
           (id, uid, kind, scope, title, summary, area, tags_json,
            body_inline, source, importance, created_at, updated_at, source_path)
           VALUES (?1, ?2, 'skill', 'global', ?3, NULL, ?4, ?5, ?6, ?7, 4.0, ?8, ?8, ?7)",
        params![
            id,
            uid,
            title,
            frontmatter_value(&body, "area"),
            tags_json,
            body,
            source,
            now
        ],
    )?;
    if inserted == 0 {
        return Err(MemStoreError::Conflict(format!("skill id {id} already taken")));
    }
    conn.execute(
        "INSERT OR REPLACE INTO mem_fts (mem_id, title, summary, body, tags) VALUES (?1, ?2, '', ?3, ?4)",
        params![id, title, body, tags_json],
    )?;
    Ok(id)
}

/// Register every `<root>/<slug>/SKILL.md` that has no item yet and no
/// tombstone, after collapsing existing double registrations.
pub fn sync_skill_folders(conn: &Connection, skills_root: &Path) -> SkillFolderReport {
    let mut report = SkillFolderReport::default();
    if let Err(e) = merge_duplicate_registrations(conn, &mut report) {
        report.errors.push(format!("merge duplicates: {e}"));
    }
    let tombstones = match tombstoned_keys(conn) {
        Ok(t) => t,
        Err(e) => {
            report.errors.push(format!("read tombstones: {e}"));
            return report;
        }
    };
    let known: HashSet<String> = match registered_skills(conn) {
        Ok(items) => items.iter().map(|s| source_key(&s.source_path)).collect(),
        Err(e) => {
            report.errors.push(format!("list skills: {e}"));
            return report;
        }
    };
    let Ok(entries) = std::fs::read_dir(skills_root) else {
        return report;
    };
    let mut folders: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    folders.sort();
    for folder in folders {
        let skill_md = folder.join("SKILL.md");
        if !folder.is_dir() || !skill_md.is_file() {
            continue;
        }
        let source = format!("file:{}", skill_md.display());
        let key = source_key(&source);
        if known.contains(&key) {
            continue;
        }
        if tombstones.contains(&key) {
            report.skipped_tombstoned.push(source);
            continue;
        }
        match register_folder(conn, &folder, &skill_md) {
            Ok(id) => report.registered.push(id),
            Err(e) => report.errors.push(format!("register skill {source}: {e}")),
        }
    }
    report
}

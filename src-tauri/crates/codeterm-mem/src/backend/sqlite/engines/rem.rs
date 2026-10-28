use super::*;
use rusqlite::params;
impl SqliteMemStore {
    pub(super) fn rem_snapshot_in(&self, blueprint_merges: usize) -> Result<String, String> {
        let conn = self.conn.lock();
        let mut lines = Vec::new();
        if let Some(candidates) =
            crate::dreaming::curator_candidates::render(&conn, chrono::Utc::now().timestamp())?
        {
            lines.push(candidates);
        }
        if blueprint_merges > 0 {
            lines.push(format!(
                "BLUEPRINT LIBRARY: merged {blueprint_merges} duplicate graphs"
            ));
        }
        if let Some(candidates) = crate::dreaming::area_candidates::render_candidates(&conn)? {
            lines.push(candidates);
        }

        // 1. Unclassified items (no area)
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, title FROM mem_items WHERE (area IS NULL OR area = '') LIMIT 20",
            )
            .map_err(|e| e.to_string())?;
        let unclassified: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        if !unclassified.is_empty() {
            lines.push(format!("UNCLASSIFIED ({}):", unclassified.len()));
            for (id, kind, title) in &unclassified {
                lines.push(format!("  {} [{}] {}", id, kind, title));
            }
        }

        // 2. Potential duplicates — same kind, similar titles (first 6 chars match)
        let mut stmt = conn
            .prepare(
                r#"SELECT a.id, b.id, a.kind, a.title, b.title, a.importance, b.importance
           FROM mem_items a
           JOIN mem_items b ON a.kind = b.kind
             AND a.id < b.id
             AND substr(a.title, 1, 12) = substr(b.title, 1, 12)
             AND a.title != b.title
           LIMIT 20"#,
            )
            .map_err(|e| e.to_string())?;
        let dupes: Vec<(String, String, String, String, String, f64, f64)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        if !dupes.is_empty() {
            lines.push(format!("\nPOTENTIAL DUPLICATES ({}):", dupes.len()));
            for (a_id, b_id, kind, a_title, b_title, a_imp, b_imp) in &dupes {
                lines.push(format!(
                    "  [{}] \"{}\" (imp={}) vs \"{}\" (imp={})",
                    kind, a_title, a_imp, b_title, b_imp
                ));
                lines.push(format!("    ids: {} vs {}", a_id, b_id));
            }
        }

        // 3. Recent high-access items (last 7 days, for pattern finding)
        let week_ago = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
        let mut stmt = conn
            .prepare(
                r#"SELECT id, kind, title, area, access_count, recurrence_count
           FROM mem_items
           WHERE updated_at > ?1 AND access_count >= 2
           ORDER BY access_count DESC
           LIMIT 15"#,
            )
            .map_err(|e| e.to_string())?;
        let hot: Vec<(String, String, String, Option<String>, i64, i64)> = stmt
            .query_map(params![&week_ago], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        if !hot.is_empty() {
            lines.push(format!("\nHOT ITEMS last 7d ({}):", hot.len()));
            for (id, kind, title, area, ac, rc) in &hot {
                let recur = if *rc > 0 {
                    format!(" recur={}", rc)
                } else {
                    String::new()
                };
                lines.push(format!(
                    "  {} [{}] area={} access={}{} {}",
                    id,
                    kind,
                    area.as_deref().unwrap_or("?"),
                    ac,
                    recur,
                    title
                ));
            }
        }

        let mut stmt = conn.prepare(
        "SELECT COALESCE(area, '(none)'), COUNT(*) FROM mem_items GROUP BY area ORDER BY COUNT(*) DESC LIMIT 10"
    ).map_err(|e| e.to_string())?;
        let areas: Vec<(String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        lines.push("\nAREA DISTRIBUTION:".into());
        for (area, count) in &areas {
            lines.push(format!("  {}: {} items", area, count));
        }

        // 5. Recurring items (recurrence_count > 0 — system flagged as saved multiple times)
        let mut stmt = conn
            .prepare(
                r#"SELECT id, kind, title, area, recurrence_count, importance
           FROM mem_items
           WHERE recurrence_count > 0
           ORDER BY recurrence_count DESC
           LIMIT 15"#,
            )
            .map_err(|e| e.to_string())?;
        let recurring: Vec<(String, String, String, Option<String>, i64, f64)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        if !recurring.is_empty() {
            lines.push(format!("\nRECURRING ITEMS ({}):", recurring.len()));
            for (id, kind, title, area, rc, imp) in &recurring {
                lines.push(format!(
                    "  {} [{}] area={} recur={} imp={} {}",
                    id,
                    kind,
                    area.as_deref().unwrap_or("?"),
                    rc,
                    imp,
                    title
                ));
            }
        }

        // 6. Stale learnings (>30 days, low access — candidates for promotion or archival)
        let month_ago = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
        let mut stmt = conn
            .prepare(
                r#"SELECT id, title, access_count, importance, created_at
           FROM mem_items
           WHERE kind = 'learning' AND created_at < ?1
           ORDER BY access_count DESC, importance DESC
           LIMIT 20"#,
            )
            .map_err(|e| e.to_string())?;
        let stale_learnings: Vec<(String, String, i64, f64, String)> = stmt
            .query_map(params![&month_ago], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        if !stale_learnings.is_empty() {
            lines.push(format!(
                "\nLEARNINGS >30d ({}) — promote or archive?:",
                stale_learnings.len()
            ));
            for (id, title, ac, imp, created) in &stale_learnings {
                lines.push(format!(
                    "  {} access={} imp={} created={} {}",
                    id,
                    ac,
                    imp,
                    &created[..10],
                    title
                ));
            }
        }

        let entity_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))
            .unwrap_or(0);
        let relation_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM relations WHERE valid_to IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let orphan_entities: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entities WHERE id NOT IN (SELECT from_id FROM relations) AND id NOT IN (SELECT to_id FROM relations)",
        [], |r| r.get(0),
    ).unwrap_or(0);
        lines.push(format!(
            "\nGRAPH: {} entities, {} active relations, {} orphan entities",
            entity_count, relation_count, orphan_entities
        ));

        let mut stmt = conn
            .prepare("SELECT kind, COUNT(*) FROM mem_items GROUP BY kind ORDER BY COUNT(*) DESC")
            .map_err(|e| e.to_string())?;
        let kinds: Vec<(String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        lines.push("\nKIND DISTRIBUTION:".into());
        for (kind, count) in &kinds {
            lines.push(format!("  {}: {}", kind, count));
        }

        if lines.is_empty() {
            Ok("Nothing to optimize.".into())
        } else {
            Ok(lines.join("\n"))
        }
    }
}

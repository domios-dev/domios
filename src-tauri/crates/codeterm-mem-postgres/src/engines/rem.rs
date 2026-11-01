use super::*;
pub(super) fn snapshot(
    conn: &mut Transaction<'_>,
    blueprint_merges: usize,
) -> Result<String, String> {
    let mut lines = Vec::new();
    if let Some(candidates) = curators(conn)? {
        lines.push(candidates);
    }
    if blueprint_merges > 0 {
        lines.push(format!(
            "BLUEPRINT LIBRARY: merged {blueprint_merges} duplicate graphs"
        ));
    }
    if let Some(candidates) = area_candidates(conn)? {
        lines.push(candidates);
    }

    let unclassified: Vec<(String, String, String)> = conn
        .query(
            "SELECT id, kind, title FROM mem_items WHERE (area IS NULL OR area = '') LIMIT 20",
            &[],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect();
    if !unclassified.is_empty() {
        lines.push(format!("UNCLASSIFIED ({}):", unclassified.len()));
        for (id, kind, title) in &unclassified {
            lines.push(format!("  {} [{}] {}", id, kind, title));
        }
    }

    let dupes: Vec<(String, String, String, String, String, f64, f64)> = conn
        .query(
            r#"SELECT a.id, b.id, a.kind, a.title, b.title, a.importance, b.importance
           FROM mem_items a
           JOIN mem_items b ON a.kind = b.kind
             AND a.id < b.id
             AND substr(a.title, 1, 12) = substr(b.title, 1, 12)
             AND a.title != b.title
           LIMIT 20"#,
            &[],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|r| {
            (
                r.get(0),
                r.get(1),
                r.get(2),
                r.get(3),
                r.get(4),
                r.get(5),
                r.get(6),
            )
        })
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

    let week_ago = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let hot: Vec<(String, String, String, Option<String>, i64, i64)> = conn
        .query(
            r#"SELECT id, kind, title, area, access_count, recurrence_count
           FROM mem_items
           WHERE updated_at > $1 AND access_count >= 2
           ORDER BY access_count DESC
           LIMIT 15"#,
            &[&week_ago],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5)))
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

    let areas: Vec<(String, i64)> = conn.query("SELECT COALESCE(area, '(none)'), COUNT(*) FROM mem_items GROUP BY area ORDER BY COUNT(*) DESC LIMIT 10", &[]).map_err(db_error)?.into_iter().map(|r|(r.get(0),r.get(1))).collect();
    lines.push("\nAREA DISTRIBUTION:".into());
    for (area, count) in &areas {
        lines.push(format!("  {}: {} items", area, count));
    }

    let recurring: Vec<(String, String, String, Option<String>, i64, f64)> = conn
        .query(
            r#"SELECT id, kind, title, area, recurrence_count, importance
           FROM mem_items
           WHERE recurrence_count > 0
           ORDER BY recurrence_count DESC
           LIMIT 15"#,
            &[],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4), r.get(5)))
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

    let month_ago = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    let stale_learnings: Vec<(String, String, i64, f64, String)> = conn
        .query(
            r#"SELECT id, title, access_count, importance, created_at
           FROM mem_items
           WHERE kind = 'learning' AND created_at < $1
           ORDER BY access_count DESC, importance DESC
           LIMIT 20"#,
            &[&month_ago],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)))
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
                created.get(..10).unwrap_or(created),
                title
            ));
        }
    }

    let entity_count = super::count(conn, "entities")?;
    let relation_count: i64 = conn
        .query_one("SELECT count(*) FROM relations WHERE valid_to IS NULL", &[])
        .map_err(db_error)?
        .get(0);
    let orphan_entities:i64=conn.query_one("SELECT count(*) FROM entities WHERE id NOT IN(SELECT from_id FROM relations) AND id NOT IN(SELECT to_id FROM relations)", &[]).map_err(db_error)?.get(0);
    lines.push(format!(
        "\nGRAPH: {} entities, {} active relations, {} orphan entities",
        entity_count, relation_count, orphan_entities
    ));

    let kinds: Vec<(String, i64)> = conn
        .query(
            "SELECT kind, COUNT(*) FROM mem_items GROUP BY kind ORDER BY COUNT(*) DESC",
            &[],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|r| (r.get(0), r.get(1)))
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

fn curators(tx: &mut Transaction<'_>) -> Result<Option<String>, String> {
    let rows=tx.query("SELECT s.item_id,s.metadata_json FROM solution_metadata s JOIN mem_items m ON m.id=s.item_id WHERE m.kind='skill' ORDER BY s.item_id", &[]).map_err(db_error)?;
    let mut ids = Vec::new();
    for r in rows {
        let metadata: codeterm_mem::solution_metadata::SolutionMetadata =
            serde_json::from_str(&r.get::<_, String>(1)).map_err(|e| e.to_string())?;
        if let Some(c) = metadata.curator {
            if c.confidence == codeterm_mem::solution_metadata::Confidence::Low
                && chrono::Utc::now().timestamp().saturating_sub(c.observed_at) >= 60 * 86400
            {
                ids.push(r.get::<_, String>(0));
            }
        }
        if ids.len() == 20 {
            break;
        }
    }
    Ok((!ids.is_empty()).then(|| {
        format!(
            "STALE LOW-CONFIDENCE CURATOR CANDIDATES (review; no automatic expiry):\n{}",
            ids.join("\n")
        )
    }))
}
#[derive(Debug, PartialEq, Eq)]
struct AreaSummary {
    pub name: String,
    pub count: usize,
    pub samples: Vec<String>,
}

fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .flat_map(char::to_lowercase)
        .collect()
}

fn similar_names(a: &str, b: &str) -> bool {
    if a.starts_with('_') || b.starts_with('_') {
        return false;
    }
    let (a, b) = (normalize(a), normalize(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    if a.len().min(b.len()) >= 3
        && (a.strip_suffix('s') == Some(b.as_str()) || b.strip_suffix('s') == Some(a.as_str()))
    {
        return true;
    }
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().min(b.len()) < 6 || a.len().max(b.len()) > 128 || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut next = vec![i + 1];
        for (j, y) in b.iter().enumerate() {
            next.push(
                (previous[j + 1] + 1)
                    .min(next[j] + 1)
                    .min(previous[j] + usize::from(x != y)),
            );
        }
        previous = next;
    }
    previous[b.len()] <= 1
}

fn read_areas(conn: &mut Transaction<'_>) -> Result<Vec<AreaSummary>, String> {
    let names=conn.query("SELECT area,count(*) FROM mem_items WHERE area IS NOT NULL AND area<>'' AND left(area,1)<>'_' GROUP BY area ORDER BY area", &[]).map_err(db_error)?;
    names
        .into_iter()
        .map(|r| {
            let name: String = r.get(0);
            let count = r.get::<_, i64>(1) as usize;
            let samples = conn
                .query(
                    "SELECT title FROM mem_items WHERE area=$1 ORDER BY id LIMIT 3",
                    &[&name],
                )
                .map_err(db_error)?
                .into_iter()
                .map(|r| r.get(0))
                .collect();
            Ok(AreaSummary {
                name,
                count,
                samples,
            })
        })
        .collect()
}

fn candidate_pairs(areas: &[AreaSummary]) -> Vec<(&AreaSummary, &AreaSummary)> {
    let mut pairs = Vec::new();
    for (i, a) in areas.iter().enumerate() {
        for b in &areas[i + 1..] {
            if similar_names(&a.name, &b.name) {
                pairs.push(if a.count <= b.count { (a, b) } else { (b, a) });
                if pairs.len() == 20 {
                    return pairs;
                }
            }
        }
    }
    pairs
}

fn area_candidates(conn: &mut Transaction<'_>) -> Result<Option<String>, String> {
    let areas = read_areas(conn)?;
    let pairs = candidate_pairs(&areas);
    if pairs.is_empty() {
        return Ok(None);
    }
    let mut lines = vec!["\nAREA MERGE CANDIDATES:".to_string()];
    for (smaller, larger) in pairs {
        for area in [smaller, larger] {
            let samples: Vec<String> = area
                .samples
                .iter()
                .map(|s| s.chars().take(120).collect())
                .collect();
            lines.push(format!(
                "  {:?}: {} items; sample titles: {:?}",
                area.name, area.count, samples
            ));
        }
        lines.push(String::new());
    }
    Ok(Some(lines.join("\n")))
}

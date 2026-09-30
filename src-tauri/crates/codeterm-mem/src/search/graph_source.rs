//! Graph candidate source for the decision pool: live mem items linked to
//! entities whose id matches a query keyword. SQL only, no model calls.

use rusqlite::{params_from_iter, Connection};

use super::{extract_keywords, SearchQuery};
use crate::graph::append_active_filter;

/// Ids of live mem items linked by an active relation to a keyword-matched entity,
/// ranked by distinct matched entities, then importance, recency and id.
pub(crate) fn graph_candidates(
    conn: &Connection,
    q: &SearchQuery,
    m: usize,
) -> Result<Vec<String>, String> {
    let mut keywords: Vec<String> = extract_keywords(&q.query).into_iter().collect();
    if keywords.is_empty() || m == 0 {
        return Ok(Vec::new());
    }
    keywords.sort();
    let mut binds: Vec<String> = keywords.iter().map(|kw| format!("%{kw}%")).collect();
    let matched = (1..=binds.len())
        .map(|n| format!("lower(id) LIKE ?{n}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut inbound = String::from(
        "SELECT from_id, to_id FROM relations WHERE to_id IN (SELECT id FROM matched)",
    );
    append_active_filter(&mut inbound, &mut binds, false, None);
    let mut outbound = String::from(
        "SELECT to_id, from_id FROM relations WHERE from_id IN (SELECT id FROM matched)",
    );
    append_active_filter(&mut outbound, &mut binds, false, None);
    let mut sql = format!(
        "WITH matched(id) AS (SELECT id FROM entities WHERE {matched}),
              links(item_id, entity_id) AS ({inbound} UNION {outbound})
         SELECT mem_items.id FROM links
         JOIN mem_items ON mem_items.id = links.item_id
         WHERE mem_items.kind != 'archived'"
    );
    for (column, value) in [("kind", &q.kind), ("scope", &q.scope), ("area", &q.area)] {
        if let Some(value) = value {
            binds.push(value.clone());
            sql.push_str(&format!(" AND mem_items.{column} = ?{}", binds.len()));
        }
    }
    sql.push_str(&format!(
        " GROUP BY mem_items.id
          ORDER BY COUNT(DISTINCT links.entity_id) DESC, mem_items.importance DESC,
                   mem_items.updated_at DESC, mem_items.id
          LIMIT {m}"
    ));
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("graph candidates: {e}"))?;
    let rows = stmt
        .query_map(params_from_iter(binds.iter()), |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| format!("graph candidates: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("graph candidates: {e}"))
}

#[cfg(test)]
mod tests;

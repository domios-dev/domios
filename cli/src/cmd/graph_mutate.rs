// No daemon round-trip — these mutations are local-only.

use codeterm_mem_store::{graph, migrations};

use crate::data_dir_name;
use crate::MemGraphAction;

fn default_db_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    Ok(home.join(data_dir_name()).join("mem").join("knowledge.db"))
}

fn open_conn(db_path_arg: Option<&str>) -> Result<rusqlite::Connection, String> {
    let db_path = match db_path_arg {
        Some(p) => std::path::PathBuf::from(p),
        None => default_db_path()?,
    };
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("open {}: {}", db_path.display(), e))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("busy_timeout: {}", e))?;
    migrations::run(&conn).map_err(|e| format!("migrations: {}", e))?;
    Ok(conn)
}

fn print_or_exit<T: serde::Serialize>(result: Result<T, String>) -> ! {
    match result {
        Ok(v) => {
            let val = serde_json::to_value(&v).unwrap_or(serde_json::json!({}));
            let mut obj = match val {
                serde_json::Value::Object(m) => m,
                other => {
                    let mut m = serde_json::Map::new();
                    m.insert("result".into(), other);
                    m
                }
            };
            obj.insert("ok".into(), serde_json::Value::Bool(true));
            println!("{}", serde_json::Value::Object(obj));
            std::process::exit(0);
        }
        Err(e) => {
            println!("{}", serde_json::json!({"ok": false, "error": e}));
            std::process::exit(1);
        }
    }
}

pub fn run_route(
    from: &str,
    to: &str,
    via: &str,
    rel_in: Option<&str>,
    rel_out: Option<&str>,
    reason: Option<&str>,
    db_path: Option<&str>,
) -> ! {
    let result = (|| -> Result<graph::RouteReport, String> {
        let conn = open_conn(db_path)?;
        graph::route(
            &conn,
            graph::RouteOptions {
                from: from.into(),
                to: to.into(),
                via: via.into(),
                rel_in: rel_in.map(String::from),
                rel_out: rel_out.map(String::from),
                reason: reason.map(String::from),
            },
        )
        .map_err(|e| e.to_string())
    })();
    print_or_exit(result);
}

pub fn run_purge_invalidated(
    before: Option<&str>,
    reason: Option<&str>,
    dry_run: bool,
    db_path: Option<&str>,
) -> ! {
    let actor = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "cli".to_string());
    let result = (|| -> Result<graph::PurgeReport, String> {
        let conn = open_conn(db_path)?;
        graph::purge_invalidated(
            &conn,
            graph::PurgeOptions {
                before: before.map(String::from),
                reason_filter: reason.map(String::from),
                dry_run,
                actor,
            },
        )
        .map_err(|e| e.to_string())
    })();
    print_or_exit(result);
}

pub fn run_revive_broken(
    since: Option<&str>,
    dry_run: bool,
    db_path: Option<&str>,
) -> ! {
    let result = (|| -> Result<graph::ReviveBrokenReport, String> {
        let conn = open_conn(db_path)?;
        graph::revive_broken(
            &conn,
            graph::ReviveBrokenOptions {
                since: since.map(String::from),
                dry_run,
            },
        )
        .map_err(|e| e.to_string())
    })();
    print_or_exit(result);
}

pub fn run_hubs(threshold: u32, sample: u32, db_path: Option<&str>) -> ! {
    let result = (|| -> Result<serde_json::Value, String> {
        let conn = open_conn(db_path)?;

        // Hub list with degree (active edges only).
        let mut stmt = conn.prepare(
            "SELECT node, COUNT(*) FROM (
                SELECT from_id AS node FROM relations WHERE valid_to IS NULL
                UNION ALL
                SELECT to_id AS node FROM relations WHERE valid_to IS NULL
             ) GROUP BY node HAVING COUNT(*) >= ?1 ORDER BY 2 DESC",
        ).map_err(|e| e.to_string())?;
        let hubs: Vec<(String, i64)> = stmt
            .query_map([threshold as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();

        // Existing topic_*/role_* anchors — Dreamer should prefer routing into these instead of inventing new slugs.
        let mut anchor_stmt = conn.prepare(
            "SELECT id, entity_type FROM entities
             WHERE id LIKE 'topic\\_%' ESCAPE '\\' OR id LIKE 'role\\_%' ESCAPE '\\'
             ORDER BY id",
        ).map_err(|e| e.to_string())?;
        let anchors: Vec<serde_json::Value> = anchor_stmt
            .query_map([], |r| Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "entity_type": r.get::<_, String>(1)?,
            })))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect();

        // For each hub: relation_type histogram + neighbor sample.
        let mut hub_objs = Vec::with_capacity(hubs.len());
        for (id, degree) in hubs {
            let rel_dist: Vec<(String, i64)> = conn
                .prepare(
                    "SELECT relation_type, COUNT(*) FROM relations
                     WHERE valid_to IS NULL AND (from_id = ?1 OR to_id = ?1)
                     GROUP BY relation_type ORDER BY 2 DESC",
                )
                .map_err(|e| e.to_string())?
                .query_map([&id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .collect();

            let neighbors: Vec<serde_json::Value> = conn
                .prepare(
                    "SELECT relation_type,
                            CASE WHEN from_id = ?1 THEN to_id ELSE from_id END AS other,
                            CASE WHEN from_id = ?1 THEN 'out' ELSE 'in' END AS dir
                     FROM relations
                     WHERE valid_to IS NULL AND (from_id = ?1 OR to_id = ?1)
                     ORDER BY relation_type LIMIT ?2",
                )
                .map_err(|e| e.to_string())?
                .query_map(rusqlite::params![&id, sample as i64], |r| Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                )))
                .map_err(|e| e.to_string())?
                .filter_map(Result::ok)
                .map(|(rel, other, dir)| {
                    let title: Option<String> = conn
                        .query_row(
                            "SELECT title FROM mem_items WHERE id = ?1",
                            [&other],
                            |r| r.get(0),
                        )
                        .ok();
                    serde_json::json!({
                        "id": other,
                        "rel": rel,
                        "dir": dir,
                        "title": title,
                    })
                })
                .collect();

            hub_objs.push(serde_json::json!({
                "id": id,
                "degree": degree,
                "rel_dist": rel_dist.into_iter().map(|(r, n)| serde_json::json!({"rel": r, "count": n})).collect::<Vec<_>>(),
                "neighbors": neighbors,
            }));
        }

        Ok(serde_json::json!({
            "threshold": threshold,
            "sample": sample,
            "hubs": hub_objs,
            "existing_anchors": anchors,
        }))
    })();
    print_or_exit(result);
}

// --- Connected components / orphans ----------------------------------------

#[derive(Debug, Clone)]
struct NodeMeta {
    kind: String,
    title: String,
    area: Option<String>,
    is_entity: bool,
}

fn load_node_meta(conn: &rusqlite::Connection) -> Result<std::collections::HashMap<String, NodeMeta>, String> {
    let mut meta: std::collections::HashMap<String, NodeMeta> = std::collections::HashMap::new();
    // mem_items: id → kind/title/area
    let mut stmt = conn.prepare("SELECT id, kind, COALESCE(title,''), area FROM mem_items")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    }).map_err(|e| e.to_string())?;
    for row in rows.flatten() {
        meta.insert(row.0, NodeMeta { kind: row.1, title: row.2, area: row.3, is_entity: false });
    }
    // entities: id → entity_type as "kind"
    let mut estmt = conn.prepare("SELECT id, entity_type FROM entities")
        .map_err(|e| e.to_string())?;
    let erows = estmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    for row in erows.flatten() {
        meta.entry(row.0.clone()).or_insert(NodeMeta {
            kind: format!("entity:{}", row.1),
            title: row.0,
            area: None,
            is_entity: true,
        });
    }
    Ok(meta)
}

fn load_active_edges(conn: &rusqlite::Connection) -> Result<Vec<(String, String)>, String> {
    let mut stmt = conn.prepare(
        "SELECT from_id, to_id FROM relations WHERE valid_to IS NULL"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

// Pick anchor by most common non-empty area among members (`topic_<area>`),
// else fall back to the most-common Tech/Project/Topic entity.
fn suggested_anchor(members: &[(&str, &NodeMeta)]) -> String {
    let mut area_count: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (_, m) in members {
        if let Some(a) = &m.area {
            if !a.is_empty() && a != "_dreams" {
                *area_count.entry(a.as_str()).or_default() += 1;
            }
        }
    }
    if let Some((a, _)) = area_count.into_iter().max_by_key(|(_, c)| *c) {
        return format!("topic_{}", a.to_lowercase().replace([' ', '-'], "_"));
    }
    // No shared area: use a cluster member of type Tech/Project/Topic as anchor.
    for (id, m) in members {
        if m.is_entity {
            let prefixes = ["topic_", "project_", "tech_"];
            if prefixes.iter().any(|p| id.starts_with(p)) {
                return (*id).to_string();
            }
        }
    }
    "topic_misc".into()
}

pub fn run_islands(max_size: usize, limit: usize, db_path: Option<&str>) -> ! {
    let result = (|| -> Result<serde_json::Value, String> {
        let conn = open_conn(db_path)?;
        let meta = load_node_meta(&conn)?;
        let edges = load_active_edges(&conn)?;

        // Build adjacency over all node ids so BFS can detect isolated entities as singleton components.
        let mut adj: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for id in meta.keys() {
            adj.insert(id.clone(), std::collections::HashSet::new());
        }
        for (a, b) in &edges {
            if meta.contains_key(a) && meta.contains_key(b) {
                adj.entry(a.clone()).or_default().insert(b.clone());
                adj.entry(b.clone()).or_default().insert(a.clone());
            }
        }

        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut components: Vec<Vec<String>> = Vec::new();
        for id in meta.keys() {
            if seen.contains(id) {
                continue;
            }
            let mut comp: Vec<String> = Vec::new();
            let mut stack = vec![id.clone()];
            while let Some(x) = stack.pop() {
                if !seen.insert(x.clone()) {
                    continue;
                }
                comp.push(x.clone());
                if let Some(ns) = adj.get(&x) {
                    for n in ns {
                        if !seen.contains(n) {
                            stack.push(n.clone());
                        }
                    }
                }
            }
            components.push(comp);
        }

        let total_components = components.len();
        let mut filtered: Vec<Vec<String>> = components
            .into_iter()
            .filter(|c| c.len() >= 2 && c.len() <= max_size)
            .collect();
        filtered.sort_by_key(|c| c.len());
        filtered.truncate(limit);

        let out: Vec<serde_json::Value> = filtered
            .into_iter()
            .map(|comp| {
                let members_with_meta: Vec<(&str, &NodeMeta)> = comp
                    .iter()
                    .filter_map(|id| meta.get_key_value(id).map(|(k, v)| (k.as_str(), v)))
                    .collect();
                let anchor = suggested_anchor(&members_with_meta);
                let member_objs: Vec<serde_json::Value> = members_with_meta
                    .iter()
                    .map(|(id, m)| {
                        serde_json::json!({
                            "id": id,
                            "kind": m.kind,
                            "title": m.title,
                            "area": m.area,
                        })
                    })
                    .collect();
                serde_json::json!({
                    "size": comp.len(),
                    "members": member_objs,
                    "suggested_anchor": anchor,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "max_size": max_size,
            "total_components": total_components,
            "count": out.len(),
            "islands": out,
        }))
    })();
    print_or_exit(result);
}

pub fn run_orphans(limit: usize, db_path: Option<&str>) -> ! {
    let result = (|| -> Result<serde_json::Value, String> {
        let conn = open_conn(db_path)?;
        let meta = load_node_meta(&conn)?;
        let edges = load_active_edges(&conn)?;

        let mut degree: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for id in meta.keys() { degree.insert(id.as_str(), 0); }
        for (a, b) in &edges {
            if let Some(d) = degree.get_mut(a.as_str()) { *d += 1; }
            if let Some(d) = degree.get_mut(b.as_str()) { *d += 1; }
        }

        let mut orphans: Vec<(&str, &NodeMeta)> = degree.iter()
            .filter(|(_, d)| **d == 0)
            .filter_map(|(id, _)| meta.get_key_value(*id).map(|(k, v)| (k.as_str(), v)))
        // entities-only degree-0 nodes are often extraction noise (e.g. tech_foo).
        // Mem_items with degree 0 are real orphans and should stay visible.
            .collect();
        // Sort: real mem_items first (the user likely wants these bridged), then loose entities (often delete candidates).
        orphans.sort_by_key(|(_, m)| (m.is_entity, m.kind.clone()));
        orphans.truncate(limit);

        let out: Vec<serde_json::Value> = orphans.into_iter().map(|(id, m)| serde_json::json!({
            "id": id,
            "kind": m.kind,
            "title": m.title,
            "area": m.area,
        })).collect();

        Ok(serde_json::json!({
            "count": out.len(),
            "orphans": out,
        }))
    })();
    print_or_exit(result);
}

pub fn run_introduce_topic(
    topic_id: &str,
    entity_type: &str,
    sources_arg: &str,
    anchor: &str,
    rel_source: &str,
    rel_anchor: &str,
    reason: Option<&str>,
    db_path: Option<&str>,
) -> ! {
    let result = (|| -> Result<graph::IntroduceTopicReport, String> {
        let sources = parse_sources(sources_arg)?;
        if sources.is_empty() {
            return Err("no sources resolved".into());
        }
        let conn = open_conn(db_path)?;
        graph::introduce_topic(
            &conn,
            graph::IntroduceTopicOptions {
                topic_id: topic_id.into(),
                entity_type: entity_type.into(),
                sources,
                anchor: anchor.into(),
                rel_source: rel_source.into(),
                rel_anchor: rel_anchor.into(),
                reason: reason.map(String::from),
            },
        )
        .map_err(|e| e.to_string())
    })();
    print_or_exit(result);
}

fn parse_sources(arg: &str) -> Result<Vec<String>, String> {
    if let Some(path) = arg.strip_prefix('@') {
        let body = std::fs::read_to_string(path)
            .map_err(|e| format!("read {}: {}", path, e))?;
        Ok(body
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|s| s.to_string())
            .collect())
    } else {
        Ok(arg
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect())
    }
}

/// Returns `Some(())` if the action was handled locally; `None` if the caller
/// should continue normal dispatch. The `run_*` functions all call
/// `std::process::exit`, so `Some(())` is unreachable in practice.
pub fn try_dispatch_local(action: &MemGraphAction) -> Option<()> {
    match action {
        MemGraphAction::Route { from, to, via, rel_in, rel_out, reason, db_path } => {
            run_route(
                from, to, via,
                rel_in.as_deref(), rel_out.as_deref(),
                reason.as_deref(), db_path.as_deref(),
            )
        }
        MemGraphAction::PurgeInvalidated { before, reason, dry_run, db_path } => {
            run_purge_invalidated(
                before.as_deref(), reason.as_deref(), *dry_run, db_path.as_deref(),
            )
        }
        MemGraphAction::ReviveBroken { since, dry_run, db_path } => {
            run_revive_broken(since.as_deref(), *dry_run, db_path.as_deref())
        }
        MemGraphAction::IntroduceTopic { topic_id, entity_type, sources, anchor, rel_source, rel_anchor, reason, db_path } => {
            run_introduce_topic(
                topic_id, entity_type, sources, anchor,
                rel_source, rel_anchor,
                reason.as_deref(), db_path.as_deref(),
            )
        }
        MemGraphAction::Hubs { threshold, sample } => {
            run_hubs(*threshold, *sample, None)
        }
        MemGraphAction::Islands { max_size, limit } => {
            run_islands(*max_size, *limit, None)
        }
        MemGraphAction::Orphans { limit } => {
            run_orphans(*limit, None)
        }
        MemGraphAction::Get { id } => run_entity_get(id, None),
        _ => None,
    }
}

pub fn run_entity_get(id: &str, db_path: Option<&str>) -> ! {
    let result = (|| -> Result<serde_json::Value, String> {
        let conn = open_conn(db_path)?;
        let row: Option<(String, String, String, String, String)> = conn
            .query_row(
                "SELECT id, entity_type, scope, properties_json, created_at
                 FROM entities WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .ok();
        let (id, entity_type, scope, props_str, created_at) =
            row.ok_or_else(|| format!("entity not found: {}", id))?;
        let in_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM relations WHERE valid_to IS NULL AND to_id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let out_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM relations WHERE valid_to IS NULL AND from_id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let props: serde_json::Value =
            serde_json::from_str(&props_str).unwrap_or(serde_json::Value::Null);
        Ok(serde_json::json!({
            "id": id,
            "entity_type": entity_type,
            "scope": scope,
            "properties": props,
            "created_at": created_at,
            "active_degree": in_count + out_count,
            "in_degree": in_count,
            "out_degree": out_count,
        }))
    })();
    print_or_exit(result);
}


#[cfg(test)]
mod tests {
    use super::parse_sources;

    #[test]
    fn parse_sources_csv() {
        let v = parse_sources("a,b , c").unwrap();
        assert_eq!(v, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_sources_csv_empty_filtered() {
        let v = parse_sources("a,,b").unwrap();
        assert_eq!(v, vec!["a", "b"]);
    }

    #[test]
    fn parse_sources_at_file() {
        let mut tmp = std::env::temp_dir();
        tmp.push(format!("graph_mutate_sources_{}.txt", std::process::id()));
        std::fs::write(
            &tmp,
            "# header comment\n\ns1\n s2 \n# inline\ns3\n",
        )
        .unwrap();
        let arg = format!("@{}", tmp.display());
        let v = parse_sources(&arg).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(v, vec!["s1", "s2", "s3"]);
    }
}

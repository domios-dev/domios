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

        // Existing topic_*/role_* anchors — Dreamer should prefer routing into
        // these instead of inventing new slugs.
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

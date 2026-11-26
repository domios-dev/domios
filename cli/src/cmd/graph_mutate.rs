// No daemon round-trip — these mutations are local-only.

use codeterm_mem_store::{graph, migrations, MemStoreError};

use crate::cmd::error_contract::ErrorCode;
use crate::cmd::print::CliError;
use crate::MemGraphAction;

pub(crate) const SQLITE_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn resolve_db_path(db_path_arg: Option<&str>) -> std::path::PathBuf {
    match db_path_arg {
        Some(p) => std::path::PathBuf::from(p),
        None => crate::data_root().join("mem").join("knowledge.db"),
    }
}

fn mem_store_error(error: MemStoreError) -> CliError {
    match error {
        MemStoreError::NotFound(msg) => CliError::typed(ErrorCode::NotFound, msg),
        MemStoreError::Conflict(msg) => CliError::typed(ErrorCode::Conflict, msg),
        MemStoreError::InvalidArgument(msg) => CliError::typed(ErrorCode::InvalidArgument, msg),
        other => CliError::new(other.to_string()),
    }
}

pub(crate) fn open_readonly_conn(
    db_path_arg: Option<&str>,
) -> Result<rusqlite::Connection, CliError> {
    let db_path = resolve_db_path(db_path_arg);
    if !db_path.exists() {
        return Err(CliError::typed(
            ErrorCode::NotFound,
            format!("memory database not found: {}", db_path.display()),
        ));
    }
    let conn = rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| CliError::new(format!("open {}: {}", db_path.display(), e)))?;
    conn.busy_timeout(SQLITE_BUSY_TIMEOUT)
        .map_err(|e| CliError::new(format!("busy_timeout: {}", e)))?;
    conn.execute_batch("PRAGMA query_only = ON;")
        .map_err(|e| CliError::new(format!("query_only pragma: {}", e)))?;
    Ok(conn)
}

pub(crate) fn open_conn(db_path_arg: Option<&str>) -> Result<rusqlite::Connection, CliError> {
    let db_path = resolve_db_path(db_path_arg);
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| CliError::new(format!("open {}: {}", db_path.display(), e)))?;
    conn.busy_timeout(SQLITE_BUSY_TIMEOUT)
        .map_err(|e| CliError::new(format!("busy_timeout: {}", e)))?;
    migrations::run(&conn)
        .map_err(|e| CliError::typed(ErrorCode::MigrationFailed, format!("migrations: {}", e)))?;
    Ok(conn)
}

pub(crate) fn print_or_exit<T: serde::Serialize>(result: Result<T, CliError>) -> ! {
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
            crate::cmd::print::print_json(&serde_json::Value::Object(obj));
            std::process::exit(0);
        }
        Err(e) => {
            crate::cmd::print::fail_with(1, &e);
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
    let result = (|| -> Result<graph::RouteReport, CliError> {
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
        .map_err(mem_store_error)
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
    let result = (|| -> Result<graph::PurgeReport, CliError> {
        let conn = if dry_run {
            open_readonly_conn(db_path)?
        } else {
            open_conn(db_path)?
        };
        graph::purge_invalidated(
            &conn,
            graph::PurgeOptions {
                before: before.map(String::from),
                reason_filter: reason.map(String::from),
                dry_run,
                actor,
            },
        )
        .map_err(mem_store_error)
    })();
    print_or_exit(result);
}

pub fn run_revive_broken(since: Option<&str>, dry_run: bool, db_path: Option<&str>) -> ! {
    let result = (|| -> Result<graph::ReviveBrokenReport, CliError> {
        let conn = if dry_run {
            open_readonly_conn(db_path)?
        } else {
            open_conn(db_path)?
        };
        graph::revive_broken(
            &conn,
            graph::ReviveBrokenOptions {
                since: since.map(String::from),
                dry_run,
            },
        )
        .map_err(mem_store_error)
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

fn load_node_meta(
    conn: &rusqlite::Connection,
) -> Result<std::collections::HashMap<String, NodeMeta>, String> {
    let mut meta: std::collections::HashMap<String, NodeMeta> = std::collections::HashMap::new();
    let mut stmt = conn
        .prepare("SELECT id, kind, COALESCE(title,''), area FROM mem_items")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    for row in rows.flatten() {
        meta.insert(
            row.0,
            NodeMeta {
                kind: row.1,
                title: row.2,
                area: row.3,
                is_entity: false,
            },
        );
    }
    // entities: id → entity_type as "kind"
    let mut estmt = conn
        .prepare("SELECT id, entity_type FROM entities")
        .map_err(|e| e.to_string())?;
    let erows = estmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
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
    let mut stmt = conn
        .prepare("SELECT from_id, to_id FROM relations WHERE valid_to IS NULL")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

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
    let result = (|| -> Result<serde_json::Value, CliError> {
        let conn = open_readonly_conn(db_path)?;
        let meta = load_node_meta(&conn).map_err(CliError::new)?;
        let edges = load_active_edges(&conn).map_err(CliError::new)?;

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
    let result = (|| -> Result<serde_json::Value, CliError> {
        let conn = open_readonly_conn(db_path)?;
        let meta = load_node_meta(&conn).map_err(CliError::new)?;
        let edges = load_active_edges(&conn).map_err(CliError::new)?;

        let mut degree: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for id in meta.keys() {
            degree.insert(id.as_str(), 0);
        }
        for (a, b) in &edges {
            if let Some(d) = degree.get_mut(a.as_str()) {
                *d += 1;
            }
            if let Some(d) = degree.get_mut(b.as_str()) {
                *d += 1;
            }
        }

        let mut orphans: Vec<(&str, &NodeMeta)> = degree
            .iter()
            .filter(|(_, d)| **d == 0)
            .filter_map(|(id, _)| meta.get_key_value(*id).map(|(k, v)| (k.as_str(), v)))
            // entities-only degree-0 nodes are often extraction noise (e.g. tech_foo).
            // Mem_items with degree 0 are real orphans and should stay visible.
            .collect();
        // Sort: real mem_items first (the user likely wants these bridged), then loose entities (often delete candidates).
        orphans.sort_by_key(|(_, m)| (m.is_entity, m.kind.clone()));
        orphans.truncate(limit);

        let out: Vec<serde_json::Value> = orphans
            .into_iter()
            .map(|(id, m)| {
                serde_json::json!({
                    "id": id,
                    "kind": m.kind,
                    "title": m.title,
                    "area": m.area,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "count": out.len(),
            "orphans": out,
        }))
    })();
    print_or_exit(result);
}

#[allow(
    clippy::too_many_arguments,
    reason = "CLI command maps positional graph repair inputs."
)]
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
    let result = (|| -> Result<graph::IntroduceTopicReport, CliError> {
        let sources = parse_sources(sources_arg).map_err(CliError::new)?;
        if sources.is_empty() {
            return Err(CliError::typed(
                ErrorCode::InvalidArgument,
                "no sources resolved",
            ));
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
        .map_err(mem_store_error)
    })();
    print_or_exit(result);
}

pub(crate) fn parse_sources(arg: &str) -> Result<Vec<String>, String> {
    if let Some(path) = arg.strip_prefix('@') {
        let body = std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path, e))?;
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
        MemGraphAction::Route {
            from,
            to,
            via,
            rel_in,
            rel_out,
            reason,
            db_path,
        } => run_route(
            from,
            to,
            via,
            rel_in.as_deref(),
            rel_out.as_deref(),
            reason.as_deref(),
            db_path.as_deref(),
        ),
        MemGraphAction::PurgeInvalidated {
            before,
            reason,
            dry_run,
            db_path,
        } => run_purge_invalidated(
            before.as_deref(),
            reason.as_deref(),
            *dry_run,
            db_path.as_deref(),
        ),
        MemGraphAction::ReviveBroken {
            since,
            dry_run,
            db_path,
        } => run_revive_broken(since.as_deref(), *dry_run, db_path.as_deref()),
        MemGraphAction::IntroduceTopic {
            topic_id,
            entity_type,
            sources,
            anchor,
            rel_source,
            rel_anchor,
            reason,
            db_path,
        } if db_path.is_some() => run_introduce_topic(
            topic_id,
            entity_type,
            sources,
            anchor,
            rel_source,
            rel_anchor,
            reason.as_deref(),
            db_path.as_deref(),
        ),
        MemGraphAction::Hubs {
            threshold,
            limit,
            sample,
        } => crate::cmd::graph_hubs::run_hubs(*threshold, *limit, *sample, None),
        MemGraphAction::Islands { max_size, limit } => run_islands(*max_size, *limit, None),
        MemGraphAction::Orphans { limit } => run_orphans(*limit, None),
        MemGraphAction::Get { id } => run_entity_get(id, None),
        _ => None,
    }
}

pub fn run_entity_get(id: &str, db_path: Option<&str>) -> ! {
    let result = (|| -> Result<serde_json::Value, CliError> {
        let conn = open_readonly_conn(db_path)?;
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
        let (id, entity_type, scope, props_str, created_at) = row.ok_or_else(|| {
            CliError::typed(ErrorCode::NotFound, format!("entity not found: {}", id))
        })?;
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
    use std::sync::{mpsc, Arc, Barrier};

    use super::{
        load_active_edges, load_node_meta, open_conn, open_readonly_conn, parse_sources,
        SQLITE_BUSY_TIMEOUT,
    };
    use crate::cmd::error_contract::{classify, ErrorCode};

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
        std::fs::write(&tmp, "# header comment\n\ns1\n s2 \n# inline\ns3\n").unwrap();
        let arg = format!("@{}", tmp.display());
        let v = parse_sources(&arg).unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(v, vec!["s1", "s2", "s3"]);
    }

    #[test]
    fn open_readonly_conn_enforces_query_only_and_skips_migrations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knowledge.db");
        {
            let seed = rusqlite::Connection::open(&path).unwrap();
            seed.execute_batch("CREATE TABLE dummy(id INTEGER PRIMARY KEY);")
                .unwrap();
        }
        let path_str = path.to_str().unwrap();
        let ro = open_readonly_conn(Some(path_str)).expect("readonly open succeeds");
        let has_version_table: i64 = ro
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='mem_schema_version')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            has_version_table, 0,
            "read-only open must not run migrations"
        );
        assert!(
            ro.execute("INSERT INTO dummy(id) VALUES (1)", []).is_err(),
            "read-only connection must refuse writes"
        );
    }

    #[test]
    fn concurrent_readonly_and_writable_cli_opens_never_lock_fail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knowledge.db");
        let path_str = path.to_str().unwrap().to_string();
        {
            let conn = open_conn(Some(&path_str)).expect("initial writable open");
            conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        }

        let (locked_tx, locked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let writer_path = path.clone();
        let writer = std::thread::spawn(move || -> Result<(), String> {
            let conn = rusqlite::Connection::open(&writer_path).map_err(|e| e.to_string())?;
            conn.busy_timeout(SQLITE_BUSY_TIMEOUT)
                .map_err(|e| e.to_string())?;
            let tx = rusqlite::Transaction::new_unchecked(
                &conn,
                rusqlite::TransactionBehavior::Immediate,
            )
            .map_err(|e| e.to_string())?;
            locked_tx.send(()).expect("signal writer locked");
            release_rx.recv().expect("wait for writer release");
            tx.execute(
                "INSERT INTO entities (id, entity_type, scope, properties_json, created_at, updated_at)
                 VALUES ('topic_cli_lock', 'Topic', 'global', '{}', '2026-01-01', '2026-01-01')",
                [],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())
        });

        locked_rx.recv().expect("writer holds write lock");
        let barrier = Arc::new(Barrier::new(8));
        let mut handles = Vec::with_capacity(8);
        for index in 0..8 {
            let path_str = path_str.clone();
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || -> Result<(), String> {
                barrier.wait();
                if index % 2 == 0 {
                    let conn = open_readonly_conn(Some(&path_str)).map_err(|e| e.message)?;
                    let _ = load_node_meta(&conn)?;
                    let _ = load_active_edges(&conn)?;
                } else {
                    let _conn = open_conn(Some(&path_str)).map_err(|e| e.message)?;
                }
                Ok(())
            }));
        }
        for handle in handles {
            handle
                .join()
                .expect("cli open thread panicked")
                .expect("concurrent CLI opens must succeed without database-is-locked");
        }
        release_tx.send(()).expect("release writer");
        writer
            .join()
            .expect("writer thread panicked")
            .expect("writer commit succeeds");
    }

    #[test]
    fn open_conn_maps_migration_failure_to_typed_migration_failed_code() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knowledge.db");
        {
            let seed = rusqlite::Connection::open(&path).unwrap();
            seed.execute_batch(
                "CREATE TABLE mem_schema_version (version INTEGER NOT NULL);
                 INSERT INTO mem_schema_version VALUES (1);
                 CREATE TABLE mem_items (id TEXT PRIMARY KEY, author_uid TEXT);",
            )
            .unwrap();
        }
        let err = open_conn(Some(path.to_str().unwrap()))
            .expect_err("conflicting migration must return a typed error");
        assert_eq!(
            classify(&err.message, err.code, err.data.as_ref()),
            ErrorCode::MigrationFailed
        );
    }
}

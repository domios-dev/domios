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
        _ => None,
    }
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

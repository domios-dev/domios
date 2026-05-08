// `mem repair` — opens knowledge.db directly so it works while the
// daemon is wedged. Delegates the actual work to codeterm-mem-store::graph
// since v1.1.23.

use codeterm_mem_store::{graph, migrations};

use crate::data_dir_name;
use crate::MemRepairAction;

pub fn run_mem_repair(action: &MemRepairAction) {
    let result = match action {
        MemRepairAction::ReviveBrokenRelations { since, dry_run, db_path } => {
            eprintln!(
                "Note: 'mem repair revive-broken-relations' is deprecated; use 'mem graph revive-broken'."
            );
            try_revive(since.as_deref(), *dry_run, db_path.as_deref())
        }
    };
    match result {
        Ok(json) => println!("{}", json),
        Err(e) => {
            println!("{}", serde_json::json!({"ok": false, "error": e}));
            std::process::exit(1);
        }
    }
}

fn try_revive(
    since: Option<&str>,
    dry_run: bool,
    db_path_arg: Option<&str>,
) -> Result<serde_json::Value, String> {
    let db_path = match db_path_arg {
        Some(p) => std::path::PathBuf::from(p),
        None => default_db_path()?,
    };
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("open {}: {}", db_path.display(), e))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("busy_timeout: {}", e))?;
    migrations::run(&conn).map_err(|e| format!("migrations: {}", e))?;

    let report = graph::revive_broken(
        &conn,
        graph::ReviveBrokenOptions {
            since: since.map(String::from),
            dry_run,
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "revived": report.revived,
        "dry_run": report.dry_run,
        "since": since,
    }))
}

fn default_db_path() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    Ok(home.join(data_dir_name()).join("mem").join("knowledge.db"))
}

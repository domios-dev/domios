//! `mem dream` CLI handlers — daemon-routed phase + local plan apply.
//!
//! `apply` opens knowledge.db directly (no daemon round-trip). `run` posts
//! to the daemon's `/api/mem/dream?phase=` endpoint. The heuristic
//! suggest-clusters phase that originally produced plans was stripped before
//! v1.1.23 ship — see crates/mem-store/src/cluster.rs module doc.

use codeterm_mem_store::{cluster, migrations, plan_format};

use crate::data_dir_name;
use crate::MemDreamAction;

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

pub fn handle_dream(action: MemDreamAction) -> ! {
    match action {
        MemDreamAction::Run { phase } => run_dream_phase(&phase),
        MemDreamAction::Apply { plan_file, db_path } => {
            run_apply(&plan_file, db_path.as_deref())
        }
    }
}

/// Daemon-routed dream phase (preserves the v1.1.22 `mem dream --phase`
/// behaviour as `mem dream run --phase`).
fn run_dream_phase(phase: &str) -> ! {
    let (base, token) = crate::read_api_config();
    let http = crate::build_http(&token);
    let url = format!("{}/api/mem/dream?phase={}", base, urlencoding::encode(phase));
    let resp = http.post(&url).json(&serde_json::json!({})).send();
    match resp {
        Ok(r) if r.status().is_success() => {
            let val: serde_json::Value = r.json().unwrap_or(serde_json::json!({}));
            println!("{}", val);
            std::process::exit(0);
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().unwrap_or_default();
            println!("{}", serde_json::json!({"ok": false, "status": status.as_u16(), "body": body}));
            std::process::exit(1);
        }
        Err(e) => {
            println!("{}", serde_json::json!({"ok": false, "error": e.to_string()}));
            std::process::exit(1);
        }
    }
}

fn run_apply(plan_file: &str, db_path: Option<&str>) -> ! {
    let plan_path = std::path::PathBuf::from(plan_file);
    let result = (|| -> Result<(usize, usize, Vec<String>), String> {
        let md = std::fs::read_to_string(&plan_path)
            .map_err(|e| format!("read {}: {}", plan_path.display(), e))?;
        let parsed = plan_format::read(&md)
            .map_err(|e| format!("parse {}: {}", plan_path.display(), e))?;
        let mut warnings = parsed.warnings.clone();
        let conn = open_conn(db_path)?;
        let report = cluster::apply(&conn, &parsed.plan)
            .map_err(|e| format!("apply: {}", e))?;
        warnings.extend(report.errors.iter().cloned());
        Ok((
            report.clusters_applied as usize,
            report.clusters_failed as usize,
            warnings,
        ))
    })();

    match result {
        Ok((applied, failed, warnings)) => {
            // Append summary footer + rename to `*-applied.md`.
            let renamed = match append_footer_and_rename(&plan_path, applied, failed, &warnings) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("warn: could not rename plan to *-applied.md: {}", e);
                    plan_path.clone()
                }
            };
            println!(
                "{}",
                serde_json::json!({
                    "ok": true,
                    "applied": applied,
                    "failed": failed,
                    "warnings": warnings,
                    "plan": renamed.display().to_string(),
                })
            );
            std::process::exit(0);
        }
        Err(e) => {
            println!("{}", serde_json::json!({"ok": false, "error": e}));
            std::process::exit(1);
        }
    }
}

fn append_footer_and_rename(
    plan_path: &std::path::Path,
    applied: usize,
    failed: usize,
    warnings: &[String],
) -> Result<std::path::PathBuf, String> {
    let original = std::fs::read_to_string(plan_path)
        .map_err(|e| format!("re-read {}: {}", plan_path.display(), e))?;
    let mut out = original;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\n---\n\n");
    out.push_str(&format!(
        "# Applied {} — {} cluster(s) applied, {} failed\n\n",
        chrono::Utc::now().to_rfc3339(),
        applied,
        failed
    ));
    if !warnings.is_empty() {
        out.push_str("Warnings / errors:\n\n");
        for w in warnings {
            out.push_str(&format!("- {}\n", w));
        }
        out.push('\n');
    }

    // Rename `<stem>.md` → `<stem>-applied.md`. If the file already ends in
    // `-applied.md`, append a numeric suffix to avoid clobbering history.
    let new_path = build_applied_path(plan_path);
    std::fs::write(&new_path, &out)
        .map_err(|e| format!("write {}: {}", new_path.display(), e))?;
    if new_path != plan_path {
        let _ = std::fs::remove_file(plan_path);
    }
    Ok(new_path)
}

fn build_applied_path(plan_path: &std::path::Path) -> std::path::PathBuf {
    let parent = plan_path.parent().unwrap_or(std::path::Path::new("."));
    let stem = plan_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "plan".to_string());
    let base = if stem.ends_with("-applied") {
        format!("{}-{}", stem, chrono::Utc::now().format("%Y%m%dT%H%M%SZ"))
    } else {
        format!("{}-applied", stem)
    };
    parent.join(format!("{}.md", base))
}

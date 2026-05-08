//! `mem dream` CLI handlers — heuristic re-cluster plan emit + apply.
//! Local-only (open knowledge.db directly, no daemon round-trip).

use codeterm_mem_store::{
    cluster::{self, ClusterCandidate, Hub},
    migrations,
    plan_format::{self, ClusterAction, ClusterPlan},
};

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

fn dream_plans_dir() -> Result<std::path::PathBuf, String> {
    let home = dirs::home_dir().ok_or("home dir not found")?;
    Ok(home
        .join(data_dir_name())
        .join("mem")
        .join("dream-plans"))
}

fn utc_timestamp_filename() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

pub fn handle_dream(action: MemDreamAction) -> ! {
    match action {
        MemDreamAction::Run { phase } => run_dream_phase(&phase),
        MemDreamAction::SuggestClusters { threshold, out, db_path } => {
            run_suggest_clusters(threshold, out.as_deref(), db_path.as_deref())
        }
        MemDreamAction::Apply { plan_file, db_path } => {
            run_apply(&plan_file, db_path.as_deref())
        }
        MemDreamAction::LlmRecluster { .. } => {
            eprintln!("LLM re-clustering arrives in Track 4b (v1.1.23 or v1.1.24).");
            eprintln!("Use `mem dream suggest-clusters` for the heuristic pass.");
            std::process::exit(2);
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

fn run_suggest_clusters(threshold: u32, out_arg: Option<&str>, db_path: Option<&str>) -> ! {
    let result = (|| -> Result<std::path::PathBuf, String> {
        let conn = open_conn(db_path)?;

        let hubs = cluster::find_hubs(&conn, threshold)
            .map_err(|e| format!("find_hubs: {}", e))?;
        let mut clusters: Vec<ClusterAction> = Vec::new();
        let mut total_proposals: usize = 0;
        for hub in &hubs {
            let proposals = cluster::propose_for_hub(&conn, hub)
                .map_err(|e| format!("propose_for_hub({}): {}", hub.id, e))?;
            for c in proposals {
                total_proposals += 1;
                clusters.push(candidate_to_action(hub, &c));
            }
        }

        let plan = ClusterPlan {
            generated_at: chrono::Utc::now().to_rfc3339(),
            clusters,
        };
        let md = plan_format::write(&plan);

        let out_path = match out_arg {
            Some(p) => std::path::PathBuf::from(p),
            None => {
                let dir = dream_plans_dir()?;
                std::fs::create_dir_all(&dir)
                    .map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
                dir.join(format!("{}.md", utc_timestamp_filename()))
            }
        };
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
        }
        std::fs::write(&out_path, &md)
            .map_err(|e| format!("write {}: {}", out_path.display(), e))?;

        eprintln!(
            "Dream plan: {} hub(s) ≥ {} edges → {} candidate cluster(s).",
            hubs.len(),
            threshold,
            total_proposals
        );
        eprintln!("Plan: {}", out_path.display());
        eprintln!("Edit any section, then: codeterm mem dream apply <plan>");
        Ok(out_path)
    })();

    match result {
        Ok(out_path) => {
            // Bridge ping is best-effort. Bridge not configured / daemon
            // down / network failure must not block the CLI exit.
            let _ = try_bridge_ping(&out_path);
            println!(
                "{}",
                serde_json::json!({"ok": true, "plan": out_path.display().to_string()})
            );
            std::process::exit(0);
        }
        Err(e) => {
            println!("{}", serde_json::json!({"ok": false, "error": e}));
            std::process::exit(1);
        }
    }
}

fn candidate_to_action(hub: &Hub, c: &ClusterCandidate) -> ClusterAction {
    let rationale = format!(
        "Hub {} has {} active edges; {} of them ({:.0}%) are '{}'. \
         Routing those through {} removes hub bloat.",
        hub.id,
        hub.degree,
        c.members.len(),
        c.ratio * 100.0,
        c.dominant_rel,
        c.topic_id,
    );
    let reason = format!(
        "heuristic-recluster-{}",
        chrono::Utc::now().format("%Y-%m-%d")
    );
    ClusterAction {
        topic_id: c.topic_id.clone(),
        entity_type: "Topic".to_string(),
        anchor: c.anchor.clone(),
        rel_source: c.rel_source.clone(),
        rel_anchor: c.rel_anchor.clone(),
        reason: Some(reason),
        rationale,
        sources: c.members.clone(),
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

/// Send a Telegram bridge ping if the daemon is reachable AND the bridge is
/// configured. Any failure (no daemon, no bridge, RPC error, network) is
/// silently swallowed — hands-off mode shouldn't crash on a wedged daemon.
fn try_bridge_ping(plan_path: &std::path::Path) -> Result<(), String> {
    let socket_path = crate::client::discover_socket()?;
    let mut client = crate::client::SocketClient::connect(&socket_path)?;
    let text = format!("v1.1.23 dream plan ready: {}", plan_path.display());
    let params = serde_json::json!({
        "service": "telegram",
        "chat_id": "186809246",
        "text": text,
        "type": "text",
        "append": false,
    });
    let _ = client.call("bridge.send", Some(params))?;
    Ok(())
}

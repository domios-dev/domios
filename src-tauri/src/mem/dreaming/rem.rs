//! REM phase: prepare candidate data for the dreaming agent and spawn the
//! headless REM worker (dedup, classify, insights, diary).

use rusqlite::params;

use crate::mem::MemManager;

/// Prepare data for the REM agent — returns a text summary of candidates.
/// Called by `codeterm mem dream --report` so the agent gets concrete data.
pub fn prepare_rem_data(mem: &MemManager) -> Result<String, String> {
    let conn = mem.conn();
    let conn = conn.lock();
    let mut lines = Vec::new();

    // 1. Unclassified items (no area)
    let mut stmt = conn
        .prepare("SELECT id, kind, title FROM mem_items WHERE (area IS NULL OR area = '') LIMIT 20")
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

    // 4. Area distribution
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

    // 7. Graph stats
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

    // 8. Kind distribution
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

pub(crate) const DREAMING_TELEGRAM_STEP_ENABLED: &str = r#"Then push the INSIGHT (only the insight prose, not the log block) to
telegram so the vault talks back. OK if it errors — the diary in mem
is the durable artifact:

    codeterm mem search "telegram chat id" --limit 1
    # extract the chat id, then:
    codeterm bridge send --to <chat_id> --service telegram \
      "🧠 Dreamer brief\n\n<just the insight, no <insight> tags, no log>"
    # If no fact found or bridge errors, skip — telegram is nice-to-have."#;

pub(crate) const DREAMING_TELEGRAM_STEP_DISABLED: &str = "Telegram notification is DISABLED \
    (memory.dreaming_telegram_notify=false). Skip any outbound notification — \
    do not search for chat ids, do not invoke the bridge command. The diary \
    in mem is the artifact; the user has opted out of the nightly Telegram ping.";

/// Spawn a headless dreaming worker for REM phase (dedup, classify, insights, diary).
/// Called after the supervisor runs the SQL phases.
pub fn spawn_rem_worker(svc: &std::sync::Arc<crate::service::ServiceContext>) {
    let mem_cfg = crate::config::memory_config();
    let agent = if mem_cfg.dreaming_agent.is_empty() {
        crate::config::default_agent_provider()
    } else {
        mem_cfg.dreaming_agent.clone()
    };
    // Dreaming REM worker uses `dreaming_model` when set, otherwise falls back to the shared `worker_model` default ("haiku").
    let raw_model = if mem_cfg.dreaming_model.is_empty() {
        mem_cfg.worker_model.clone()
    } else {
        mem_cfg.dreaming_model.clone()
    };
    let model = crate::agent::models::canonicalize_short(&raw_model).to_string();

    // Telegram notification is optional: enabled builds send a one-line insight, but mem diary remains the durable record either way.
    let telegram_step = if mem_cfg.dreaming_telegram_notify {
        DREAMING_TELEGRAM_STEP_ENABLED
    } else {
        DREAMING_TELEGRAM_STEP_DISABLED
    };
    let prompt = crate::prompts::DREAMING_REM.replace("{telegram_notify_step}", telegram_step);

    match crate::mem::workers::spawn_headless(
        svc,
        format!("{}rem", crate::mem::workers::DREAMING_INTENT_PREFIX),
        prompt,
        "system:dreaming".into(),
        agent,
        model,
    ) {
        Ok(wid) => log::info!("[dreaming] REM worker spawned: {}", wid),
        Err(e) => log::error!("[dreaming] REM worker spawn failed: {}", e),
    }
}

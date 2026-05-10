//! Layered wake-up: L0 (identity) + L1 (critical facts) text payload for
//! injection into the General Agent's system prompt at session start.
//!
//! L0  identity         ~50-100 tokens   always loaded
//! L1  critical facts   ~500-800 tokens  always loaded
//! L2  area recall      on-demand        NOT in wake-up
//! L3  deep semantic    on-demand        NOT in wake-up
//!
//! Hard cap with priority eviction: rule > preference > peer > fact > summary.
//! When we exceed budget, drop in reverse priority order until we fit.

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::store::MemItem;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeUpOptions {
    pub l0_budget_tokens: usize,
    pub l1_budget_tokens: usize,
    pub include_l0: bool,
    pub include_l1: bool,
}

impl Default for WakeUpOptions {
    fn default() -> Self {
        Self {
            l0_budget_tokens: 100,
            l1_budget_tokens: 700,
            include_l0: true,
            include_l1: true,
        }
    }
}

const IDENTITY_ID: &str = "preference_identity";

pub fn generate(
    conn: &Connection,
    opts: &WakeUpOptions,
    blobs_dir: &Path,
) -> Result<String, String> {
    let mut out = String::new();
    let mut l0_tokens = 0;
    let mut l1_tokens = 0;

    if opts.include_l0 {
        let identity = super::store::get(conn, IDENTITY_ID, blobs_dir)?
            .map(|i| i.body)
            .unwrap_or_else(|| {
                "No identity configured. Tell the agent what to call you and run `codeterm mem save --kind preference --title identity \"...\"`.".to_string()
            });
        let truncated = truncate_tokens(&identity, opts.l0_budget_tokens);
        l0_tokens = approx_tokens(&truncated);
        out.push_str("## L0 — Identity\n\n");
        out.push_str(&truncated);
        out.push_str("\n\n");
    }

    if opts.include_l1 {
        let l1_items = collect_l1_candidates(conn, blobs_dir)?;
        let (rendered, used) = render_l1(&l1_items, opts.l1_budget_tokens);
        l1_tokens = used;
        out.push_str("## L1 — Critical context\n\n");
        out.push_str(&rendered);
    }

    out.push_str(&format!(
        "\n\n<!-- L0: {} tok | L1: {} tok | total: {} tok -->\n",
        l0_tokens,
        l1_tokens,
        l0_tokens + l1_tokens,
    ));
    Ok(out)
}

/// Pull L1 candidates from mem_items: rules + preferences + peers always,
/// facts/summaries with importance ≥ 4.0. Sorted by priority then importance desc.
fn collect_l1_candidates(
    conn: &Connection,
    blobs_dir: &Path,
) -> Result<Vec<(L1Priority, MemItem)>, String> {
    let mut stmt = conn
        .prepare(
            r#"SELECT id FROM mem_items
               WHERE (kind IN ('rule', 'preference', 'peer')
                  OR (kind IN ('fact', 'summary') AND importance >= 4.0))
                 AND tags_json NOT LIKE '%auto-extracted%'
               ORDER BY importance DESC, updated_at DESC"#,
        )
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let mut out = Vec::new();
    for id in ids {
        if id == IDENTITY_ID {
            continue; // already in L0
        }
        if let Some(item) = super::store::get(conn, &id, blobs_dir)? {
            let prio = priority_for_kind(&item.kind);
            out.push((prio, item));
        }
    }
    out.sort_by_key(|(p, item)| (*p as u8, -(item.importance * 100.0) as i64));
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum L1Priority {
    Rule = 0,
    Preference = 1,
    Peer = 2,
    Fact = 3,
    Summary = 4,
}

fn priority_for_kind(kind: &str) -> L1Priority {
    match kind {
        "rule" => L1Priority::Rule,
        "preference" => L1Priority::Preference,
        "peer" => L1Priority::Peer,
        "fact" => L1Priority::Fact,
        "summary" => L1Priority::Summary,
        _ => L1Priority::Summary,
    }
}

/// Render grouped by kind, hard-capped at budget. Returns (rendered, tokens_used).
fn render_l1(items: &[(L1Priority, MemItem)], budget: usize) -> (String, usize) {
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<u8, (&'static str, Vec<&MemItem>)> = BTreeMap::new();
    for (p, item) in items {
        let label = match p {
            L1Priority::Rule => "Rules",
            L1Priority::Preference => "Preferences",
            L1Priority::Peer => "Peers",
            L1Priority::Fact => "Facts",
            L1Priority::Summary => "Summaries",
        };
        groups.entry(*p as u8).or_insert((label, Vec::new())).1.push(item);
    }

    let mut out = String::new();
    let mut tokens = 0usize;
    let mut dropped = 0usize;
    let mut dropped_tokens = 0usize;

    for (_, (label, items)) in &groups {
        let header = format!("### {}\n", label);
        let header_tok = approx_tokens(&header);
        if tokens + header_tok > budget {
            dropped += items.len();
            for it in items {
                dropped_tokens += approx_tokens(&item_line(it));
            }
            continue;
        }
        out.push_str(&header);
        tokens += header_tok;
        for item in items {
            let line = item_line(item);
            let line_tok = approx_tokens(&line);
            if tokens + line_tok > budget {
                dropped += 1;
                dropped_tokens += line_tok;
                continue;
            }
            out.push_str(&line);
            tokens += line_tok;
        }
        out.push('\n');
        tokens += 1;
    }

    if dropped > 0 {
        out.push_str(&format!(
            "<!-- dropped {} items, {} tok over budget -->\n",
            dropped, dropped_tokens
        ));
    }
    (out, tokens)
}

fn item_line(item: &MemItem) -> String {
    let body_snip = item.summary.as_ref().unwrap_or(&item.body);
    let snip = truncate_chars(body_snip, 160);
    let snip = snip.replace('\n', " ");
    format!("- {}: {}\n", item.title, snip)
}

fn approx_tokens(s: &str) -> usize {
    (s.len() + 3) / 4
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        head + "…"
    } else {
        head
    }
}

fn truncate_tokens(s: &str, max: usize) -> String {
    truncate_chars(s, max * 4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::store::{init_schema, insert, MemKind, MemScope, NewMemItem};
    use rusqlite::Connection;
    use std::path::PathBuf;

    fn fresh() -> (Connection, PathBuf) {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let tmp = std::env::temp_dir().join(format!("memlay-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&tmp).unwrap();
        (conn, tmp)
    }

    #[test]
    fn wake_up_includes_l0_l1() {
        let (conn, blobs) = fresh();
        insert(&conn, &NewMemItem {
            kind: MemKind::Preference, scope: MemScope::Global,
            title: "identity".into(), summary: None, area: None, tags: vec![],
            body: "I am Rollo, the GA on this machine.".into(),
            source: None, importance: 5.0, source_path: None,
        }, &blobs).unwrap();
        insert(&conn, &NewMemItem {
            kind: MemKind::Rule, scope: MemScope::Global,
            title: "never edit autogen".into(), summary: None, area: None, tags: vec![],
            body: "Never edit CLAUDE.md or AGENTS.md".into(),
            source: None, importance: 5.0, source_path: None,
        }, &blobs).unwrap();
        insert(&conn, &NewMemItem {
            kind: MemKind::Peer, scope: MemScope::Global,
            title: "ainyiva-bot".into(), summary: Some("mac mini, paired".into()), area: None,
            tags: vec![], body: "ainyiva-bot is the Mac mini peer".into(),
            source: None, importance: 4.0, source_path: None,
        }, &blobs).unwrap();

        let out = generate(&conn, &WakeUpOptions::default(), &blobs).unwrap();
        assert!(out.contains("L0 — Identity"));
        assert!(out.contains("Rollo"));
        assert!(out.contains("L1 — Critical context"));
        assert!(out.contains("Rules"));
        assert!(out.contains("never edit autogen"));
        assert!(out.contains("Peers"));
    }

    #[test]
    fn wake_up_default_when_no_identity() {
        let (conn, blobs) = fresh();
        let out = generate(&conn, &WakeUpOptions::default(), &blobs).unwrap();
        assert!(out.contains("No identity configured"));
    }

    #[test]
    fn budget_drops_low_priority_first() {
        let (conn, blobs) = fresh();
        // Big summary that won't fit
        for i in 0..20 {
            insert(&conn, &NewMemItem {
                kind: MemKind::Summary, scope: MemScope::Global,
                title: format!("summary {}", i), summary: None, area: None, tags: vec![],
                body: "x".repeat(200), source: None, importance: 4.5, source_path: None,
            }, &blobs).unwrap();
        }
        insert(&conn, &NewMemItem {
            kind: MemKind::Rule, scope: MemScope::Global,
            title: "important rule".into(), summary: None, area: None, tags: vec![],
            body: "this rule must always survive".into(), source: None, importance: 5.0, source_path: None,
        }, &blobs).unwrap();

        let out = generate(&conn, &WakeUpOptions {
            l0_budget_tokens: 50, l1_budget_tokens: 100,
            include_l0: false, include_l1: true,
        }, &blobs).unwrap();
        assert!(out.contains("important rule"), "rule must survive eviction: {}", out);
        assert!(out.contains("dropped"));
    }
}

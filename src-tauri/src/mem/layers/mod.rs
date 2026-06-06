//! Layered wake-up: L0 (identity) + L1 (critical facts) text payload for
//! injection into the General Agent's system prompt at session start.
//!
//! L0  identity         ~50-100 tokens   always loaded
//! L1  critical facts   ~300 tokens      always loaded
//! L2  area recall      on-demand        NOT in wake-up
//! L3  deep semantic    on-demand        NOT in wake-up
//!
//! L1 selection is a two-pass pipeline:
//!   1. pinned items (`is_pinned_wakeup = 1`) in scope, capped at 50.
//!   2. ranked unpinned items by
//!      `score = importance + log10(1+evidence)*0.5 + ws_match*0.5 + recent_30d*0.3`,
//!      capped at 100.
//! Render groups by kind (rules > preferences > peers > facts > summaries > skills > learnings).
//! Pinned items render first inside each kind section, prefixed with `★ `.
//! Budget eviction drops in reverse kind-priority; if even the pinned set alone
//! overflows the L1 budget we surface a warning so the user can shrink pins.

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::store::MemItem;

mod score;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeUpOptions {
    pub l0_budget_tokens: usize,
    pub l1_budget_tokens: usize,
    pub include_l0: bool,
    pub include_l1: bool,
    #[serde(default)]
    pub current_workspace: Option<String>,
    #[serde(default)]
    pub explain: bool,
}

impl Default for WakeUpOptions {
    fn default() -> Self {
        Self {
            l0_budget_tokens: 100,
            l1_budget_tokens: 300,
            include_l0: true,
            include_l1: true,
            current_workspace: None,
            explain: false,
        }
    }
}

const IDENTITY_ID: &str = "preference_identity";
const PASS2_LIMIT: usize = 100;

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
        let ws = opts.current_workspace.as_deref();
        let (pinned, ranked, breakdown) = collect_l1_candidates(conn, blobs_dir, ws, opts.explain)?;
        let (rendered, used, evicted_pinned) = render_l1(&pinned, &ranked, opts.l1_budget_tokens);
        l1_tokens = used;
        if evicted_pinned > 0 {
            out.push_str(&format!(
                "<!-- WARN: {evicted_pinned} pinned items did not fit L1 budget {} -->\n",
                opts.l1_budget_tokens,
            ));
            log::warn!(
                "[wake-up] pinned-eviction count={evicted_pinned} budget={}",
                opts.l1_budget_tokens
            );
        }
        out.push_str("## L1 — Critical context\n\n");
        out.push_str(&rendered);
        if opts.explain {
            out.push_str("\n<!-- L1 score breakdown -->\n");
            for p in &breakdown {
                out.push_str(&format!(
                    "[L1 score] id={} kind={} score={:.2} importance={:.1} evidence={} ws_match={} recent={}\n",
                    p.id, p.kind, p.score, p.importance, p.evidence_count, p.ws_match, p.recent_30d,
                ));
            }
        }
    }

    out.push_str(&format!(
        "\n\n<!-- L0: {} tok | L1: {} tok | total: {} tok -->\n",
        l0_tokens,
        l1_tokens,
        l0_tokens + l1_tokens,
    ));
    Ok(out)
}

#[derive(Debug, Clone)]
pub(crate) struct ScoreParts {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) importance: f64,
    pub(crate) evidence_count: i64,
    pub(crate) ws_match: bool,
    pub(crate) recent_30d: bool,
    pub(crate) score: f64,
}

fn collect_l1_candidates(
    conn: &Connection,
    blobs_dir: &Path,
    current_ws: Option<&str>,
    explain: bool,
) -> Result<(Vec<MemItem>, Vec<MemItem>, Vec<ScoreParts>), String> {
    let scope_param = current_ws.map(|w| format!("workspace:{w}"));
    let bind: Option<&str> = scope_param.as_deref();

    let mut pinned_ids = Vec::<String>::new();
    {
        let mut stmt = conn.prepare(score::PASS1_SQL).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([bind], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        for r in rows.flatten() {
            pinned_ids.push(r);
        }
    }

    struct Pass2Row {
        id: String,
        importance: f64,
        evidence: i64,
        ws_match: bool,
        recent: bool,
        score: f64,
    }
    let mut scored: Vec<Pass2Row> = Vec::new();
    {
        let mut stmt = conn.prepare(score::PASS2_SQL).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([bind], |r| {
                let id: String = r.get(0)?;
                let importance: f64 = r.get(1)?;
                let evidence: i64 = r.get(2)?;
                let ws_match: i64 = r.get(3)?;
                let recent: i64 = r.get(4)?;
                Ok((id, importance, evidence, ws_match != 0, recent != 0))
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            let (id, importance, evidence, ws_match, recent) = row;
            if pinned_ids.contains(&id) {
                continue;
            }
            let score = score::score_for(importance, evidence, ws_match, recent);
            scored.push(Pass2Row {
                id,
                importance,
                evidence,
                ws_match,
                recent,
                score,
            });
        }
    }
    // Score desc, id asc tiebreaker (deterministic ordering for tests / explain).
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    scored.truncate(PASS2_LIMIT);
    let ranked_ids: Vec<String> = scored.iter().map(|r| r.id.clone()).collect();

    // N+1 load is acceptable for ≤150 items. Don't batch prematurely —
    // store::get also handles blob inflation and source-freshness debounce.
    let pinned = load_items(conn, blobs_dir, &pinned_ids)?;
    let ranked = load_items(conn, blobs_dir, &ranked_ids)?;

    let breakdown = if explain {
        let mut out = Vec::with_capacity(scored.len());
        for row in &scored {
            let kind = ranked
                .iter()
                .find(|m| m.id == row.id)
                .map(|m| m.kind.clone())
                .unwrap_or_default();
            out.push(ScoreParts {
                id: row.id.clone(),
                kind,
                importance: row.importance,
                evidence_count: row.evidence,
                ws_match: row.ws_match,
                recent_30d: row.recent,
                score: row.score,
            });
        }
        out
    } else {
        Vec::new()
    };

    Ok((pinned, ranked, breakdown))
}

fn load_items(conn: &Connection, blobs_dir: &Path, ids: &[String]) -> Result<Vec<MemItem>, String> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(item) = super::store::get(conn, id, blobs_dir)? {
            out.push(item);
        }
    }
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
    Skill = 5,
    Learning = 6,
}

fn priority_for_kind(kind: &str) -> L1Priority {
    match kind {
        "rule" => L1Priority::Rule,
        "preference" => L1Priority::Preference,
        "peer" => L1Priority::Peer,
        "fact" => L1Priority::Fact,
        "summary" => L1Priority::Summary,
        "skill" => L1Priority::Skill,
        "learning" => L1Priority::Learning,
        _ => L1Priority::Summary,
    }
}

fn label_for(prio: L1Priority) -> &'static str {
    match prio {
        L1Priority::Rule => "Rules",
        L1Priority::Preference => "Preferences",
        L1Priority::Peer => "Peers",
        L1Priority::Fact => "Facts",
        L1Priority::Summary => "Summaries",
        L1Priority::Skill => "Skills",
        L1Priority::Learning => "Learnings",
    }
}

/// Render grouped by kind. Returns (rendered, tokens_used, evicted_pinned).
/// `evicted_pinned` counts pinned items that did NOT fit the L1 budget — even
/// when ranked items were also dropped, this stays focused on the pinned set
/// because that's the user-actionable signal (shrink pins or grow budget).
fn render_l1(pinned: &[MemItem], ranked: &[MemItem], budget: usize) -> (String, usize, usize) {
    use std::collections::BTreeMap;

    let mut groups: BTreeMap<u8, (L1Priority, Vec<(bool, &MemItem)>)> = BTreeMap::new();
    for it in pinned {
        let prio = priority_for_kind(&it.kind);
        groups
            .entry(prio as u8)
            .or_insert((prio, Vec::new()))
            .1
            .push((true, it));
    }
    for it in ranked {
        let prio = priority_for_kind(&it.kind);
        groups
            .entry(prio as u8)
            .or_insert((prio, Vec::new()))
            .1
            .push((false, it));
    }
    // Within each kind: pinned first, ranked after (stable order otherwise).
    for (_, (_, list)) in groups.iter_mut() {
        list.sort_by(|a, b| b.0.cmp(&a.0));
    }

    let mut out = String::new();
    let mut tokens = 0usize;
    let mut dropped = 0usize;
    let mut dropped_tokens = 0usize;
    let mut evicted_pinned = 0usize;

    for (_, (prio, list)) in &groups {
        let header = format!("### {}\n", label_for(*prio));
        let header_tok = approx_tokens(&header);
        if tokens + header_tok > budget {
            for (is_pinned, it) in list {
                dropped += 1;
                dropped_tokens += approx_tokens(&item_line(it, *is_pinned));
                if *is_pinned {
                    evicted_pinned += 1;
                }
            }
            continue;
        }
        out.push_str(&header);
        tokens += header_tok;
        for (is_pinned, item) in list {
            let line = item_line(item, *is_pinned);
            let line_tok = approx_tokens(&line);
            if tokens + line_tok > budget {
                dropped += 1;
                dropped_tokens += line_tok;
                if *is_pinned {
                    evicted_pinned += 1;
                }
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
    (out, tokens, evicted_pinned)
}

fn item_line(item: &MemItem, pinned: bool) -> String {
    let body_snip = item.summary.as_ref().unwrap_or(&item.body);
    let snip = truncate_chars(body_snip, 160);
    let snip = snip.replace('\n', " ");
    let lead = if pinned { "★ " } else { "" };
    format!("- {lead}{}: {}\n", item.title, snip)
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

use crate::cmd::print::print_json;

use super::render::format_tab_row;

/// Rank and print tab-find results. Shared between the local socket path
/// (handle_tab) and the mesh-relayed path (main.rs intercepts `--mesh
/// Tab find` so the ranker runs over the peer's list).
pub fn print_tab_find_results(json: bool, pattern: &str, tabs: &[serde_json::Value]) {
    let scored = rank_tabs(pattern, tabs);
    if json {
        let out = serde_json::Value::Array(scored.into_iter().map(|(_, p)| p.clone()).collect());
        print_json(&out);
    } else if scored.is_empty() {
        println!("No tabs match: {}", pattern);
    } else {
        for (_, p) in &scored {
            println!("{}", format_tab_row(p));
        }
    }
}

/// Fuzzy ranker over id / tab.title / session name / tab.title / agent / model. Cross-script via deunicode.
pub fn rank_tabs<'a>(
    pattern: &str,
    tabs: &'a [serde_json::Value],
) -> Vec<(i32, &'a serde_json::Value)> {
    let needle = pattern.to_lowercase();
    let needle_translit = translit(&needle);
    let needle_compact = strip_separators(&needle);
    let needle_translit_compact = strip_separators(&needle_translit);
    let mut scored: Vec<(i32, &serde_json::Value)> = tabs
        .iter()
        .filter_map(|p| {
            let id = field(p, "id");
            let title = field(p, "title");
            let legacy_tab_title = field(p, "tab_title");
            let tab_title = crate::tab_name::tab_name(p).to_lowercase();
            let ws_name = field(p, "workspace_name");
            let sname = field(p, "agent_session_name");
            let agent = field(p, "agent_type");
            let model = field(p, "agent_session_model");
            let mut score = 0i32;
            // id prefix only counts at ≥4 chars — one-letter needles like "a" otherwise match ~6% of all UUIDs and drown the real result.
            if needle.len() >= 4 && id.starts_with(&needle) {
                score += 100;
            }
            score += score_field(&sname, &needle, &needle_translit, 80, 50);
            score += score_field(&tab_title, &needle, &needle_translit, 75, 45);
            if !needle_compact.is_empty() || !needle_translit_compact.is_empty() {
                let sname_c = strip_separators(&sname);
                let sname_c_translit = strip_separators(&translit(&sname));
                if !sname_c.is_empty()
                    && ((!needle_compact.is_empty() && sname_c.contains(&needle_compact))
                        || (!needle_translit_compact.is_empty()
                            && sname_c_translit.contains(&needle_translit_compact)))
                {
                    score += 40;
                }
                let tab_c = strip_separators(&tab_title);
                let tab_c_translit = strip_separators(&translit(&tab_title));
                if !tab_c.is_empty()
                    && ((!needle_compact.is_empty() && tab_c.contains(&needle_compact))
                        || (!needle_translit_compact.is_empty()
                            && tab_c_translit.contains(&needle_translit_compact)))
                {
                    score += 38;
                }
            }
            if !legacy_tab_title.is_empty() && legacy_tab_title != title {
                score += score_field(&title, &needle, &needle_translit, 35, 20);
            }
            if !ws_name.is_empty() {
                let ws_translit = translit(&ws_name);
                if ws_name.contains(&needle) || ws_translit.contains(&needle_translit) {
                    score += 15;
                }
            }
            if !agent.is_empty() && agent.contains(&needle) {
                score += 10;
            }
            if !model.is_empty() && model.contains(&needle) {
                score += 10;
            }
            if score > 0 {
                Some((score, p))
            } else {
                None
            }
        })
        .collect();
    // Stable tie-break by id so identical scores keep a deterministic order.
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| {
            let ai = a.1.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let bi = b.1.get("id").and_then(|v| v.as_str()).unwrap_or("");
            ai.cmp(bi)
        })
    });
    scored
}

fn field(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_lowercase()
}

fn strip_separators(s: &str) -> String {
    // "Dalton" tab. Same for whitespace / dash / underscore which commonly differ between user input and stored titles.
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_' && *c != '\'')
        .collect()
}

/// ASCII-lowercase transliteration. `deunicode` already handles Cyrillic →
/// Latin (Далтон → Dalton), Greek, Jatabse, etc. Returns the lowered form
/// so the result is symmetric with `needle.to_lowercase()`.
fn translit(s: &str) -> String {
    deunicode::deunicode(s).to_lowercase()
}

/// Score one haystack field with both exact-match (high) and contains
/// (medium) tiers, falling back to the transliterated form if the raw
/// match misses. Returning 0 means no contribution.
fn score_field(
    haystack: &str,
    needle: &str,
    needle_translit: &str,
    exact: i32,
    contains: i32,
) -> i32 {
    if haystack.is_empty() {
        return 0;
    }
    if haystack == needle {
        return exact;
    }
    let h_translit = translit(haystack);
    if h_translit == needle_translit && !needle_translit.is_empty() {
        return exact;
    }
    if haystack.contains(needle) {
        return contains;
    }
    if !needle_translit.is_empty() && h_translit.contains(needle_translit) {
        return contains;
    }
    0
}

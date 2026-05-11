use regex::Regex;

use crate::agent::usage_parser::{BreakdownDimension, UsageBreakdownRow};

pub fn parse_all(raw_text: &str, normalized: &str) -> Vec<UsageBreakdownRow> {
    let mut rows = Vec::new();
    rows.extend(parse_heuristics(raw_text));
    rows.extend(parse_named_block(raw_text, "Skills", BreakdownDimension::Skill));
    rows.extend(parse_named_block(raw_text, "Subagents", BreakdownDimension::Subagent));
    rows.extend(parse_named_block(raw_text, "Plugins", BreakdownDimension::Plugin));
    let _ = normalized;
    dedup(rows)
}

fn parse_heuristics(text: &str) -> Vec<UsageBreakdownRow> {
    let re = match Regex::new(r"(\d+)\s*%\s*of\s*your\s*usage([^\n]+)") {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for caps in re.captures_iter(text) {
        let pct: f32 = caps
            .get(1)
            .and_then(|m| m.as_str().parse::<f32>().ok())
            .unwrap_or(0.0);
        let rest = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let key = canonicalize_heuristic(rest);
        if key.is_empty() {
            continue;
        }
        out.push(UsageBreakdownRow {
            dimension: BreakdownDimension::Heuristic,
            key,
            pct,
        });
    }
    out
}

fn canonicalize_heuristic(rest: &str) -> String {
    let mut s = rest.replace('\u{ad}', "");
    s = s.trim().to_string();
    let lower = s.to_ascii_lowercase();
    if lower.contains("subagent") && lower.contains("heavy") {
        return "subagent_heavy_sessions".into();
    }
    if lower.contains("session") && lower.contains("active") && lower.contains("hour") {
        return "long_sessions".into();
    }
    if lower.contains(">150k") || lower.contains("150kcontext") || (lower.contains("context") && lower.contains("150")) {
        return "high_context".into();
    }
    if lower.contains("parallel") || lower.contains("sessionsran") {
        return "parallel_sessions".into();
    }
    if let Some(re) = Regex::new(r#"subagent[s]?[a-z]*under\s*"([^"]+)""#).ok() {
        if let Some(m) = re.captures(&lower) {
            return format!("subagent:{}", m.get(1).map(|x| x.as_str()).unwrap_or(""));
        }
    }
    String::new()
}

fn parse_named_block(text: &str, name: &str, dim: BreakdownDimension) -> Vec<UsageBreakdownRow> {
    let header_re = match Regex::new(&format!(r"(?m){}\s*%\s*of\s*usage\s*\n", regex::escape(name))) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let header_match = match header_re.find(text) {
        Some(m) => m,
        None => return Vec::new(),
    };
    let rest = &text[header_match.end()..];
    let stop = find_next_section(rest);
    let block = &rest[..stop];
    let line_re = Regex::new(r"(?m)^\s*([^\n%]+?)\s*(\d+)\s*%\s*$").ok();
    let mut rows = Vec::new();
    if let Some(re) = line_re {
        for caps in re.captures_iter(block) {
            let key = caps
                .get(1)
                .map(|m| m.as_str().trim().trim_end_matches('…').trim().to_string())
                .unwrap_or_default();
            let pct = caps
                .get(2)
                .and_then(|m| m.as_str().parse::<f32>().ok())
                .unwrap_or(0.0);
            if key.is_empty() {
                continue;
            }
            rows.push(UsageBreakdownRow {
                dimension: dim,
                key,
                pct,
            });
        }
    }
    rows
}

fn find_next_section(rest: &str) -> usize {
    let markers = [
        "Skills%ofusage",
        "Subagents%ofusage",
        "Plugins%ofusage",
        "Extrausage",
        "dtoday",
        "Esctocancel",
    ];
    let mut min = rest.len();
    for m in markers.iter() {
        if let Some(i) = rest.find(m) {
            if i > 0 && i < min {
                min = i;
            }
        }
    }
    min
}

fn dedup(rows: Vec<UsageBreakdownRow>) -> Vec<UsageBreakdownRow> {
    let mut seen: std::collections::HashSet<(BreakdownDimension, String)> =
        std::collections::HashSet::new();
    let mut out = Vec::new();
    for r in rows {
        let key = (r.dimension, r.key.clone());
        if seen.insert(key) {
            out.push(r);
        }
    }
    out
}

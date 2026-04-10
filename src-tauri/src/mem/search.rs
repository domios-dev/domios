//! Search across mem_items via FTS5 BM25 + hybrid scoring.
//!
//! Score formula (ported from a reference memory system hybrid_v3 — empirically validated
//! +1.8pp over raw BM25 on LongMemEval):
//!
//!   fused = bm25 * (1 - 0.30 * keyword_overlap) * (1 - temporal_boost)
//!
//! - bm25: native FTS5 ranking (lower = more relevant)
//! - keyword_overlap: fraction of query terms ≥3 chars (minus stop words)
//!                    appearing literally in title/body/tags
//! - temporal_boost: if query mentions a time anchor ("last week",
//!                   "two weeks ago"), items closer to (now - offset)
//!                   get up to 40% distance reduction
//!
//! Plus: importance bumps highly-curated items by up to 20%.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{params_from_iter, Connection, ToSql};
use serde::{Deserialize, Serialize};

use super::store::MemItem;

const STOP_WORDS: &[&str] = &[
    "what", "when", "where", "who", "how", "which", "did", "do", "does", "was", "were",
    "have", "has", "had", "is", "are", "the", "a", "an", "my", "me", "i", "you", "your",
    "their", "it", "its", "in", "on", "at", "to", "for", "of", "with", "by", "from",
    "ago", "last", "that", "this", "there", "about", "get", "got", "give", "gave",
    "buy", "bought", "made", "make",
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub kind: Option<String>,
    pub scope: Option<String>,
    pub area: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub item: MemItem,
    pub score: f64,
}

pub fn run(
    conn: &Connection,
    q: &SearchQuery,
    blobs_dir: &Path,
) -> Result<Vec<SearchHit>, String> {
    let limit = q.limit.unwrap_or(5).clamp(1, 50);
    let pool = limit * 5; // overfetch then re-rank with hybrid scoring

    let fts_query = sanitize_fts_query(&q.query);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    // Pull candidates from FTS5 with optional kind/scope/area filters.
    let mut sql = String::from(
        r#"SELECT mem_items.id, bm25(mem_fts) AS rank
           FROM mem_fts
           JOIN mem_items ON mem_items.id = mem_fts.mem_id
           WHERE mem_fts MATCH ?1"#,
    );
    let mut binds: Vec<Box<dyn ToSql>> = vec![Box::new(fts_query)];
    if let Some(k) = &q.kind {
        sql.push_str(&format!(" AND mem_items.kind = ?{}", binds.len() + 1));
        binds.push(Box::new(k.clone()));
    }
    if let Some(s) = &q.scope {
        sql.push_str(&format!(" AND mem_items.scope = ?{}", binds.len() + 1));
        binds.push(Box::new(s.clone()));
    }
    if let Some(a) = &q.area {
        sql.push_str(&format!(" AND mem_items.area = ?{}", binds.len() + 1));
        binds.push(Box::new(a.clone()));
    }
    sql.push_str(&format!(" ORDER BY rank LIMIT {}", pool));

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let bind_refs: Vec<&dyn ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_from_iter(bind_refs), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })
        .map_err(|e| e.to_string())?;

    let keywords = extract_keywords(&q.query);
    let temporal = parse_temporal_offset(&q.query);
    let now = chrono::Utc::now();

    let mut hits: Vec<SearchHit> = Vec::new();
    for row in rows {
        let (id, bm25) = row.map_err(|e| e.to_string())?;
        let Some(item) = super::store::get(conn, &id, blobs_dir)? else {
            continue;
        };
        let overlap = keyword_overlap(&keywords, &item);
        let importance_boost = ((item.importance - 3.0) / 5.0).max(0.0) * 0.20;
        let temporal_boost = match &temporal {
            Some(days_ago) => temporal_boost_for(&item, *days_ago, now),
            None => 0.0,
        };
        // Lower fused = better. bm25 is already negative-ish; we work with absolute.
        let abs_bm = bm25.abs();
        let fused =
            abs_bm * (1.0 - 0.30 * overlap) * (1.0 - temporal_boost) * (1.0 - importance_boost);
        hits.push(SearchHit { item, score: fused });
    }

    hits.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(limit);
    Ok(hits)
}

fn sanitize_fts_query(q: &str) -> String {
    let cleaned: String = q
        .chars()
        .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().filter(|w| w.len() >= 2).collect();
    if words.is_empty() {
        String::new()
    } else {
        words.join(" OR ")
    }
}

fn extract_keywords(q: &str) -> HashSet<String> {
    let stops: HashSet<&str> = STOP_WORDS.iter().copied().collect();
    q.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(|w| w.to_lowercase())
        .filter(|w| !stops.contains(w.as_str()))
        .collect()
}

fn keyword_overlap(keywords: &HashSet<String>, item: &MemItem) -> f64 {
    if keywords.is_empty() {
        return 0.0;
    }
    let haystack = format!(
        "{} {} {} {}",
        item.title,
        item.summary.as_deref().unwrap_or(""),
        item.body,
        item.tags.join(" "),
    )
    .to_lowercase();
    let mut hits = 0;
    for kw in keywords {
        if haystack.contains(kw) {
            hits += 1;
        }
    }
    hits as f64 / keywords.len() as f64
}

/// Returns days_ago if the query has a temporal anchor, else None.
fn parse_temporal_offset(q: &str) -> Option<u32> {
    let lc = q.to_lowercase();
    if lc.contains("yesterday") {
        return Some(1);
    }
    if lc.contains("last week") || lc.contains("a week ago") {
        return Some(7);
    }
    if lc.contains("last month") || lc.contains("a month ago") {
        return Some(30);
    }
    if lc.contains("last year") || lc.contains("a year ago") {
        return Some(365);
    }
    // "N days/weeks/months ago"
    let re = regex::Regex::new(r"(\d+)\s+(day|days|week|weeks|month|months|year|years)\s+ago")
        .ok()?;
    let cap = re.captures(&lc)?;
    let n: u32 = cap.get(1)?.as_str().parse().ok()?;
    let unit = cap.get(2)?.as_str();
    let mult: u32 = match unit {
        "day" | "days" => 1,
        "week" | "weeks" => 7,
        "month" | "months" => 30,
        "year" | "years" => 365,
        _ => return None,
    };
    Some(n * mult)
}

fn temporal_boost_for(
    item: &MemItem,
    days_ago: u32,
    now: chrono::DateTime<chrono::Utc>,
) -> f64 {
    let target = now - chrono::Duration::days(days_ago as i64);
    let Ok(created) = chrono::DateTime::parse_from_rfc3339(&item.created_at) else {
        return 0.0;
    };
    let diff_days = (created.with_timezone(&chrono::Utc) - target).num_days().abs() as f64;
    let window = (days_ago as f64 / 2.0).max(3.0);
    if diff_days >= window {
        0.0
    } else {
        0.40 * (1.0 - diff_days / window)
    }
}

/// Reciprocal Rank Fusion: merge FTS5 + vector results into unified ranking.
/// RRF score = Σ 1/(k + rank_i) where k=60 (standard constant).
/// Higher RRF score = more relevant.
const RRF_K: f64 = 60.0;

pub fn hybrid_rrf(
    conn: &Connection,
    q: &SearchQuery,
    blobs_dir: &Path,
) -> Result<Vec<SearchHit>, String> {
    let limit = q.limit.unwrap_or(5).clamp(1, 50);

    // 1. FTS5 candidates (may be empty on typos)
    let fts_hits = run(conn, q, blobs_dir).unwrap_or_default();

    // 2. Vector candidates
    #[cfg(feature = "embeddings")]
    let vec_hits = {
        let pool = limit * 5;
        let raw = super::embeddings::recall(conn, &q.query, pool).unwrap_or_default();
        let mut out = Vec::new();
        for (id, dist) in raw {
            if let Ok(Some(item)) = super::store::get(conn, &id, blobs_dir) {
                // Filter by kind/scope/area if specified
                if let Some(ref k) = q.kind { if item.kind != *k { continue; } }
                if let Some(ref s) = q.scope { if item.scope != *s { continue; } }
                if let Some(ref a) = q.area { if item.area.as_deref() != Some(a) { continue; } }
                out.push(SearchHit { item, score: dist });
            }
        }
        out
    };
    #[cfg(not(feature = "embeddings"))]
    let vec_hits: Vec<SearchHit> = Vec::new();

    // 3. If only one source has results, return it directly
    if fts_hits.is_empty() && vec_hits.is_empty() {
        return Ok(Vec::new());
    }
    if vec_hits.is_empty() {
        return Ok(fts_hits);
    }
    if fts_hits.is_empty() {
        let mut sorted = vec_hits;
        sorted.truncate(limit);
        return Ok(sorted);
    }

    // 4. RRF fusion: combine ranks from both sources
    let mut rrf_scores: HashMap<String, (f64, MemItem)> = HashMap::new();

    for (rank, hit) in fts_hits.iter().enumerate() {
        let rrf = 1.0 / (RRF_K + rank as f64);
        rrf_scores
            .entry(hit.item.id.clone())
            .and_modify(|(s, _)| *s += rrf)
            .or_insert((rrf, hit.item.clone()));
    }

    for (rank, hit) in vec_hits.iter().enumerate() {
        let rrf = 1.0 / (RRF_K + rank as f64);
        rrf_scores
            .entry(hit.item.id.clone())
            .and_modify(|(s, _)| *s += rrf)
            .or_insert((rrf, hit.item.clone()));
    }

    // Higher RRF = better. Apply penalty to auto-extracted items so originals
    // rank above their derivatives. Without this, the sheer count of extracted
    // items pushes originals out of top-5.
    let mut merged: Vec<SearchHit> = rrf_scores
        .into_values()
        .map(|(rrf, item)| {
            let is_auto = item.tags.iter().any(|t| t == "auto-extracted");
            let adjusted = if is_auto { rrf * 0.5 } else { rrf };
            SearchHit { item, score: -adjusted }
        })
        .collect();
    merged.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal));
    merged.truncate(limit);
    Ok(merged)
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
        let tmp = std::env::temp_dir().join(format!("memsrch-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&tmp).unwrap();
        (conn, tmp)
    }

    #[test]
    fn fts_basic_recall() {
        let (conn, blobs) = fresh();
        insert(&conn, &NewMemItem {
            kind: MemKind::Skill, scope: MemScope::Global,
            title: "home assistant".into(), summary: None, area: Some("home".into()),
            tags: vec!["automation".into()], body: "control HA entities, scenes, automations".into(),
            source: None, importance: 3.0,
        }, &blobs).unwrap();
        insert(&conn, &NewMemItem {
            kind: MemKind::Fact, scope: MemScope::Global,
            title: "deploy pipeline".into(), summary: None, area: Some("deploy".into()),
            tags: vec![], body: "uses ./build.sh build for production".into(),
            source: None, importance: 3.0,
        }, &blobs).unwrap();

        let r = run(&conn, &SearchQuery {
            query: "home automation".into(), ..Default::default()
        }, &blobs).unwrap();
        assert!(!r.is_empty());
        assert_eq!(r[0].item.id, "skill_home_assistant");
    }

    #[test]
    fn kind_filter() {
        let (conn, blobs) = fresh();
        insert(&conn, &NewMemItem {
            kind: MemKind::Skill, scope: MemScope::Global,
            title: "auth skill".into(), summary: None, area: None,
            tags: vec![], body: "authentication helpers".into(),
            source: None, importance: 3.0,
        }, &blobs).unwrap();
        insert(&conn, &NewMemItem {
            kind: MemKind::Fact, scope: MemScope::Global,
            title: "auth fact".into(), summary: None, area: None,
            tags: vec![], body: "authentication broken on staging".into(),
            source: None, importance: 3.0,
        }, &blobs).unwrap();

        let r = run(&conn, &SearchQuery {
            query: "authentication".into(),
            kind: Some("fact".into()),
            ..Default::default()
        }, &blobs).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].item.kind, "fact");
    }

    #[test]
    fn keyword_overlap_boosts() {
        let kw: HashSet<String> = ["postgres", "database"].iter().map(|s| s.to_string()).collect();
        let item = MemItem {
            id: "x".into(), uid: "u".into(), kind: "fact".into(), scope: "global".into(),
            title: "Postgres notes".into(), summary: None, area: None, tags: vec![],
            body: "Postgres database tuning".into(), source: None, importance: 3.0,
            created_at: "".into(), updated_at: "".into(), accessed_at: None,
            graph_x: None, graph_y: None,
        };
        assert!((keyword_overlap(&kw, &item) - 1.0).abs() < 0.01);
    }

    #[test]
    fn temporal_offset_parsing() {
        assert_eq!(parse_temporal_offset("what did I do yesterday?"), Some(1));
        assert_eq!(parse_temporal_offset("last week's deploy"), Some(7));
        assert_eq!(parse_temporal_offset("3 weeks ago"), Some(21));
        assert_eq!(parse_temporal_offset("2 months ago"), Some(60));
        assert_eq!(parse_temporal_offset("nothing temporal here"), None);
    }
}

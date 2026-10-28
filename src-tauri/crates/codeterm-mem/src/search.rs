//! Search across mem_items via FTS5 BM25 + hybrid scoring.
//!
//! Score formula (empirically validated +1.8pp over raw BM25 on LongMemEval):
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

use std::collections::HashSet;
use std::path::Path;

use rusqlite::{params_from_iter, Connection, ToSql};
use serde::{Deserialize, Serialize};

use super::store::MemItem;

pub(crate) mod graph_source;
mod lexical;
pub mod ranking;
mod scoped;
use super::embedder::{ActiveEmbedder, StampedVector};
pub(crate) use scoped::scoped_vectors;

const STOP_WORDS: &[&str] = &[
    "what",
    "when",
    "where",
    "who",
    "how",
    "which",
    "did",
    "do",
    "does",
    "was",
    "were",
    "have",
    "has",
    "had",
    "is",
    "are",
    "the",
    "a",
    "an",
    "my",
    "me",
    "i",
    "you",
    "your",
    "their",
    "it",
    "its",
    "in",
    "on",
    "at",
    "to",
    "for",
    "of",
    "with",
    "by",
    "from",
    "ago",
    "last",
    "that",
    "this",
    "there",
    "about",
    "get",
    "got",
    "give",
    "gave",
    "buy",
    "bought",
    "made",
    "make",
    "can",
    "could",
    "would",
    "should",
    "will",
    "am",
    "be",
    "been",
    "being",
    "or",
    "and",
    "but",
    "not",
    "no",
    "yes",
    "if",
    "then",
    "else",
    "also",
    "just",
    "like",
    "so",
    "than",
    "some",
    "any",
    "all",
    "they",
    "them",
    "we",
    "us",
    "our",
    "he",
    "she",
    "his",
    "her",
    "что",
    "как",
    "где",
    "когда",
    "кто",
    "какой",
    "какая",
    "какие",
    "какое",
    "почему",
    "зачем",
    "куда",
    "откуда",
    "это",
    "этот",
    "эта",
    "эти",
    "тот",
    "та",
    "те",
    "у",
    "в",
    "во",
    "на",
    "по",
    "за",
    "с",
    "со",
    "от",
    "до",
    "из",
    "к",
    "о",
    "об",
    "про",
    "для",
    "при",
    "над",
    "под",
    "без",
    "через",
    "я",
    "ты",
    "он",
    "она",
    "оно",
    "мы",
    "вы",
    "они",
    "меня",
    "тебя",
    "его",
    "её",
    "нас",
    "вас",
    "их",
    "мой",
    "моя",
    "моё",
    "мои",
    "твой",
    "твоя",
    "твоё",
    "твои",
    "наш",
    "наша",
    "наше",
    "наши",
    "ваш",
    "ваша",
    "ваше",
    "ваши",
    "есть",
    "быть",
    "был",
    "была",
    "было",
    "были",
    "будет",
    "будут",
    "и",
    "или",
    "но",
    "да",
    "нет",
    "не",
    "ни",
    "же",
    "ли",
    "бы",
    "ведь",
    "уже",
    "ещё",
    "только",
    "также",
    "тоже",
    "очень",
    "так",
    "вот",
    "делать",
    "делает",
    "сделать",
    "сделал",
];

pub fn is_stop_word(term: &str) -> bool {
    STOP_WORDS.contains(&term)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub kind: Option<String>,
    pub scope: Option<String>,
    pub area: Option<String>,
    pub limit: Option<usize>,
}

/// Which retrieval pipeline to use when `hybrid_rrf_with_config` is called.
/// Lets us A/B the existing RRF fusion against a vector-first
/// rerank without changing the public `recall` API surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// Current default: pull FTS candidates with keyword-rerank AND vector
    /// candidates, then fuse both rankings with Reciprocal Rank Fusion.
    HybridRrf,
    /// Vector-first: pull only from vector recall (overfetched),
    /// then rerank with `dist * (1 - w * keyword_overlap) * (1 - temporal_boost)`.
    /// Skips FTS entirely — tolerant of misspellings, fixes the
    /// "specific noun underweighted by cosine" failure mode via the keyword
    /// overlap factor. See benchmarks/HYBRID_MODE.md for the +1.8pp R@5
    /// measurement on LongMemEval.
    VectorFirstRerank,
}

/// Tunable search ranking parameters. `Default::default()` reproduces the
/// production values baked in today; tests/benches may override to A/B.
#[derive(Debug, Clone)]
pub struct SearchConfig {
    pub source_filter: Option<Option<String>>,
    /// RRF k constant. Smaller k → top ranks dominate more.
    pub rrf_k: f64,
    /// Multiplier applied to items tagged `auto-extracted`. <1.0 penalises.
    pub auto_extracted_penalty: f64,
    /// Max recurrence boost (e.g. 0.25 → up to +25% for items saved many times).
    pub recurrence_cap: f64,
    /// Per-recurrence increment (e.g. 0.05 → +5% per save).
    pub recurrence_step: f64,
    /// How strongly keyword_overlap reduces bm25 distance. Higher = FTS leans harder on exact matches.
    pub keyword_overlap_weight: f64,
    /// If true, drop stop-words from FTS query (tighter match). If false, current behaviour (all words joined with OR).
    pub fts_drop_stopwords: bool,
    /// Score reduction applied when any non-stop query keyword appears in the item's title.
    /// 0.0 = no bonus, 0.4 = 40% distance reduction (stronger than general keyword_overlap).
    pub title_match_bonus: f64,
    /// RRF multiplier boost for items connected via graph relations to an entity
    /// whose id (lowercased) contains any of the query keywords. 1.0 = no effect,
    /// 1.5 = +50% post-RRF score. Operates on the fused hybrid rank, not FTS.
    pub graph_entity_boost: f64,
    /// If true, treat graph-related items as a third RRF source alongside
    /// FTS and vector hits. Each graph-matched item contributes 1/(rrf_k+rank)
    /// to the fused score, just like FTS/vector — so graph rank survives even
    /// when title_match_bonus collapses FTS scores to 0.
    pub graph_as_rrf_source: bool,
    /// Retrieval pipeline selector. Default `HybridRrf` = unchanged behaviour.
    /// Flip to `VectorFirstRerank` to test vector-first re-ranking.
    pub mode: SearchMode,
    /// Weight of the keyword-overlap boost in vector-first mode. A full-500
    /// LongMemEval sweep settled on 0.30; higher values overweight
    /// exact-match at the cost of semantic neighbours.
    pub vector_rerank_weight: f64,
    /// Candidate pool multiplier for vector recall in vector-first mode.
    /// The pool is overfetched by
    /// `limit * multiplier`, then truncated post-rerank.
    pub vector_rerank_pool_multiplier: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            source_filter: None,
            rrf_k: 60.0,
            auto_extracted_penalty: 0.5,
            recurrence_cap: 0.25,
            recurrence_step: 0.05,
            // A/B: 0.5 gave +3.3pp Recall@1 over 0.30 after the embed format was extended to carry kind/area/tags alongside title+body.
            keyword_overlap_weight: 0.5,
            // A/B proven: +10pp Recall@1 on 30-query bench (30 EN queries). RU stop words added too but not yet bench-validated on RU queries.
            fts_drop_stopwords: true,
            // - bonus) × bm25 is non-zero until bonus=1.0. 1.0 guarantees title-matchers rank first, which is what we want.
            title_match_bonus: 1.0,
            graph_entity_boost: 1.0,
            graph_as_rrf_source: false,
            // Off by default — default behaviour stays HybridRrf until benches show vector-first wins on our own corpus.
            mode: SearchMode::HybridRrf,
            vector_rerank_weight: 0.30,
            vector_rerank_pool_multiplier: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub item: MemItem,
    pub score: f64,
}

pub fn run(conn: &Connection, q: &SearchQuery, blobs_dir: &Path) -> Result<Vec<SearchHit>, String> {
    run_with_config(conn, q, blobs_dir, &SearchConfig::default())
}

pub fn run_with_config(
    conn: &Connection,
    q: &SearchQuery,
    blobs_dir: &Path,
    cfg: &SearchConfig,
) -> Result<Vec<SearchHit>, String> {
    let limit = q.limit.unwrap_or(5).clamp(1, 50);
    let pool = limit * 5; // overfetch then re-rank with hybrid scoring

    let fts_query = sanitize_fts_query(&q.query, cfg.fts_drop_stopwords);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    // Pull candidates from FTS5 with optional kind/scope/area filters.
    let mut sql = String::from(
        r#"SELECT mem_items.id, bm25(mem_fts) AS rank
           FROM mem_fts
           JOIN mem_items ON mem_items.id = mem_fts.mem_id
           WHERE mem_fts MATCH ?1
             AND mem_items.kind != 'archived'"#,
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
    if let Some(source) = &cfg.source_filter {
        sql.push_str(&format!(" AND mem_items.source IS ?{}", binds.len() + 1));
        binds.push(Box::new(source.clone()));
    }
    sql.push_str(&format!(" ORDER BY rank LIMIT {}", pool));

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let bind_refs: Vec<&dyn ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(params_from_iter(bind_refs), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })
        .map_err(|e| e.to_string())?;

    let scorer = ranking::Scorer::new(&q.query, cfg, chrono::Utc::now());
    let mut hits = Vec::new();
    for row in rows {
        let (id, bm25) = row.map_err(|e| e.to_string())?;
        let Some(item) = super::store::get_or_synthesize_entity(conn, &id, blobs_dir)? else {
            continue;
        };
        let score = scorer.lexical(&item, bm25);
        hits.push(SearchHit { item, score });
    }
    ranking::sort_hits(&mut hits);
    hits.truncate(limit);
    Ok(hits)
}

fn sanitize_fts_query(q: &str, drop_stopwords: bool) -> String {
    let (words, conjunctive) = fts_terms(q, drop_stopwords);
    words.join(if conjunctive { " AND " } else { " OR " })
}

pub fn fts_terms(q: &str, drop_stopwords: bool) -> (Vec<String>, bool) {
    let cleaned: String = q
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();
    let stops: HashSet<&str> = STOP_WORDS.iter().copied().collect();
    let words: Vec<String> = cleaned
        .split_whitespace()
        .filter(|w| w.len() >= 2)
        .filter(|w| !drop_stopwords || !stops.contains(w.to_lowercase().as_str()))
        .map(|w| w.to_string())
        .collect();
    if words.is_empty() && drop_stopwords {
        return fts_terms(q, false);
    }
    (words, is_canonical_issue_query(q))
}

fn is_canonical_issue_query(query: &str) -> bool {
    let value = query.trim().trim_end_matches('/').to_ascii_lowercase();
    let compact = value.split_whitespace().count() == 1;
    if let Some((repository, number)) = value.split_once('#') {
        return compact
            && repository
                .split('/')
                .filter(|part| !part.is_empty())
                .count()
                == 2
            && is_issue_number(number);
    }
    compact
        && value.contains("/issues/")
        && value
            .rsplit_once('/')
            .is_some_and(|(_, number)| is_issue_number(number))
}

fn is_issue_number(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub fn extract_keywords(q: &str) -> HashSet<String> {
    let stops: HashSet<&str> = STOP_WORDS.iter().copied().collect();
    q.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .map(|w| w.to_lowercase())
        .filter(|w| !stops.contains(w.as_str()))
        .collect()
}

/// Find mem_items connected via graph relations to entities whose id contains
/// any of the query keywords (case-insensitive substring on the entity id).
/// Used to boost search results for items linked to relevant people/projects/tech.
fn find_graph_boosted_items(
    conn: &Connection,
    keywords: &HashSet<String>,
) -> Result<HashSet<String>, String> {
    if keywords.is_empty() {
        return Ok(HashSet::new());
    }

    // 1. Find entity ids matching any keyword (e.g. "alex" → person_alex).
    let mut entity_ids: HashSet<String> = HashSet::new();
    {
        let mut stmt = conn
            .prepare("SELECT id FROM entities WHERE lower(id) LIKE ?1")
            .map_err(|e| e.to_string())?;
        for kw in keywords {
            let pattern = format!("%{}%", kw.to_lowercase());
            let rows = stmt
                .query_map([pattern], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            for row in rows.flatten() {
                entity_ids.insert(row);
            }
        }
    }

    if entity_ids.is_empty() {
        return Ok(HashSet::new());
    }

    // 2. Find items related to those entities (both directions).
    let placeholders: Vec<String> = (1..=entity_ids.len()).map(|i| format!("?{}", i)).collect();
    let in_clause = placeholders.join(",");
    let sql = format!(
        "SELECT DISTINCT from_id FROM relations WHERE to_id IN ({in_clause})
         UNION
         SELECT DISTINCT to_id FROM relations WHERE from_id IN ({in_clause})"
    );
    let bind_vec: Vec<String> = entity_ids.iter().cloned().collect();
    let bind_refs: Vec<&dyn ToSql> = bind_vec.iter().map(|s| s as &dyn ToSql).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(bind_refs), |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;

    let mut out = HashSet::new();
    for row in rows.flatten() {
        out.insert(row);
    }
    Ok(out)
}

fn title_has_keyword(keywords: &HashSet<String>, title: &str) -> bool {
    if keywords.is_empty() {
        return false;
    }
    let lc = title.to_lowercase();
    keywords.iter().any(|k| lc.contains(k))
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
    let re =
        regex::Regex::new(r"(\d+)\s+(day|days|week|weeks|month|months|year|years)\s+ago").ok()?;
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

fn temporal_boost_for(item: &MemItem, days_ago: u32, now: chrono::DateTime<chrono::Utc>) -> f64 {
    let target = now - chrono::Duration::days(days_ago as i64);
    let Ok(created) = chrono::DateTime::parse_from_rfc3339(&item.created_at) else {
        return 0.0;
    };
    let diff_days = (created.with_timezone(&chrono::Utc) - target)
        .num_days()
        .abs() as f64;
    let window = (days_ago as f64 / 2.0).max(3.0);
    if diff_days >= window {
        0.0
    } else {
        0.40 * (1.0 - diff_days / window)
    }
}

/// Reciprocal Rank Fusion: merge FTS5 + vector results into unified ranking.
/// RRF score = Σ 1/(k + rank_i). Standard k=60; lower k makes top ranks dominate more.
/// Higher RRF score = more relevant.
pub fn hybrid_rrf(
    conn: &Connection,
    q: &SearchQuery,
    blobs_dir: &Path,
) -> Result<Vec<SearchHit>, String> {
    hybrid_rrf_with_config(conn, q, blobs_dir, &SearchConfig::default())
}

pub fn hybrid_rrf_with_config(
    conn: &Connection,
    q: &SearchQuery,
    blobs_dir: &Path,
    cfg: &SearchConfig,
) -> Result<Vec<SearchHit>, String> {
    let embedding = query_embedding(&q.query);
    hybrid_rrf_with_embedding(conn, q, blobs_dir, cfg, embedding.as_ref())
}

pub(crate) fn query_embedding(query: &str) -> Option<StampedVector> {
    ActiveEmbedder::global().query(query)
}

pub(crate) fn hybrid_rrf_with_embedding(
    conn: &Connection,
    q: &SearchQuery,
    blobs_dir: &Path,
    cfg: &SearchConfig,
    embedding: Option<&StampedVector>,
) -> Result<Vec<SearchHit>, String> {
    if matches!(cfg.mode, SearchMode::VectorFirstRerank) && embedding.is_some() {
        return vector_first_with_embedding(conn, q, blobs_dir, cfg, embedding);
    }
    let limit = q.limit.unwrap_or(5).clamp(1, 50);

    // 1. FTS5 candidates (may be empty on typos)
    let wide_query = SearchQuery {
        limit: Some(limit * 5),
        ..q.clone()
    };
    let fts_hits = run_with_config(conn, &wide_query, blobs_dir, cfg).unwrap_or_default();

    let vec_hits = {
        let pool = limit * 5;
        let raw = embedding
            .and_then(|v| scoped_vectors(conn, v, pool, q, cfg).ok())
            .unwrap_or_default();
        let mut out = Vec::new();
        for (id, dist) in raw {
            if let Ok(Some(item)) = super::store::get_or_synthesize_entity(conn, &id, blobs_dir) {
                if let Some(ref k) = q.kind {
                    if item.kind != *k {
                        continue;
                    }
                }
                if let Some(ref s) = q.scope {
                    if item.scope != *s {
                        continue;
                    }
                }
                if let Some(ref a) = q.area {
                    if item.area.as_deref() != Some(a) {
                        continue;
                    }
                }
                if cfg
                    .source_filter
                    .as_ref()
                    .is_some_and(|source| item.source != *source)
                {
                    continue;
                }
                out.push(SearchHit { item, score: dist });
            }
        }
        out
    };

    let graph = if !fts_hits.is_empty() && !vec_hits.is_empty()
        && (cfg.graph_entity_boost > 1.0 || cfg.graph_as_rrf_source)
    {
        let ids = find_graph_boosted_items(conn, &extract_keywords(&q.query)).unwrap_or_default();
        ids.into_iter().map(|id| {
            let item = cfg.graph_as_rrf_source.then(|| super::store::get_or_synthesize_entity(conn, &id, blobs_dir).ok().flatten()).flatten();
            (id, item)
        }).collect()
    } else {
        Vec::new()
    };
    Ok(ranking::fuse(q, cfg, fts_hits, vec_hits, graph))
}

/// Vector-first retrieval + keyword/temporal re-rank.
///
/// Formula: `fused = cosine_dist × (1 − w × keyword_overlap) × (1 − temporal_boost)`
/// where `w = cfg.vector_rerank_weight` (0.30 from the LongMemEval sweep).
///
/// Differs from `hybrid_rrf_with_config`:
///   - Skips FTS5 entirely — tolerates typos naturally (vector is character-fuzzy).
///   - Single-stage: no RRF fusion, scores are comparable across items.
///   - Keyword overlap is a *boost*, not a *filter* — items with 0 overlap still
///     score by pure cosine distance, so semantically-close-but-lexically-absent
///     items aren't eliminated.
///
/// Filters (kind/scope/area) are applied *after* vector recall. We overfetch
/// `limit × cfg.vector_rerank_pool_multiplier` to give the rerank pool room
/// after filtering.
///
/// Lock duration: caller passes `&Connection` and the per-id `store::get`
/// loop performs blob and source-file disk I/O while that lock is held — up
/// to `pool` (≤250) candidates per call. Inherits caller-side lock duration.
pub fn run_vector_first(
    conn: &Connection,
    q: &SearchQuery,
    blobs_dir: &Path,
    cfg: &SearchConfig,
) -> Result<Vec<SearchHit>, String> {
    let embedding = query_embedding(&q.query);
    vector_first_with_embedding(conn, q, blobs_dir, cfg, embedding.as_ref())
}

fn vector_first_with_embedding(
    conn: &Connection,
    q: &SearchQuery,
    blobs_dir: &Path,
    cfg: &SearchConfig,
    embedding: Option<&StampedVector>,
) -> Result<Vec<SearchHit>, String> {
    let limit = q.limit.unwrap_or(5).clamp(1, 50);
    let raw = {
        let pool = limit * cfg.vector_rerank_pool_multiplier.max(1);
        embedding
            .and_then(|v| scoped_vectors(conn, v, pool, q, cfg).ok())
            .unwrap_or_default()
    };

    if raw.is_empty() {
        return Ok(Vec::new());
    }

    let scorer = ranking::Scorer::new(&q.query, cfg, chrono::Utc::now());
    let mut hits = Vec::with_capacity(raw.len());
    for (id, dist) in raw {
        let Some(item) = super::store::get_or_synthesize_entity(conn, &id, blobs_dir)? else {
            continue;
        };
        if !ranking::matches_filters(&item, q, cfg) {
            continue;
        }
        let score = scorer.vector(&item, dist);
        hits.push(SearchHit { item, score });
    }
    ranking::sort_hits(&mut hits);
    hits.truncate(limit);
    Ok(hits)
}

#[cfg(test)]
mod tests;

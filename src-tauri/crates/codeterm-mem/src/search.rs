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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub kind: Option<String>,
    pub scope: Option<String>,
    pub area: Option<String>,
    pub limit: Option<usize>,
}

/// Which retrieval pipeline to use when `hybrid_rrf_with_config` is called.
/// Lets us A/B the existing RRF fusion against a a reference memory system-style vector-first
/// rerank without changing the public `recall` API surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// Current default: pull FTS candidates with keyword-rerank AND vector
    /// candidates, then fuse both rankings with Reciprocal Rank Fusion.
    HybridRrf,
    /// a reference memory system hybrid_v2 port: pull only from vector recall (overfetched),
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
    /// Flip to `VectorFirstRerank` to test a reference memory system-style re-ranking.
    pub mode: SearchMode,
    /// Weight of the keyword-overlap boost in vector-first mode. a reference memory system's
    /// full-500 sweep settled on 0.30 as optimal; higher values overweight
    /// exact-match at the cost of semantic neighbours.
    pub vector_rerank_weight: f64,
    /// Candidate pool multiplier for vector recall in vector-first mode.
    /// a reference memory system uses fixed n_results=50; we mirror that by fetching
    /// `limit * multiplier` then truncating post-rerank.
    pub vector_rerank_pool_multiplier: usize,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
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
        let Some(item) = super::store::get_or_synthesize_entity(conn, &id, blobs_dir)? else {
            continue;
        };
        let overlap = keyword_overlap(&keywords, &item);
        let importance_boost = ((item.importance - 3.0) / 5.0).max(0.0) * 0.20;
        let recurrence_boost =
            ((item.recurrence_count as f64) * cfg.recurrence_step).min(cfg.recurrence_cap);
        let temporal_boost = match &temporal {
            Some(days_ago) => temporal_boost_for(&item, *days_ago, now),
            None => 0.0,
        };
        let title_boost =
            if cfg.title_match_bonus > 0.0 && title_has_keyword(&keywords, &item.title) {
                cfg.title_match_bonus
            } else {
                0.0
            };
        let abs_bm = bm25.abs();
        let fused = abs_bm
            * (1.0 - cfg.keyword_overlap_weight * overlap)
            * (1.0 - temporal_boost)
            * (1.0 - importance_boost)
            * (1.0 - recurrence_boost)
            * (1.0 - title_boost);
        hits.push(SearchHit { item, score: fused });
    }

    hits.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(limit);
    Ok(hits)
}

fn sanitize_fts_query(q: &str, drop_stopwords: bool) -> String {
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
    if words.is_empty() {
        if drop_stopwords {
            return sanitize_fts_query(q, false);
        }
        String::new()
    } else {
        let operator = if is_canonical_issue_query(q) {
            " AND "
        } else {
            " OR "
        };
        words.join(operator)
    }
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

fn extract_keywords(q: &str) -> HashSet<String> {
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
    if matches!(cfg.mode, SearchMode::VectorFirstRerank) {
        return run_vector_first(conn, q, blobs_dir, cfg);
    }
    let limit = q.limit.unwrap_or(5).clamp(1, 50);

    // 1. FTS5 candidates (may be empty on typos)
    let fts_hits = run_with_config(conn, q, blobs_dir, cfg).unwrap_or_default();

    #[cfg(feature = "embeddings")]
    let vec_hits = {
        let pool = limit * 5;
        let raw = super::embeddings::recall(conn, &q.query, pool).unwrap_or_default();
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
                out.push(SearchHit { item, score: dist });
            }
        }
        out
    };
    #[cfg(not(feature = "embeddings"))]
    let vec_hits: Vec<SearchHit> = Vec::new();

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

    let mut rrf_scores: HashMap<String, (f64, MemItem)> = HashMap::new();

    for (rank, hit) in fts_hits.iter().enumerate() {
        let rrf = 1.0 / (cfg.rrf_k + rank as f64);
        rrf_scores
            .entry(hit.item.id.clone())
            .and_modify(|(s, _)| *s += rrf)
            .or_insert((rrf, hit.item.clone()));
    }

    for (rank, hit) in vec_hits.iter().enumerate() {
        let rrf = 1.0 / (cfg.rrf_k + rank as f64);
        rrf_scores
            .entry(hit.item.id.clone())
            .and_modify(|(s, _)| *s += rrf)
            .or_insert((rrf, hit.item.clone()));
    }

    // Graph-aware: items linked to entities whose id matches query keywords.
    let keywords = extract_keywords(&q.query);
    let graph_boosted: HashSet<String> = if cfg.graph_entity_boost > 1.0 || cfg.graph_as_rrf_source
    {
        find_graph_boosted_items(conn, &keywords).unwrap_or_default()
    } else {
        HashSet::new()
    };

    // Optional: feed graph hits as a third RRF source. This survives the title_match_bonus=1.0 score-collapse trick because RRF works on rank, not on bm25 score.
    if cfg.graph_as_rrf_source && !graph_boosted.is_empty() {
        let mut graph_ids: Vec<&String> = graph_boosted.iter().collect();
        graph_ids.sort();
        for (rank, gid) in graph_ids.iter().enumerate() {
            let rrf = 1.0 / (cfg.rrf_k + rank as f64);
            if let Some(entry) = rrf_scores.get_mut(*gid) {
                entry.0 += rrf;
            } else if let Ok(Some(item)) =
                super::store::get_or_synthesize_entity(conn, gid, blobs_dir)
            {
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
                rrf_scores.insert((*gid).clone(), (rrf, item));
            }
        }
    }

    let mut merged: Vec<SearchHit> = rrf_scores
        .into_values()
        .map(|(rrf, item)| {
            let is_auto = item.tags.iter().any(|t| t == "auto-extracted");
            let auto_mul = if is_auto {
                cfg.auto_extracted_penalty
            } else {
                1.0
            };
            let rec_mul = 1.0
                + ((item.recurrence_count as f64) * cfg.recurrence_step).min(cfg.recurrence_cap);
            let graph_mul = if graph_boosted.contains(&item.id) {
                cfg.graph_entity_boost
            } else {
                1.0
            };
            let adjusted = rrf * auto_mul * rec_mul * graph_mul;
            SearchHit {
                item,
                score: -adjusted,
            }
        })
        .collect();
    merged.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    merged.truncate(limit);
    Ok(merged)
}

/// Vector-first retrieval + keyword/temporal re-rank. Ported from a reference memory system's
/// `build_palace_and_retrieve_hybrid_v2` (`benchmarks/longmemeval_bench.py`).
///
/// Formula: `fused = cosine_dist × (1 − w × keyword_overlap) × (1 − temporal_boost)`
/// where `w = cfg.vector_rerank_weight` (a reference memory system settled on 0.30 for LongMemEval).
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
    let limit = q.limit.unwrap_or(5).clamp(1, 50);
    let pool = limit * cfg.vector_rerank_pool_multiplier.max(1);

    #[cfg(feature = "embeddings")]
    let raw = super::embeddings::recall(conn, &q.query, pool).unwrap_or_default();
    #[cfg(not(feature = "embeddings"))]
    let raw: Vec<(String, f64)> = Vec::new();

    if raw.is_empty() {
        return Ok(Vec::new());
    }

    let keywords = extract_keywords(&q.query);
    let temporal = parse_temporal_offset(&q.query);
    let now = chrono::Utc::now();

    let mut hits: Vec<SearchHit> = Vec::with_capacity(raw.len());
    for (id, dist) in raw {
        let Some(item) = super::store::get_or_synthesize_entity(conn, &id, blobs_dir)? else {
            continue;
        };
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

        let overlap = keyword_overlap(&keywords, &item);
        let temporal_boost = match &temporal {
            Some(days_ago) => temporal_boost_for(&item, *days_ago, now),
            None => 0.0,
        };
        let importance_boost = ((item.importance - 3.0) / 5.0).max(0.0) * 0.20;
        let recurrence_boost =
            ((item.recurrence_count as f64) * cfg.recurrence_step).min(cfg.recurrence_cap);
        let auto_penalty = if item.tags.iter().any(|t| t == "auto-extracted") {
            1.0 / cfg.auto_extracted_penalty.max(1e-6)
        } else {
            1.0
        };

        let fused = dist
            * (1.0 - cfg.vector_rerank_weight * overlap)
            * (1.0 - temporal_boost)
            * (1.0 - importance_boost)
            * (1.0 - recurrence_boost)
            * auto_penalty;

        hits.push(SearchHit { item, score: fused });
    }

    hits.sort_by(|a, b| {
        a.score
            .partial_cmp(&b.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.truncate(limit);
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{init_schema, insert, MemKind, MemScope, NewMemItem};
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
        insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Skill,
                scope: MemScope::Global,
                title: "home assistant".into(),
                summary: None,
                area: Some("home".into()),
                tags: vec!["automation".into()],
                body: "control HA entities, scenes, automations".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            },
            &blobs,
        )
        .unwrap();
        insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "deploy pipeline".into(),
                summary: None,
                area: Some("deploy".into()),
                tags: vec![],
                body: "uses ./build.sh build for production".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            },
            &blobs,
        )
        .unwrap();

        let r = run(
            &conn,
            &SearchQuery {
                query: "home automation".into(),
                ..Default::default()
            },
            &blobs,
        )
        .unwrap();
        assert!(!r.is_empty());
        assert_eq!(r[0].item.id, "skill_home_assistant");
    }

    #[test]
    fn canonical_issue_query_uses_conjunctive_terms() {
        assert_eq!(
            sanitize_fts_query("rollacode/codeterm-releases#18", true),
            "rollacode AND codeterm AND releases AND 18"
        );
        assert!(sanitize_fts_query("dirty worktree report gate", true).contains(" OR "));
    }

    #[test]
    fn kind_filter() {
        let (conn, blobs) = fresh();
        insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Skill,
                scope: MemScope::Global,
                title: "auth skill".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "authentication helpers".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            },
            &blobs,
        )
        .unwrap();
        insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "auth fact".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "authentication broken on staging".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            },
            &blobs,
        )
        .unwrap();

        let r = run(
            &conn,
            &SearchQuery {
                query: "authentication".into(),
                kind: Some("fact".into()),
                ..Default::default()
            },
            &blobs,
        )
        .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].item.kind, "fact");
    }

    #[test]
    fn keyword_overlap_boosts() {
        let kw: HashSet<String> = ["postgres", "database"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let item = MemItem {
            id: "x".into(),
            uid: "u".into(),
            kind: "fact".into(),
            scope: "global".into(),
            title: "Postgres notes".into(),
            summary: None,
            area: None,
            tags: vec![],
            body: "Postgres database tuning".into(),
            source: None,
            importance: 3.0,
            created_at: "".into(),
            updated_at: "".into(),
            accessed_at: None,
            access_count: 0,
            recurrence_count: 0,
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

    #[test]
    fn default_mode_is_hybrid_rrf() {
        // Default behaviour must stay unchanged until benched.
        let cfg = SearchConfig::default();
        assert!(matches!(cfg.mode, SearchMode::HybridRrf));
        assert!((cfg.vector_rerank_weight - 0.30).abs() < 1e-9);
        assert_eq!(cfg.vector_rerank_pool_multiplier, 5);
    }

    /// When the `embeddings` feature is off, vector-first must degrade cleanly
    /// (empty result) rather than panicking. This is the contract callers rely
    /// on: they can set `mode = VectorFirstRerank` and fall back to HybridRrf
    /// via `unwrap_or_default`.
    #[cfg(not(feature = "embeddings"))]
    #[test]
    fn vector_first_returns_empty_without_embeddings() {
        let (conn, blobs) = fresh();
        insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "a".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "b".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            },
            &blobs,
        )
        .unwrap();
        let cfg = SearchConfig {
            mode: SearchMode::VectorFirstRerank,
            ..SearchConfig::default()
        };
        let r = run_vector_first(
            &conn,
            &SearchQuery {
                query: "anything".into(),
                ..Default::default()
            },
            &blobs,
            &cfg,
        )
        .unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn archived_items_excluded_from_search() {
        let (conn, blobs) = fresh();
        insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "active deployment fact".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "active deployment pipeline details".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            },
            &blobs,
        )
        .unwrap();
        // Insert an archived item directly — bypasses the kind enum since 'archived' is a runtime value written by the sweep, not a MemKind variant.
        conn.execute(
            "INSERT INTO mem_items (id, uid, kind, scope, title, importance,
                evidence_count, created_at, updated_at, accessed_at, tags_json,
                is_pinned_wakeup, body_inline)
             VALUES ('fact_archived_deployment', 'fact_archived_deployment', 'archived',
                'global', 'archived deployment fact', 2.0, 1,
                datetime('now','-100 days'), datetime('now','-100 days'),
                NULL, '[]', 0, 'archived deployment pipeline details')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mem_fts (mem_id, title, summary, body, tags)
             VALUES ('fact_archived_deployment', 'archived deployment fact', '', 'archived deployment pipeline details', '')",
            [],
        ).unwrap();
        let hits = run(
            &conn,
            &SearchQuery {
                query: "deployment".into(),
                ..Default::default()
            },
            &blobs,
        )
        .unwrap();
        let ids: Vec<&str> = hits.iter().map(|h| h.item.id.as_str()).collect();
        assert!(
            ids.iter().any(|id| id.contains("active")),
            "active item must appear in search results"
        );
        assert!(
            !ids.contains(&"fact_archived_deployment"),
            "archived item must not appear in search results"
        );
    }

    /// FTS path must behave the same regardless of `mode` — the dispatch only
    /// kicks in via `hybrid_rrf_with_config`. `run` directly hits FTS.
    #[test]
    fn fts_path_unchanged_by_mode_flag() {
        let (conn, blobs) = fresh();
        insert(
            &conn,
            &NewMemItem {
                kind: MemKind::Skill,
                scope: MemScope::Global,
                title: "home assistant".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "scenes and automations".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            },
            &blobs,
        )
        .unwrap();
        let cfg_a = SearchConfig::default();
        let cfg_b = SearchConfig {
            mode: SearchMode::VectorFirstRerank,
            ..SearchConfig::default()
        };
        let q = SearchQuery {
            query: "home automation".into(),
            ..Default::default()
        };
        let a = run_with_config(&conn, &q, &blobs, &cfg_a).unwrap();
        let b = run_with_config(&conn, &q, &blobs, &cfg_b).unwrap();
        assert_eq!(a.len(), b.len());
        assert_eq!(a[0].item.id, b[0].item.id);
    }
}

use std::collections::{HashMap, HashSet};

use super::{extract_keywords, keyword_overlap, lexical, parse_temporal_offset, temporal_boost_for,
    title_has_keyword, MemItem, SearchConfig, SearchHit, SearchQuery};

pub struct Scorer<'a> {
    cfg: &'a SearchConfig,
    keywords: HashSet<String>,
    temporal: Option<u32>,
    now: chrono::DateTime<chrono::Utc>,
}

impl<'a> Scorer<'a> {
    pub fn new(query: &str, cfg: &'a SearchConfig, now: chrono::DateTime<chrono::Utc>) -> Self {
        Self { cfg, keywords: extract_keywords(query), temporal: parse_temporal_offset(query), now }
    }

    fn refined(&self, item: &MemItem, distance: f64, weight: f64) -> f64 {
        let overlap = keyword_overlap(&self.keywords, item);
        let temporal = self.temporal.map_or(0.0, |days| temporal_boost_for(item, days, self.now));
        let importance = ((item.importance - 3.0) / 5.0).max(0.0) * 0.20;
        let recurrence = (item.recurrence_count as f64 * self.cfg.recurrence_step).min(self.cfg.recurrence_cap);
        distance * (1.0 - weight * overlap) * (1.0 - temporal)
            * (1.0 - importance) * (1.0 - recurrence)
    }

    pub fn lexical(&self, item: &MemItem, rank: f64) -> f64 {
        let title = if self.cfg.title_match_bonus > 0.0 && title_has_keyword(&self.keywords, &item.title) {
            self.cfg.title_match_bonus
        } else { 0.0 };
        self.refined(item, 1.0 / (1.0 + rank.abs()), self.cfg.keyword_overlap_weight) * (1.0 - title)
    }

    pub fn vector(&self, item: &MemItem, distance: f64) -> f64 {
        let auto = if item.tags.iter().any(|tag| tag == "auto-extracted") {
            1.0 / self.cfg.auto_extracted_penalty.max(1e-6)
        } else { 1.0 };
        self.refined(item, distance, self.cfg.vector_rerank_weight) * auto
    }
}

pub fn matches_filters(item: &MemItem, query: &SearchQuery, cfg: &SearchConfig) -> bool {
    query.kind.as_ref().is_none_or(|kind| item.kind == *kind)
        && query.scope.as_ref().is_none_or(|scope| item.scope == *scope)
        && query.area.as_ref().is_none_or(|area| item.area.as_ref() == Some(area))
        && cfg.source_filter.as_ref().is_none_or(|source| item.source == *source)
}

pub fn sort_hits(hits: &mut [SearchHit]) {
    hits.sort_by(|a, b| a.score.total_cmp(&b.score).then_with(|| a.item.id.cmp(&b.item.id)));
}

pub fn fuse(
    q: &SearchQuery,
    cfg: &SearchConfig,
    mut fts_hits: Vec<SearchHit>,
    vec_hits: Vec<SearchHit>,
    graph: Vec<(String, Option<MemItem>)>,
) -> Vec<SearchHit> {
    let limit = q.limit.unwrap_or(5).clamp(1, 50);
    if fts_hits.is_empty() && vec_hits.is_empty() {
        return Vec::new();
    }
    if vec_hits.is_empty() {
        lexical::rerank_fts_hits(&q.query, &mut fts_hits);
        fts_hits.truncate(limit);
        return fts_hits;
    }
    if fts_hits.is_empty() {
        let mut sorted = vec_hits;
        sorted.truncate(limit);
        return sorted;
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

    let graph_boosted: HashSet<String> = graph.iter().map(|(id, _)| id.clone()).collect();
    let mut graph_items: HashMap<String, MemItem> = graph.into_iter()
        .filter_map(|(id, item)| item.map(|item| (id, item))).collect();

    if cfg.graph_as_rrf_source && !graph_boosted.is_empty() {
        let mut graph_ids: Vec<&String> = graph_boosted.iter().collect();
        graph_ids.sort();
        for (rank, gid) in graph_ids.iter().enumerate() {
            let rrf = 1.0 / (cfg.rrf_k + rank as f64);
            if let Some(entry) = rrf_scores.get_mut(*gid) {
                entry.0 += rrf;
            } else if let Some(item) =
                graph_items.remove(*gid)
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

    let entries: Vec<(f64, MemItem)> = rrf_scores.into_values().collect();
    let lift = {
        let items: Vec<&MemItem> = entries.iter().map(|(_, item)| item).collect();
        lexical::lift_factors(&q.query, &items)
    };
    let mut merged: Vec<SearchHit> = entries
        .into_iter()
        .zip(lift)
        .map(|((rrf, item), lift)| {
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
            let adjusted = rrf * auto_mul * rec_mul * graph_mul * lift;
            SearchHit {
                item,
                score: -adjusted,
            }
        })
        .collect();
    sort_hits(&mut merged);
    merged.truncate(limit);
    merged
}

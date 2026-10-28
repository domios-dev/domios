//! Lexical lift applied after rank fusion so exact title and rare-keyword
//! hits are not outvoted by generic items that both retrievers return.

use std::collections::HashSet;

use super::{extract_keywords, SearchHit};
use crate::store::MemItem;

const EXACT_TITLE_BOOST: f64 = 4.0;
const RARE_KEYWORD_BOOST: f64 = 8.0;

fn normalize_phrase(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn contains_phrase(haystack: &str, needle: &str) -> bool {
    format!(" {haystack} ").contains(&format!(" {needle} "))
}

fn is_exact_title_match(query: &str, title: &str) -> bool {
    let (q, t) = (normalize_phrase(query), normalize_phrase(title));
    if q.is_empty() || t.is_empty() {
        return false;
    }
    if q == t {
        return true;
    }
    let multi_word = |s: &str| s.contains(' ');
    (multi_word(&q) && contains_phrase(&t, &q)) || (multi_word(&t) && contains_phrase(&q, &t))
}

fn haystack(item: &MemItem) -> String {
    format!(
        "{} {} {} {}",
        item.title,
        item.summary.as_deref().unwrap_or(""),
        item.body,
        item.tags.join(" "),
    )
    .to_lowercase()
}

/// Per-item score multiplier (>= 1.0), aligned with `items`. Coverage weights
/// each query keyword by how rare it is among the candidates.
pub(super) fn lift_factors(query: &str, items: &[&MemItem]) -> Vec<f64> {
    let keywords: HashSet<String> = extract_keywords(query);
    let haystacks: Vec<String> = items.iter().map(|i| haystack(i)).collect();
    let total = items.len() as f64;
    let weights: Vec<(&String, f64)> = keywords
        .iter()
        .map(|k| {
            let df = haystacks.iter().filter(|h| h.contains(k.as_str())).count() as f64;
            (k, ((total + 1.0) / (df + 0.5)).ln())
        })
        .collect();
    let weight_sum: f64 = weights.iter().map(|(_, w)| w).sum();

    items
        .iter()
        .zip(&haystacks)
        .map(|(item, hay)| {
            let exact = if is_exact_title_match(query, &item.title) {
                EXACT_TITLE_BOOST
            } else {
                0.0
            };
            let coverage = if weight_sum > 0.0 {
                weights
                    .iter()
                    .filter(|(k, _)| hay.contains(k.as_str()))
                    .map(|(_, w)| w)
                    .sum::<f64>()
                    / weight_sum
            } else {
                0.0
            };
            (1.0 + exact) * (1.0 + RARE_KEYWORD_BOOST * coverage * coverage)
        })
        .collect()
}

/// Reorder FTS-only hits (lower score = better) by lexical lift, with the lift
/// breaking ties among collapsed zero scores.
pub(super) fn rerank_fts_hits(query: &str, hits: &mut Vec<SearchHit>) {
    let factors = {
        let items: Vec<&MemItem> = hits.iter().map(|h| &h.item).collect();
        lift_factors(query, &items)
    };
    let mut keyed: Vec<(f64, SearchHit)> = factors.into_iter().zip(hits.drain(..)).collect();
    keyed.sort_by(|(fa, a), (fb, b)| {
        (a.score / fa)
            .partial_cmp(&(b.score / fb))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| fb.partial_cmp(fa).unwrap_or(std::cmp::Ordering::Equal))
    });
    hits.extend(keyed.into_iter().map(|(_, h)| h));
}

#[cfg(test)]
mod tests;

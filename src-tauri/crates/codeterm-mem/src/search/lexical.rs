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
mod tests {
    use rusqlite::Connection;

    use crate::search::{hybrid_rrf, SearchQuery};
    use crate::store::{init_schema, insert, MemKind, MemScope, NewMemItem};

    fn seed(conn: &Connection, blobs: &std::path::Path, kind: MemKind, title: &str, body: &str) {
        let item = NewMemItem {
            kind,
            scope: MemScope::Global,
            title: title.into(),
            summary: None,
            area: None,
            tags: vec![],
            body: body.into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        };
        insert(conn, &item, blobs).unwrap();
    }

    fn top(conn: &Connection, blobs: &std::path::Path, query: &str) -> String {
        let q = SearchQuery {
            query: query.into(),
            limit: Some(5),
            ..Default::default()
        };
        hybrid_rrf(conn, &q, blobs).unwrap()[0].item.title.clone()
    }

    #[test]
    fn search_ranks_exact_keyword_first() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let blobs = std::env::temp_dir().join(format!("memlex-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&blobs).unwrap();

        seed(&conn, &blobs, MemKind::Fact, "NAS nightly backup", "restic nightly at 02:00 to Backblaze B2 bucket archivist-b2-night");
        seed(&conn, &blobs, MemKind::Fact, "Laptop backup", "Time Machine weekly backup");
        seed(&conn, &blobs, MemKind::Fact, "Phone photos", "Immich hourly photo upload");
        seed(&conn, &blobs, MemKind::Skill, "codeterm ai terminal control", "run commands in a tab, set the tab name, run agents");
        seed(&conn, &blobs, MemKind::Skill, "verify before claim", "run the check, read the name of the failing test, run it again");

        assert_eq!(top(&conn, &blobs, "restic run"), "NAS nightly backup");
        assert_eq!(top(&conn, &blobs, "when does restic run"), "NAS nightly backup");
        assert_eq!(top(&conn, &blobs, "bucket name"), "NAS nightly backup");
        assert_eq!(top(&conn, &blobs, "Laptop backup"), "Laptop backup");
        assert_eq!(top(&conn, &blobs, "verify before claim"), "verify before claim");
    }
}

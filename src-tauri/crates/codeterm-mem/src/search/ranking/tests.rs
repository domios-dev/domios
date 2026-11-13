use super::{sort_lexical_hits, MemItem, SearchHit};

fn hit(id: &str, score: f64) -> SearchHit {
    SearchHit {
        item: MemItem {
            author: crate::store::MemAuthor::default(),
            id: id.into(),
            uid: id.into(),
            kind: "note".into(),
            scope: "global".into(),
            title: id.into(),
            summary: None,
            area: None,
            tags: Vec::new(),
            body: String::new(),
            source: None,
            importance: 3.0,
            created_at: String::new(),
            updated_at: String::new(),
            accessed_at: None,
            access_count: 0,
            recurrence_count: 0,
        },
        score,
    }
}

#[test]
fn lexical_score_ties_preserve_candidate_order() {
    let mut hits = vec![hit("z", 0.0), hit("a", 0.0), hit("b", -1.0), hit("c", 0.0)];
    sort_lexical_hits(&mut hits);
    assert_eq!(
        hits.iter()
            .map(|hit| hit.item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "z", "a", "c"]
    );
}

#[test]
fn graph_fusion_excludes_archived_candidates() {
    let query = super::SearchQuery {
        query: "template".into(),
        ..Default::default()
    };
    let config = super::SearchConfig {
        graph_as_rrf_source: true,
        ..Default::default()
    };
    let mut retired = hit("retired-template", 0.0).item;
    retired.kind = "archived".into();
    let hits = super::fuse(
        &query,
        &config,
        vec![hit("current-template", 0.0)],
        vec![hit("current-template", 0.0)],
        vec![(retired.id.clone(), Some(retired))],
    );
    assert_eq!(
        hits.iter()
            .map(|hit| hit.item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["current-template"]
    );
}

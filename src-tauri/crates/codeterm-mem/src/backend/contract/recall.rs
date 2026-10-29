use super::{item, MemStore, WriteContext};
use crate::{
    backend::SqliteMemStore,
    embedder::{
        provider::{Content, EmbedError, EmbeddingProvider, Modality},
        ActiveEmbedder, StampedVector,
    },
    search::{SearchConfig, SearchMode},
    Entity, Relation, SearchHit, SearchQuery,
};
use std::sync::Arc;

struct QueryProvider(bool);

impl EmbeddingProvider for QueryProvider {
    fn id(&self) -> &str {
        "recall-contract"
    }
    fn model(&self) -> &str {
        "fixed"
    }
    fn dimension(&self) -> usize {
        3
    }
    fn max_tokens(&self) -> usize {
        512
    }
    fn modalities(&self) -> &[Modality] {
        &[Modality::Text]
    }
    fn remote(&self) -> bool {
        false
    }
    fn embed_passage(&self, _: &Content) -> Result<Vec<f32>, EmbedError> {
        if self.0 {
            Ok(vec![1.0, 0.0, 0.0])
        } else {
            Err(EmbedError::Unavailable("contract outage".into()))
        }
    }
    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.embed_passage(content)
    }
}

pub fn fixture() -> (tempfile::TempDir, SqliteMemStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteMemStore::open_in_memory(dir.path()).unwrap();
    let ctx = WriteContext::system("recall-contract");
    let provider = QueryProvider(true);
    store.init_vectors(&ctx, &provider.identity()).unwrap();
    let data = [
        (
            "Ancient register",
            "chronometer observations",
            "temporal",
            Some(0.75),
        ),
        (
            "Weekly register",
            "chronometer observations",
            "temporal",
            Some(0.70),
        ),
        (
            "Orbital cache recovery",
            "restart the orbital cache recovery procedure",
            "lexical",
            None,
        ),
        (
            "General cache notes",
            "orbital cache recovery general overview",
            "lexical",
            Some(0.99),
        ),
        (
            "Semantic neighbor",
            "unrelated material",
            "lexical",
            Some(0.80),
        ),
        (
            "Primary notebook",
            "aster graph observations",
            "graph",
            Some(0.95),
        ),
        (
            "Linked notebook",
            "aster graph observations",
            "graph",
            Some(0.85),
        ),
        (
            "Bridge notebook",
            "remote connection observations",
            "graph",
            None,
        ),
        (
            "Inactive notebook",
            "remote historical observations",
            "graph",
            None,
        ),
        (
            "Normal register",
            "weighted chronometer observations",
            "weights",
            Some(0.80),
        ),
        (
            "Curated register",
            "weighted chronometer observations",
            "weights",
            Some(0.70),
        ),
        (
            "Extracted register",
            "weighted chronometer observations",
            "weights",
            Some(0.88),
        ),
    ];
    let mut ids = Vec::new();
    for (title, body, area, cosine) in data {
        let mut payload = item(title);
        payload.body = body.into();
        payload.area = Some(area.into());
        payload.importance = 3.0;
        if title == "Primary notebook" {
            payload.importance = 4.0;
        }
        if title == "Curated register" {
            payload.importance = 8.0;
        }
        if title == "Extracted register" {
            payload.tags.push("auto-extracted".into());
        }
        let id = store.save(&ctx, &payload, false).unwrap().id;
        if let Some(cosine) = cosine {
            let cosine: f32 = cosine;
            store
                .insert_vector(
                    &ctx,
                    &id,
                    0,
                    &StampedVector {
                        identity: provider.identity(),
                        vector: vec![cosine, (1.0 - cosine * cosine).sqrt(), 0.0],
                    },
                )
                .unwrap();
        }
        ids.push(id);
    }
    let now = chrono::Utc::now();
    for (id, age) in [(&ids[0], 60), (&ids[1], 7)] {
        store
            .conn
            .lock()
            .execute(
                "UPDATE mem_items SET created_at=?2 WHERE id=?1",
                rusqlite::params![id, (now - chrono::Duration::days(age)).to_rfc3339()],
            )
            .unwrap();
    }
    store
        .conn
        .lock()
        .execute(
            "UPDATE mem_items SET recurrence_count=5 WHERE id=?1",
            [&ids[10]],
        )
        .unwrap();
    store
        .graph_create_entity(
            &ctx,
            &Entity {
                id: "project_aster".into(),
                entity_type: "Project".into(),
                scope: "global".into(),
                properties: serde_json::json!({"name":"Aster"}),
            },
        )
        .unwrap();
    for (index, expired) in [(6, false), (7, false), (8, true)] {
        store
            .graph_relate(
                &ctx,
                &Relation {
                    from_id: ids[index].clone(),
                    to_id: "project_aster".into(),
                    relation_type: "mentions".into(),
                    scope: "global".into(),
                    valid_from: "2026-01-01T00:00:00Z".into(),
                    valid_to: expired.then(|| "2026-02-01T00:00:00Z".into()),
                    confidence: 1.0,
                    properties: serde_json::json!({}),
                    reason: None,
                },
            )
            .unwrap();
    }
    (dir, store)
}

fn query(text: &str, area: &str, limit: usize) -> SearchQuery {
    SearchQuery {
        query: text.into(),
        area: Some(area.into()),
        limit: Some(limit),
        ..Default::default()
    }
}

fn hits(
    store: &dyn MemStore,
    query: &SearchQuery,
    cfg: &SearchConfig,
    available: bool,
) -> Vec<SearchHit> {
    store
        .recall(
            query,
            cfg,
            &ActiveEmbedder::new(Arc::new(QueryProvider(available))),
            false,
        )
        .unwrap()
}

fn titles(hits: &[SearchHit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.item.title.as_str()).collect()
}

pub fn temporal(store: &dyn MemStore) -> Vec<Vec<String>> {
    let cfg = SearchConfig {
        mode: SearchMode::VectorFirstRerank,
        source_filter: Some(None),
        vector_rerank_weight: 0.0,
        title_match_bonus: 0.0,
        ..Default::default()
    };
    let anchored = query("chronometer observations last week", "temporal", 2);
    let recent = hits(store, &anchored, &cfg, true);
    assert_eq!(titles(&recent), ["Weekly register", "Ancient register"]);
    let unanchored = hits(
        store,
        &query("chronometer observations", "temporal", 2),
        &cfg,
        true,
    );
    assert_eq!(titles(&unanchored), ["Ancient register", "Weekly register"]);
    let lexical = hits(store, &anchored, &cfg, false);
    assert_eq!(titles(&lexical), ["Weekly register", "Ancient register"]);
    orders([recent, unanchored, lexical])
}

pub fn lexical_lift(store: &dyn MemStore) -> Vec<Vec<String>> {
    let q = query("Orbital cache recovery", "lexical", 3);
    let cfg = SearchConfig::default();
    let hybrid = hits(store, &q, &cfg, true);
    assert_eq!(
        titles(&hybrid),
        [
            "Orbital cache recovery",
            "General cache notes",
            "Semantic neighbor"
        ]
    );
    let fts = hits(store, &q, &cfg, false);
    assert_eq!(
        titles(&fts),
        ["Orbital cache recovery", "General cache notes"]
    );
    orders([hybrid, fts])
}

pub fn graph(store: &dyn MemStore) -> Vec<Vec<String>> {
    let q = query("aster graph", "graph", 4);
    let cfg = SearchConfig {
        graph_entity_boost: 10.0,
        ..Default::default()
    };
    let boosted = hits(store, &q, &cfg, true);
    assert_eq!(
        titles(&boosted)[..2],
        ["Linked notebook", "Primary notebook"]
    );
    let cfg = SearchConfig {
        graph_as_rrf_source: true,
        ..Default::default()
    };
    let third_source = hits(store, &q, &cfg, true);
    assert_eq!(
        titles(&third_source),
        [
            "Linked notebook",
            "Primary notebook",
            "Bridge notebook",
            "Inactive notebook"
        ]
    );
    let cfg = SearchConfig::default();
    let baseline = hits(store, &q, &cfg, true);
    assert_eq!(titles(&baseline), ["Primary notebook", "Linked notebook"]);
    let additions = store.graph_candidates(&q, 4).unwrap();
    let names = titles(&additions);
    assert!(names.contains(&"Bridge notebook"));
    assert!(!names.contains(&"Inactive notebook"));
    orders([boosted, third_source, baseline, additions])
}

pub fn vector_refinements(store: &dyn MemStore) -> Vec<Vec<String>> {
    let cfg = SearchConfig {
        mode: SearchMode::VectorFirstRerank,
        source_filter: Some(None),
        vector_rerank_weight: 0.0,
        ..Default::default()
    };
    let weighted = hits(store, &query("chronometer", "weights", 3), &cfg, true);
    assert_eq!(
        titles(&weighted),
        ["Curated register", "Normal register", "Extracted register"]
    );
    let cfg = SearchConfig::default();
    let vectors_only = hits(store, &query("unmatchedtypo", "temporal", 2), &cfg, true);
    assert_eq!(
        titles(&vectors_only),
        ["Ancient register", "Weekly register"]
    );
    let cfg = SearchConfig {
        source_filter: Some(Some("absent-source".into())),
        ..cfg
    };
    let excluded = hits(store, &query("chronometer", "temporal", 2), &cfg, true);
    assert!(excluded.is_empty());
    orders([weighted, vectors_only, excluded])
}

fn orders<const N: usize>(groups: [Vec<SearchHit>; N]) -> Vec<Vec<String>> {
    groups
        .into_iter()
        .map(|group| group.into_iter().map(|hit| hit.item.id).collect())
        .collect()
}

#[cfg(test)]
mod tests;

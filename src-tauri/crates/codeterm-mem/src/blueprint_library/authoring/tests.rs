use super::*;
use crate::blueprint_library::{fakes::WordEmbedder, index::Slot};
use codeterm_blueprint_types::{blueprint_document::Document, blueprint_memory::Evidence};
use serde_json::json;

fn callable(name: &str) -> Document {
    serde_json::from_value(json!({"schema_version":2,"kind":"pipeline","name":name,"intent":"opaque integer operation","inputs":{"args":{"type":"integer"}},"steps":[{"id":"calculate","does":"Return an integer","code":{"lang":"python","src":"def main(value): return value"},"in":{"type":"integer"},"out":{"type":"integer"}}],"result":{"type":{"type":"integer"},"from":"calculate.out"}})).unwrap()
}

fn candidate(id: &str, cosine: f32) -> Candidate {
    let document = callable("rank-fixture");
    let intents = vec!["opaque purpose".into()];
    let row = super::super::qualification::index_row(&document, &intents);
    let mut vectors = Vectors::default();
    for slot in [Slot::Intent, Slot::Canon] {
        vectors.by_slot.insert(
            (slot, 0),
            (
                "opaque purpose".into(),
                vec![cosine, (1.0 - cosine * cosine).sqrt()],
            ),
        );
    }
    Candidate {
        entry: Entry {
            id: id.into(),
            document,
            intents,
            uses: 0,
            successes: 0,
            failures: 0,
            kept: false,
            last_used: 0,
        },
        row: Some(row),
        vectors,
    }
}

#[test]
fn blueprint_v2_intent_uses_one_fusion_for_mem_and_catalog_with_known_scores() {
    let hits = recall::rank(
        vec![
            candidate("mem:low", 0.6),
            candidate("catalog:middle", 0.8),
            candidate("mem:high", 1.0),
        ],
        "unrelated query",
        Some(&[1.0, 0.0]),
        None,
        &RecallConfig::default(),
        0,
    );
    assert_eq!(
        hits.iter().map(|h| h.entry.id.as_str()).collect::<Vec<_>>(),
        ["mem:high", "catalog:middle", "mem:low"]
    );
    for (hit, expected) in hits.iter().zip([1.3, 1.04, 0.78]) {
        assert!(
            (hit.score - expected).abs() < 1e-6,
            "{}: {}",
            hit.entry.id,
            hit.score
        );
        assert!(hit.enrichment_pending);
        assert!(hit.why.contains("intent"));
        assert!(hit.why.contains("canon"));
    }
}

#[test]
fn blueprint_v2_intent_requires_vectors_and_an_explicit_scope_before_returning_cards() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(WordEmbedder::new(false));
    let recalled = mem
        .recall_authoring_blueprints("workspace", "opaque integer operation", 3)
        .unwrap();
    assert!(recalled.hits.is_empty());
    assert_eq!(recalled.state.status, "degraded");
    assert!(recalled
        .state
        .reason
        .unwrap()
        .contains("no lexical substitute"));
    for limit in [0, 21, usize::MAX] {
        assert!(mem
            .recall_authoring_blueprints("workspace", "intent", limit)
            .is_err());
    }
    for intent in ["", " ", "bad\0intent"] {
        assert!(mem
            .recall_authoring_blueprints("workspace", intent, 3)
            .is_err());
    }
    assert!(mem.recall_authoring_blueprints("", "intent", 3).is_err());
}

#[test]
fn blueprint_v2_intent_scopes_candidates_before_ranking_and_limit() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(WordEmbedder::new(true));
    let mut local = Vec::new();
    for (scope, name) in [
        ("workspace", "one"),
        ("workspace", "two"),
        ("workspace", "three"),
        ("foreign", "secret"),
    ] {
        let saved = mem
            .save_blueprint_without_merge(
                scope,
                callable(name),
                &["opaque integer operation".into()],
                Evidence {
                    kept: true,
                    ..Default::default()
                },
            )
            .unwrap();
        mem.index_blueprint(&saved.id).unwrap();
        if scope == "workspace" {
            local.push(saved.id);
        }
    }
    let result = mem
        .recall_authoring_blueprints("workspace", "opaque integer operation", 3)
        .unwrap();
    assert_eq!(result.hits.len(), 3);
    assert!(result.hits.iter().all(|hit| local.contains(&hit.entry.id)));
    assert!(result.hits.windows(2).all(|w| w[0].score >= w[1].score));
    let first = mem
        .recall_authoring_blueprints("workspace", "opaque integer operation", 1)
        .unwrap();
    assert_eq!(first.hits.len(), 1);
    assert_eq!(first.hits[0].entry.id, result.hits[0].entry.id);
    assert_eq!(first.hits[0].score, result.hits[0].score);
}

#[test]
fn blueprint_v2_intent_no_fit_keeps_irrelevant_vectors_out() {
    let hits = recall::rank(
        vec![candidate("catalog:unrelated", 0.0)],
        "unrelated query",
        Some(&[1.0, 0.0]),
        None,
        &RecallConfig::default(),
        0,
    );
    assert!(hits.is_empty());
    assert!(recall::rank(
        vec![],
        "query",
        Some(&[1.0, 0.0]),
        None,
        &RecallConfig::default(),
        0
    )
    .is_empty());
}

#[test]
fn blueprint_v2_intent_catalog_vectors_are_available_on_every_platform() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(WordEmbedder::new(true));
    let mut unavailable = Vec::new();
    let candidates = mem
        .catalog_candidates(&mem.embedder.identity(), &mut unavailable)
        .unwrap();
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.entry.id.as_str())
            .collect::<Vec<_>>(),
        catalog::ENTRIES
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()
    );
    assert!(unavailable.is_empty());
    assert!(candidates.iter().all(|candidate| {
        candidate.vectors.slot(Slot::Intent).next().is_some()
            && candidate.vectors.slot(Slot::Canon).next().is_some()
    }));
}

#[test]
fn blueprint_v2_intent_never_uses_vectors_for_replaced_intents_or_stale_enrichment() {
    let mut value = candidate("mem:changed", 1.0);
    value.entry.intents = vec!["replacement intent".into()];
    value.vectors.by_slot.insert(
        (Slot::WorkerIntent, 0),
        ("stale worker intent".into(), vec![1.0, 0.0]),
    );
    value
        .vectors
        .by_slot
        .insert((Slot::Summary, 0), ("stale summary".into(), vec![1.0, 0.0]));
    retain_current_vectors(&mut value);
    assert!(value.vectors.slot(Slot::Intent).next().is_none());
    assert!(value.vectors.slot(Slot::WorkerIntent).next().is_none());
    assert!(value.vectors.slot(Slot::Summary).next().is_none());
}

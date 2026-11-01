use super::{export_all, item};
use crate::backend::{ExportPage, MemStore, StoreRow, StoreTable, StoreValue, WriteContext};
use crate::blueprint_library::{
    enrichment::blueprint_vector_texts,
    index::{Enrichment, Provenance, Slot},
    qualification::{index_row, PreparedBlueprint},
};
use crate::embedder::{ActiveEmbedder, Content, EmbedError, EmbeddingProvider, Modality};
use crate::solution_metadata::Confidence;
use crate::store::MemAuthor;
use codeterm_blueprint_types::{
    blueprint_document::{calls::CallPin, projection, Document},
    blueprint_memory::{CuratorEvidence, Evidence},
};
use serde_json::json;
use std::sync::Arc;

struct Provider;
impl EmbeddingProvider for Provider {
    fn id(&self) -> &str {
        "contract"
    }
    fn model(&self) -> &str {
        "blueprint"
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
        Ok(vec![1.0, 0.0, 0.0])
    }
    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.embed_passage(content)
    }
}

fn embedder() -> Arc<ActiveEmbedder> {
    ActiveEmbedder::new(Arc::new(Provider))
}
fn ctx() -> WriteContext {
    WriteContext::new(
        MemAuthor::owner(Some("blueprint-owner".into())),
        "contract-blueprint",
    )
}
fn document(name: &str, value: i64) -> Document {
    Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":name,"inputs":{"n":{"type":"integer"}},"steps":[{"id":"value","code":{"lang":"python","src":format!("def main(p): return {value}")},"in":{"type":"integer"},"out":{"type":"integer"}}],"result":{"type":{"type":"integer"},"from":"value.out"}})).unwrap()
}
fn prepared(document: Document, team: Option<&str>, successes: u64) -> PreparedBlueprint {
    let intents = vec!["Return fixed data".into()];
    PreparedBlueprint {
        index: index_row(&document, &intents),
        document,
        team: team.map(str::to_owned),
        intents,
        evidence: Evidence {
            kept: true,
            successes,
            origin: Some("contract-origin".into()),
            ..Default::default()
        },
    }
}
fn save(store: &dyn MemStore, document: Document, team: Option<&str>, merge: bool) -> String {
    store
        .blueprint_save(
            &ctx(),
            "contract-source",
            prepared(document, team, 2),
            merge,
            20,
            None,
            &embedder(),
        )
        .unwrap()
        .id
}

pub fn qualified_save_merge_archive_reuse(store: &dyn MemStore) {
    let id = crate::store::with_author(
        MemAuthor::neighbor("ambient".into(), "ambient".into()),
        || save(store, document("constant", 7), None, true),
    );
    assert_eq!(store.get(&id).unwrap().unwrap().author, ctx().author);
    assert!(store
        .get_item_history(&id)
        .unwrap()
        .iter()
        .all(|entry| entry.author == ctx().author && entry.changed_by == ctx().changed_by));
    let merged = save(store, document("renamed", 7), None, true);
    assert_eq!(merged, id);
    let record = store.solution_metadata(&id).unwrap().unwrap();
    assert_eq!(record.blueprint.unwrap().successes, 2);
    store
        .blueprint_reuse(&ctx(), &["missing".into(), id.clone()])
        .unwrap();
    let stats = store
        .solution_metadata(&id)
        .unwrap()
        .unwrap()
        .blueprint
        .unwrap();
    assert_eq!(stats.uses, 1);
    assert!(stats.last_used > 0);
    let snapshot = export_all(store);
    assert_eq!(
        store.blueprint_readonly("contract-source").unwrap().len(),
        1
    );
    assert_eq!(export_all(store), snapshot);
    assert!(store.blueprint_archive(&ctx(), "foreign", &id).is_err());
    assert_eq!(export_all(store), snapshot);
    store
        .blueprint_archive(&ctx(), "contract-source", &id)
        .unwrap();
    assert_eq!(store.get(&id).unwrap().unwrap().kind, "archived");
    assert!(store
        .blueprint_entries(&ctx(), "contract-source", true, None)
        .unwrap()
        .is_empty());
    let history = store.get_item_history(&id).unwrap();
    let archive = history.last().unwrap();
    assert_eq!(archive.author, ctx().author);
    assert_eq!(archive.changed_by, "system:blueprint-library");
    assert_eq!(archive.reason.as_deref(), Some("Archived by owner"));
}

pub fn scoped_save_eviction_and_rollback(store: &dyn MemStore) {
    let first = save(store, document("first", 1), Some("team"), true);
    let foreign = save(store, document("foreign", 1), Some("other"), true);
    assert_ne!(first, foreign);
    let local = store
        .blueprint_candidates(
            &ctx(),
            "contract-source",
            Some("team"),
            Some(&embedder().identity()),
        )
        .unwrap();
    assert_eq!(local.len(), 1);
    assert_eq!(local[0].entry.id, first);
    assert!(local[0].row.is_some());
    let before = export_all(store);
    let conflict = prepared(document("first", 99), Some("team"), 3);
    assert!(store
        .blueprint_save(
            &ctx(),
            "contract-source",
            conflict,
            true,
            20,
            None,
            &embedder()
        )
        .unwrap_err()
        .starts_with("blueprint_name_conflict:"));
    assert_eq!(export_all(store), before);
    let mut next = prepared(document("second", 2), Some("team"), 10);
    next.evidence.kept = false;
    let saved = store
        .blueprint_save(&ctx(), "contract-source", next, true, 1, None, &embedder())
        .unwrap();
    assert_eq!(saved.evicted_ids, vec![first.clone()]);
    assert_eq!(store.get(&first).unwrap().unwrap().kind, "archived");
    assert_eq!(store.get(&foreign).unwrap().unwrap().kind, "skill");
    assert_eq!(
        store
            .blueprint_entries(&ctx(), "contract-source", true, Some("team"))
            .unwrap()[0]
            .id,
        saved.id
    );
}

pub fn call_scope_hash_and_immutable_revisions(store: &dyn MemStore) {
    let original = document("constant", 7);
    let id = save(store, original.clone(), Some("team"), true);
    let reference = format!("mem:{id}");
    let pin = CallPin {
        id: reference.clone(),
        hash: projection::identity(&original).unwrap(),
    };
    assert_eq!(
        store
            .blueprint_call_head("contract-source", "team", &reference)
            .unwrap(),
        original
    );
    assert_eq!(
        store
            .blueprint_call_revision("contract-source", "team", &pin)
            .unwrap(),
        original
    );
    assert!(store
        .blueprint_call_head("foreign", "team", &reference)
        .is_err());
    assert!(store
        .blueprint_call_revision("contract-source", "foreign", &pin)
        .is_err());
    let wrong = CallPin {
        hash: "missing-hash".into(),
        ..pin.clone()
    };
    assert!(store
        .blueprint_call_revision("contract-source", "team", &wrong)
        .unwrap_err()
        .contains("Missing pinned hash"));
    let caller = Document::from_definition(&json!({"schema_version":2,"kind":"pipeline","name":"caller","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"call","call":{"ref":reference,"with":{"n":{"lit":7}}},"out":{"type":"integer"}}],"result":{"to":"notify"}})).unwrap();
    let frozen = store
        .blueprint_pin_calls(&ctx(), "contract-source", "team", caller.clone())
        .unwrap();
    assert_eq!(
        frozen.steps[0].call.as_ref().unwrap().pin.as_ref(),
        Some(&pin)
    );
    let bundle = store
        .blueprint_pin_bundle(&ctx(), "contract-source", "team", caller, &[])
        .unwrap();
    assert_eq!(
        bundle.steps[0].call.as_ref().unwrap().pin.as_ref(),
        Some(&pin)
    );
    let revised = document("constant", 99);
    store
        .update(
            &ctx(),
            &id,
            &crate::store::MemUpdate {
                body: Some(revised.yaml().unwrap()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        store
            .blueprint_call_head("contract-source", "team", &reference)
            .unwrap(),
        revised
    );
    assert_eq!(
        store
            .blueprint_call_revision("contract-source", "team", &pin)
            .unwrap(),
        original
    );
    store
        .blueprint_archive(&ctx(), "contract-source", &id)
        .unwrap();
    assert!(store
        .blueprint_call_head("contract-source", "team", &reference)
        .unwrap_err()
        .contains("Archived callee"));
    assert_eq!(
        store
            .blueprint_call_revision("contract-source", "team", &pin)
            .unwrap(),
        original
    );
    let drift = store
        .blueprint_call_head_for_drift("contract-source", "team", &reference)
        .unwrap();
    assert_eq!(drift.0, revised);
    assert_eq!(drift.1.as_deref(), Some("Archived by owner"));
}

pub fn dream_merge_and_archived_keeper(store: &dyn MemStore) {
    let first = save(store, document("constant", 7), Some("team"), false);
    let second = save(store, document("renamed", 7), Some("team"), false);
    let changes = store.blueprint_dream(&ctx(), &embedder(), 0).unwrap();
    assert_eq!(changes.ids.len(), 1);
    assert_eq!(changes.areas, vec!["contract-source"]);
    let keeper = &changes.ids[0];
    let archived = if keeper == &first { &second } else { &first };
    assert_eq!(store.get(archived).unwrap().unwrap().kind, "archived");
    let drift = store
        .blueprint_call_head_for_drift("contract-source", "team", &format!("mem:{archived}"))
        .unwrap();
    assert_eq!(
        drift.0,
        store
            .blueprint_call_head("contract-source", "team", &format!("mem:{keeper}"))
            .unwrap()
    );
    assert_eq!(
        drift.1,
        Some(format!("blueprint solution merged into {keeper}"))
    );
    assert_eq!(
        store
            .solution_metadata(keeper)
            .unwrap()
            .unwrap()
            .blueprint
            .unwrap()
            .successes,
        4
    );
}

fn enrichment(hash: &str) -> Enrichment {
    Enrichment {
        summary: "Return a constant integer".into(),
        intents: vec!["Retrieve constant data".into()],
        provenance: Provenance {
            model: "contract-worker".into(),
            document_hash: hash.into(),
            created_at: 42,
        },
    }
}

pub fn enrichment_index_vectors_with_stamp(store: &dyn MemStore) {
    let id = save(store, document("constant", 7), None, true);
    let (row, body) = store.blueprint_index_snapshot(&id).unwrap();
    assert_eq!(body, document("constant", 7).yaml().unwrap());
    assert!(row.enrichment_pending());
    assert!(!store
        .blueprint_complete_enrichment(&ctx(), &id, "stale", enrichment("stale"))
        .unwrap());
    assert!(store
        .blueprint_index(&id)
        .unwrap()
        .unwrap()
        .enrichment
        .is_none());
    assert!(store
        .blueprint_complete_enrichment(
            &ctx(),
            &id,
            &row.document_hash,
            enrichment(&row.document_hash)
        )
        .unwrap());
    let (current, intents) = store.blueprint_vector_projection(&id).unwrap().unwrap();
    assert_eq!(
        current.current_enrichment().unwrap().provenance.created_at,
        42
    );
    let texts = blueprint_vector_texts(&current, &intents);
    assert_eq!(
        texts
            .iter()
            .map(|(slot, ordinal, _)| (slot.as_str(), *ordinal))
            .collect::<Vec<_>>(),
        vec![
            ("intent", 0),
            ("canon", 0),
            ("summary", 0),
            ("worker_intent", 0)
        ]
    );
    let embedder = embedder();
    let identity = embedder.identity();
    let key = (Slot::Intent, 0);
    store
        .blueprint_replace_vectors(
            &ctx(),
            &id,
            &embedder,
            &identity,
            &[key],
            &[(key, intents[0].clone(), vec![1.0, 0.0, 0.0])],
        )
        .unwrap();
    let vectors = store.blueprint_vectors(&id, &identity).unwrap();
    assert_eq!(
        vectors.by_slot[&key],
        (intents[0].clone(), vec![1.0, 0.0, 0.0])
    );
    let mut foreign = identity.clone();
    foreign.model = "other".into();
    assert!(store
        .blueprint_vectors(&id, &foreign)
        .unwrap()
        .by_slot
        .is_empty());
    let before = export_all(store);
    assert!(store
        .blueprint_replace_vectors(&ctx(), &id, &embedder, &foreign, &[], &[])
        .is_err());
    assert!(store
        .blueprint_replace_vectors(
            &ctx(),
            &id,
            &embedder,
            &identity,
            &[],
            &[(key, "invalid".into(), vec![1.0])]
        )
        .is_err());
    assert_eq!(export_all(store), before);
    let pair = store.blueprint_pair(&id, &id, &embedder).unwrap();
    assert_eq!(pair.0 .0, pair.1 .0);
    assert_eq!(pair.0 .1.by_slot, pair.1 .1.by_slot);
    store
        .blueprint_replace_vectors(&ctx(), &id, &embedder, &identity, &[], &[])
        .unwrap();
    assert!(store
        .blueprint_vectors(&id, &identity)
        .unwrap()
        .by_slot
        .is_empty());
    assert_eq!(store.blueprint_indexed_items().unwrap(), vec![id]);
}

fn proof(round: u64) -> CuratorEvidence {
    CuratorEvidence {
        plan: "contract-plan".into(),
        round,
        episodes: vec![format!("episode-{round}")],
        granted: round == 2,
    }
}
pub fn curator_skill_metadata_merge(store: &dyn MemStore) {
    let id = save(store, document("constant", 7), None, true);
    let mut skill = item("curated skill");
    skill.kind = crate::MemKind::Skill;
    skill.body = "Use a constant integer".into();
    skill.tags = vec!["curator:candidate:discard".into()];
    let fresh = store
        .curator_skill_save(&ctx(), skill.clone(), None, proof(1), 10)
        .unwrap();
    assert_eq!(store.get(&fresh).unwrap().unwrap().author, ctx().author);
    assert_eq!(
        store
            .solution_metadata(&fresh)
            .unwrap()
            .unwrap()
            .curator
            .unwrap()
            .confidence,
        Confidence::Low
    );
    assert_eq!(
        store
            .curator_skill_save(&ctx(), skill.clone(), Some(&id), proof(1), 20)
            .unwrap(),
        id
    );
    store
        .curator_skill_save(&ctx(), skill.clone(), Some(&id), proof(2), 30)
        .unwrap();
    let metadata = store.solution_metadata(&id).unwrap().unwrap();
    assert_eq!(metadata.blueprint.unwrap().successes, 2);
    assert_eq!(metadata.evidence["contract-origin"].successes, 2);
    assert_eq!(
        metadata.content_fingerprint,
        Some(codeterm_blueprint_types::blueprint_memory::curator::fingerprint(&skill.body))
    );
    let curator = metadata.curator.unwrap();
    assert_eq!(curator.confidence, Confidence::Medium);
    assert_eq!(curator.observed_at, 30);
    assert_eq!(curator.rounds["contract-plan"], 2);
    assert_eq!(
        curator.episodes.into_iter().collect::<Vec<_>>(),
        vec!["episode-1", "episode-2"]
    );
    assert!(curator.grants.contains("contract-plan:2"));
    let saved = store.get(&id).unwrap().unwrap();
    assert!(!saved
        .tags
        .iter()
        .any(|tag| tag.starts_with("curator:candidate:")));
    assert!(saved.body.contains("grant:contract-plan:2"));
    let before = export_all(store);
    assert!(store
        .curator_skill_save(&ctx(), skill, Some("missing"), proof(3), 40)
        .is_err());
    assert_eq!(export_all(store), before);
}

pub fn blueprint_export_import_round_trip(source: &dyn MemStore, target: &dyn MemStore) {
    let id = save(source, document("constant", 7), Some("team"), true);
    let row = source.blueprint_index(&id).unwrap().unwrap();
    source
        .blueprint_complete_enrichment(
            &ctx(),
            &id,
            &row.document_hash,
            enrichment(&row.document_hash),
        )
        .unwrap();
    let embedder = embedder();
    let identity = embedder.identity();
    let key = (Slot::Canon, 0);
    source
        .blueprint_replace_vectors(
            &ctx(),
            &id,
            &embedder,
            &identity,
            &[key],
            &[(key, row.canon, vec![1.0, 0.0, 0.0])],
        )
        .unwrap();
    source
        .import_page(
            &ctx(),
            &ExportPage {
                table: StoreTable::BlueprintCleanupReceipt,
                rows: vec![StoreRow {
                    values: [
                        ("id".into(), StoreValue::Integer(1)),
                        ("removed_count".into(), StoreValue::Integer(7)),
                        (
                            "completed_at".into(),
                            StoreValue::Text("2026-01-01 00:00:00".into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                    blobs: Default::default(),
                }],
                next: None,
            },
        )
        .unwrap();
    let counts = source.table_counts().unwrap();
    for table in [
        StoreTable::BlueprintCallScopes,
        StoreTable::BlueprintCallRevisions,
        StoreTable::BlueprintIndex,
        StoreTable::BlueprintVectors,
        StoreTable::BlueprintCleanupReceipt,
    ] {
        assert_eq!(counts[&table], 1, "{table:?}");
    }
    let pages = export_all(source);
    for page in &pages {
        target
            .import_page(&WriteContext::system("foreign-importer"), page)
            .unwrap();
    }
    assert_eq!(target.table_counts().unwrap(), counts);
    assert_eq!(export_all(target), pages);
    assert_eq!(
        target.blueprint_index(&id).unwrap(),
        source.blueprint_index(&id).unwrap()
    );
    assert_eq!(
        target.blueprint_vectors(&id, &identity).unwrap().by_slot,
        source.blueprint_vectors(&id, &identity).unwrap().by_slot
    );
    assert_eq!(target.get(&id).unwrap().unwrap().author, ctx().author);
}

pub fn source_filtered_solution_candidates(store: &dyn MemStore) {
    let embedder = embedder();
    let identity = embedder.identity();
    store.init_vectors(&ctx(), &identity).unwrap();
    let mut payload = item("contract astrolabe solution");
    payload.kind = crate::MemKind::Skill;
    payload.source = Some("library".into());
    let local = store.insert(&ctx(), &payload).unwrap();
    payload.source = Some("foreign".into());
    payload.title = "foreign astrolabe solution".into();
    let foreign = store.insert(&ctx(), &payload).unwrap();
    payload.source = None;
    payload.title = "unowned astrolabe solution".into();
    let unowned = store.insert(&ctx(), &payload).unwrap();
    let vector = crate::embedder::StampedVector {
        identity,
        vector: vec![1.0, 0.0, 0.0],
    };
    for id in [&local, &foreign, &unowned] {
        store.insert_vector(&ctx(), id, 0, &vector).unwrap();
    }
    let query = crate::SearchQuery {
        query: "astrolabe".into(),
        kind: Some("skill".into()),
        area: Some("contract-source".into()),
        limit: Some(20),
        ..Default::default()
    };
    let (hits, vectors) = store
        .solution_candidates(&query, Some("library"), Some(&vector))
        .unwrap();
    assert_eq!(
        hits.iter().map(|hit| &hit.item.id).collect::<Vec<_>>(),
        vec![&local]
    );
    assert_eq!(vectors.into_iter().collect::<Vec<_>>(), vec![local]);
    let (hits, vectors) = store
        .solution_candidates(&query, None, Some(&vector))
        .unwrap();
    assert_eq!(
        hits.iter().map(|hit| &hit.item.id).collect::<Vec<_>>(),
        vec![&unowned]
    );
    assert_eq!(vectors.into_iter().collect::<Vec<_>>(), vec![unowned]);
}

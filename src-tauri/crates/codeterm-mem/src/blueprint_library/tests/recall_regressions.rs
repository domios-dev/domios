use super::*;
use crate::embedder::{provider, Content, EmbedError, EmbeddingProvider, Modality};
use std::sync::Arc;

struct DeploymentEmbedder;

impl DeploymentEmbedder {
    fn vector(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0; self.dimension()];
        if matches!(
            text.to_lowercase().as_str(),
            "detect stalled deployments" | "watch ci failure" | "identify frozen releases"
        ) {
            vector[0] = 1.0;
        }
        vector
    }
}

impl EmbeddingProvider for DeploymentEmbedder {
    fn id(&self) -> &str {
        "fixture"
    }
    fn model(&self) -> &str {
        "deployments"
    }
    fn dimension(&self) -> usize {
        384
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
    fn embed_passage(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        let text = provider::supported_text(self.modalities(), content)?;
        Ok(self.vector(text))
    }

    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        let text = provider::supported_text(self.modalities(), content)?;
        let mut vector = self.vector(text);
        if vector[0] == 0.0 {
            vector[1] = 1.0;
        }
        Ok(vector)
    }
}

fn recall(mem: &MemManager, area: &str, query: &str, limit: usize) -> Vec<BlueprintHit> {
    mem.recall_blueprint_hits(area, query, None, limit, &RecallConfig::default())
        .unwrap()
}

#[test]
fn blueprint_recall_uses_vectors_without_shared_words_and_scopes_before_limit() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(Arc::new(DeploymentEmbedder));
    let intent = "Detect stalled deployments";
    let query = "Identify frozen releases";
    assert!(!intent.split_whitespace().any(|word| query.contains(word)));
    let id = mem
        .save_blueprint("project", document("done"), &when(intent), evidence())
        .unwrap()
        .id;
    let foreign = mem
        .save_blueprint("elsewhere", document("failed"), &when(intent), evidence())
        .unwrap()
        .id;
    for index in 0..300 {
        store::insert(
            &mem.conn.lock(),
            &NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: format!("Unrelated record {index}"),
                summary: None,
                area: Some("project".into()),
                tags: vec![],
                body: "unrelated populated memory".into(),
                source: None,
                importance: 1.0,
                source_path: None,
                pinned: false,
            },
            &mem.blobs_dir,
        )
        .unwrap();
    }
    assert!(mem
        .search(&crate::SearchQuery {
            query: query.into(),
            ..Default::default()
        })
        .unwrap()
        .is_empty());
    let hits = recall(&mem, "project", query, 1);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].entry.id, id);
    let mut ids = recall(&mem, "", query, 5)
        .into_iter()
        .map(|hit| hit.entry.id)
        .collect::<Vec<_>>();
    ids.sort();
    let mut expected = vec![id, foreign];
    expected.sort();
    assert_eq!(ids, expected);
}

#[test]
fn blueprint_usage_breaks_equal_retrieval_scores() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(Arc::new(DeploymentEmbedder));
    mem.save_blueprint(
        "project",
        document("done"),
        &when("Watch CI failure"),
        evidence(),
    )
    .unwrap();
    let popular = mem
        .save_blueprint(
            "project",
            document("failed"),
            &when("Watch CI failure"),
            evidence(),
        )
        .unwrap()
        .id;
    mem.reuse_blueprints(std::slice::from_ref(&popular))
        .unwrap();
    assert_eq!(
        recall(&mem, "project", "Identify frozen releases", 1)[0]
            .entry
            .id,
        popular
    );
}

#[test]
fn recall_reembed_lone_unrelated_kept_solution_is_rejected_despite_rrf_rank() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(Arc::new(DeploymentEmbedder));
    let id = mem
        .save_blueprint(
            "project",
            document("done"),
            &when("Detect stalled deployments"),
            evidence(),
        )
        .unwrap()
        .id;
    for query in [
        "tell me when every worker in the team has finished",
        "rotate the log files each night and compress them",
    ] {
        assert!(recall(&mem, "project", query, 3).is_empty());
    }
    assert_eq!(
        recall(&mem, "project", "Identify frozen releases", 3)[0]
            .entry
            .id,
        id
    );
}

#[test]
fn blueprint_recall_rejects_incidental_keyword_overlap() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.save_blueprint(
        "project",
        document("done"),
        &when("Detect stalled deployments"),
        evidence(),
    )
    .unwrap();
    assert!(recall(
        &mem,
        "project",
        "rotate deployments log files each night and compress them",
        3
    )
    .is_empty());
}

#[test]
fn blueprint_vectors_follow_the_active_embedder_and_refuse_stale_stamps() {
    use crate::blueprint_library::index::{self, Slot};
    use crate::embedder::testing::{fixture, wait_for, MockProvider, StepClock};
    use crate::embedder::{EmbedderEvent, ReembedState};

    let old = MockProvider::new("fixture-old", "old-model", 384);
    let fx = fixture(old.clone(), StepClock::new());
    let intent = "Detect stalled deployments";
    let id = fx
        .mem
        .save_blueprint("project", document("done"), &when(intent), evidence())
        .unwrap()
        .id;
    let old_identity = old.identity();
    let old_vectors = index::read_vectors(&fx.mem.conn.lock(), &id, &old_identity).unwrap();
    assert!(old_vectors.slot(Slot::Intent).next().is_some());

    let new = MockProvider::new("fixture-new", "wide-model", 1536);
    let plan = fx.mem.switch_embedder(new.clone()).unwrap();
    assert_eq!(plan.blueprints, 1);
    let event = wait_for(&fx.events, |event| {
        matches!(event,
        EmbedderEvent::Progress(progress) if progress.state == ReembedState::Completed)
    });
    let EmbedderEvent::Progress(progress) = event else {
        unreachable!()
    };
    assert_eq!(progress.failed, 0);
    assert_eq!(progress.done, plan.total());
    let identity = new.identity();
    let vectors = index::read_vectors(&fx.mem.conn.lock(), &id, &identity).unwrap();
    assert!(vectors
        .slot(Slot::Intent)
        .all(|(_, vector)| vector.len() == 1536));
    assert_eq!(vectors.slot(Slot::Intent).count(), 1);
    assert!(index::read_vectors(&fx.mem.conn.lock(), &id, &old_identity)
        .unwrap()
        .by_slot
        .is_empty());
    let config = RecallConfig {
        weights: recall::Weights {
            lexical: 0.0,
            canon: 0.0,
            ..RecallConfig::default().weights
        },
        ..RecallConfig::default()
    };
    assert_eq!(
        fx.mem
            .recall_blueprint_hits("project", intent, None, 1, &config)
            .unwrap()[0]
            .entry
            .id,
        id
    );
    let conn = fx.mem.conn.lock();
    let stale = index::replace_vectors(&conn, &id, &fx.embedder, &old_identity, &[], &[]);
    assert!(stale.unwrap_err().contains("mixed-model"));
    conn.execute(
        "UPDATE blueprint_vectors SET provider=?1, model=?2 WHERE item_id=?3",
        params![old_identity.provider, old_identity.model, id],
    )
    .unwrap();
    drop(conn);
    assert!(fx
        .mem
        .recall_blueprint_hits("project", intent, None, 1, &config)
        .unwrap()
        .is_empty());
    fx.mem.index_blueprint(&id).unwrap();
    assert_eq!(
        fx.mem
            .recall_blueprint_hits("project", intent, None, 1, &config)
            .unwrap()[0]
            .entry
            .id,
        id
    );
}

#[test]
fn recall_reembed_blueprint_semantic_hits_and_intent_status_survive_shadow_build() {
    use crate::embedder::testing::{fixture_for, MockProvider, DEADLINE};
    let fx = fixture_for(Arc::new(DeploymentEmbedder));
    let id = fx
        .mem
        .save_blueprint(
            "project",
            document("done"),
            &when("Detect stalled deployments"),
            evidence(),
        )
        .unwrap()
        .id;
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    let new = MockProvider::new("new", "new", 768);
    let (started, release) = new.gated();
    fx.mem.switch_embedder(new).unwrap();
    started.recv_timeout(DEADLINE).unwrap();
    let offers = fx
        .mem
        .recall_blueprints("project", "Identify frozen releases", 5)
        .unwrap();
    let recalled = fx
        .mem
        .recall_authoring_blueprints("project", "team", "Identify frozen releases", 5)
        .unwrap();
    drop(release);
    assert_eq!(offers[0].entry.id, id);
    assert_eq!(
        recalled.state.reembed.state,
        crate::embedder::ReembedState::Running
    );
    assert_eq!(recalled.state.reembed.done, 0);
    assert!(recalled.state.reembed.total > 0);
}

#[test]
fn recall_reembed_shadow_blueprint_vectors_equal_direct_indexing_bytes() {
    use crate::embedder::testing::{fixture_for, wait_for, MockProvider, DEADLINE};
    use crate::embedder::{EmbedderEvent, ReembedState};
    let fx = fixture_for(Arc::new(DeploymentEmbedder));
    let id = fx
        .mem
        .save_blueprint(
            "project",
            document("done"),
            &when("Detect stalled deployments"),
            evidence(),
        )
        .unwrap()
        .id;
    assert!(fx.mem.embed_queue.wait_idle(DEADLINE));
    fx.mem
        .switch_embedder(MockProvider::new("new", "new", 768))
        .unwrap();
    wait_for(
        &fx.events,
        |event| matches!(event, EmbedderEvent::Progress(progress) if progress.state == ReembedState::Completed),
    );
    let rows = || -> Vec<(String, i64, String, Vec<u8>)> {
        let conn = fx.mem.conn.lock();
        let mut statement = conn
            .prepare("SELECT slot, ordinal, text, embedding FROM blueprint_vectors WHERE item_id=?1 ORDER BY slot, ordinal")
            .unwrap();
        statement
            .query_map([&id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let published = rows();
    assert!(!published.is_empty());
    fx.mem
        .conn
        .lock()
        .execute("DELETE FROM blueprint_vectors WHERE item_id=?1", [&id])
        .unwrap();
    fx.mem.index_blueprint(&id).unwrap();
    assert_eq!(rows(), published);
}

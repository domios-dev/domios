use super::*;
use std::sync::Arc;

struct DeploymentEmbedder;

impl DeploymentEmbedder {
    fn vector(text: &str) -> Vec<f32> {
        let mut vector = vec![0.0; crate::embeddings::EMBEDDING_DIMENSION];
        if matches!(text, "Detect stalled deployments" | "Watch CI failure" | "Identify frozen releases") {
            vector[0] = 1.0;
        }
        vector
    }
}

impl BlueprintEmbedder for DeploymentEmbedder {
    fn passage(&self, text: &str) -> Result<Vec<f32>, String> {
        Ok(Self::vector(text))
    }

    fn query(&self, text: &str) -> Result<Vec<f32>, String> {
        let mut vector = Self::vector(text);
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
    mem.save_blueprint("project", document("done"), &when("Watch CI failure"), evidence())
        .unwrap();
    let popular = mem
        .save_blueprint("project", document("failed"), &when("Watch CI failure"), evidence())
        .unwrap()
        .id;
    mem.reuse_blueprints(std::slice::from_ref(&popular)).unwrap();
    assert_eq!(recall(&mem, "project", "Identify frozen releases", 1)[0].entry.id, popular);
}

#[test]
fn blueprint_recall_refuses_unrelated_vectors_even_when_the_only_solution_is_kept() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.set_blueprint_embedder(Arc::new(DeploymentEmbedder));
    let id = mem
        .save_blueprint("project", document("done"), &when("Detect stalled deployments"), evidence())
        .unwrap()
        .id;
    for query in [
        "tell me when every worker in the team has finished",
        "rotate the log files each night and compress them",
    ] {
        assert!(recall(&mem, "project", query, 3).is_empty());
    }
    assert_eq!(recall(&mem, "project", "Identify frozen releases", 3)[0].entry.id, id);
}

#[test]
fn blueprint_recall_rejects_incidental_keyword_overlap() {
    let mem = MemManager::open_in_memory().unwrap();
    mem.save_blueprint("project", document("done"), &when("Detect stalled deployments"), evidence())
        .unwrap();
    assert!(recall(&mem, "project", "rotate deployments log files each night and compress them", 3).is_empty());
}

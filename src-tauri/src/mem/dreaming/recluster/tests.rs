use super::*;
use crate::mem::store::{MemKind, MemScope, NewMemItem};
use crate::mem::MemManager;

struct StubCaller {
    response: String,
}

#[async_trait]
impl LlmCaller for StubCaller {
    async fn complete(&self, _prompt: &str, _model: &str) -> Result<String, String> {
        Ok(self.response.clone())
    }
}

struct CapturePromptCaller {
    captured: std::sync::Mutex<String>,
}

#[async_trait]
impl LlmCaller for CapturePromptCaller {
    async fn complete(&self, prompt: &str, _model: &str) -> Result<String, String> {
        *self.captured.lock().unwrap() = prompt.to_string();
        Ok("{\"clusters\": []}".into())
    }
}

fn make_entity(conn: &Connection, id: &str, etype: &str) {
    crate::mem::graph::insert_entity(
        conn,
        &crate::mem::graph::Entity {
            id: id.into(),
            entity_type: etype.into(),
            scope: "global".into(),
            properties: serde_json::json!({"label": id}),
        },
    )
    .unwrap();
}

fn make_relation(conn: &Connection, from: &str, rel: &str, to: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    crate::mem::graph::insert_relation(
        conn,
        &crate::mem::graph::Relation {
            from_id: from.into(),
            relation_type: rel.into(),
            to_id: to.into(),
            scope: "global".into(),
            valid_from: now,
            valid_to: None,
            confidence: 1.0,
            properties: serde_json::json!({}),
            reason: None,
        },
    )
    .unwrap();
}

fn seed_basic_graph(conn: &Connection) {
    // Hub with 3 neighbors plus existing topic anchor.
    make_entity(conn, "hub_andrey", "person");
    for n in ["thing_a", "thing_b", "thing_c"] {
        make_entity(conn, n, "thing");
        make_relation(conn, "hub_andrey", "mentions", n);
    }
    make_entity(conn, "topic_codeterm_mesh_topology", "Topic");
    make_entity(conn, "role_release_host_windows", "Role");
}

#[tokio::test]
async fn recluster_prompt_includes_existing_anchors() {
    let mem = MemManager::open_in_memory().unwrap();
    {
        let c = mem.conn().lock();
        seed_basic_graph(&c);
    }
    let dir = tempfile::tempdir().unwrap();
    let cap = CapturePromptCaller { captured: std::sync::Mutex::new(String::new()) };

    {
        let c = mem.conn().lock();
        run_recluster_phase(&c, "haiku", dir.path(), &cap).await.unwrap();
    }
    let prompt = cap.captured.lock().unwrap().clone();
    assert!(prompt.contains("topic_codeterm_mesh_topology"), "anchor missing from prompt");
    assert!(prompt.contains("role_release_host_windows"), "role anchor missing");
    assert!(prompt.contains("hub_andrey"), "hub missing from prompt");
}

#[tokio::test]
async fn recluster_validates_anchor_exists() {
    let mem = MemManager::open_in_memory().unwrap();
    {
        let c = mem.conn().lock();
        seed_basic_graph(&c);
    }
    let dir = tempfile::tempdir().unwrap();
    let resp = serde_json::json!({
        "clusters": [{
            "topic_id": "topic_invented",
            "entity_type": "Topic",
            "anchor": "ghost_does_not_exist",
            "sources": ["thing_a"],
            "rationale": "test"
        }]
    }).to_string();
    let stub = StubCaller { response: resp };

    let report = {
        let c = mem.conn().lock();
        run_recluster_phase(&c, "haiku", dir.path(), &stub).await.unwrap()
    };
    assert_eq!(report.clusters_proposed, 0, "missing-anchor cluster must be skipped");
    assert!(report.message.contains("ghost_does_not_exist"));
}

#[tokio::test]
async fn recluster_validates_source_exists() {
    let mem = MemManager::open_in_memory().unwrap();
    {
        let c = mem.conn().lock();
        seed_basic_graph(&c);
    }
    let dir = tempfile::tempdir().unwrap();
    let resp = serde_json::json!({
        "clusters": [{
            "topic_id": "topic_realtopic",
            "entity_type": "Topic",
            "anchor": "hub_andrey",
            "sources": ["thing_a", "totally_missing_id"],
            "rationale": "test"
        }]
    }).to_string();
    let stub = StubCaller { response: resp };

    let report = {
        let c = mem.conn().lock();
        run_recluster_phase(&c, "haiku", dir.path(), &stub).await.unwrap()
    };
    assert_eq!(report.clusters_proposed, 0, "missing-source cluster must be skipped");
    assert!(report.message.contains("totally_missing_id"));
}

#[tokio::test]
async fn recluster_validates_topic_id_slug() {
    let mem = MemManager::open_in_memory().unwrap();
    {
        let c = mem.conn().lock();
        seed_basic_graph(&c);
    }
    let dir = tempfile::tempdir().unwrap();
    let resp = serde_json::json!({
        "clusters": [{
            "topic_id": "Topic_Bad-Slug",
            "entity_type": "Topic",
            "anchor": "hub_andrey",
            "sources": ["thing_a"],
            "rationale": "test"
        }]
    }).to_string();
    let stub = StubCaller { response: resp };

    let report = {
        let c = mem.conn().lock();
        run_recluster_phase(&c, "haiku", dir.path(), &stub).await.unwrap()
    };
    assert_eq!(report.clusters_proposed, 0, "invalid slug must be skipped");
    assert!(report.message.to_lowercase().contains("invalid topic_id"));
}

#[tokio::test]
async fn recluster_skips_self_referential_sources() {
    let mem = MemManager::open_in_memory().unwrap();
    {
        let c = mem.conn().lock();
        seed_basic_graph(&c);
    }
    let dir = tempfile::tempdir().unwrap();
    let resp = serde_json::json!({
        "clusters": [{
            "topic_id": "topic_realtopic",
            "entity_type": "Topic",
            "anchor": "hub_andrey",
            "sources": ["hub_andrey", "thing_a"],
            "rationale": "test"
        }]
    }).to_string();
    let stub = StubCaller { response: resp };

    let report = {
        let c = mem.conn().lock();
        run_recluster_phase(&c, "haiku", dir.path(), &stub).await.unwrap()
    };
    assert_eq!(report.clusters_proposed, 0, "self-referential cluster must be skipped");
    assert!(report.message.contains("source equal to anchor"));
}

#[tokio::test]
async fn recluster_writes_plan_file_to_disk() {
    let mem = MemManager::open_in_memory().unwrap();
    {
        let c = mem.conn().lock();
        seed_basic_graph(&c);
    }
    let id = mem.save(NewMemItem {
        kind: MemKind::Fact,
        scope: MemScope::Global,
        title: "fact about thing_a".into(),
        summary: None,
        area: None,
        tags: vec![],
        body: "x".into(),
        source: None,
        importance: 3.0,
        source_path: None,
        pinned: false,
    }).unwrap();
    let dir = tempfile::tempdir().unwrap();

    let resp = serde_json::json!({
        "clusters": [{
            "topic_id": "topic_grouped_things",
            "entity_type": "Topic",
            "anchor": "hub_andrey",
            "sources": ["thing_a", "thing_b", &id],
            "rationale": "all three describe the same hub aspect"
        }]
    }).to_string();
    let stub = StubCaller { response: resp };

    let report = {
        let c = mem.conn().lock();
        run_recluster_phase(&c, "haiku", dir.path(), &stub).await.unwrap()
    };
    assert_eq!(report.clusters_proposed, 1, "valid cluster must land");
    let plan_path_str = report.plan_path.expect("plan_path missing");
    let plan_path = std::path::PathBuf::from(&plan_path_str);
    assert!(plan_path.exists(), "plan file must exist on disk");

    // Round-trip parse must work.
    let md = std::fs::read_to_string(&plan_path).unwrap();
    let parsed = plan_format::read(&md).unwrap();
    assert_eq!(parsed.plan.clusters.len(), 1);
    assert_eq!(parsed.plan.clusters[0].topic_id, "topic_grouped_things");
    assert_eq!(parsed.plan.clusters[0].sources.len(), 3);
    assert_eq!(parsed.plan.clusters[0].rel_source, "about");
    assert_eq!(parsed.plan.clusters[0].rel_anchor, "about");
    assert!(parsed.plan.clusters[0].reason.as_deref().unwrap().starts_with("dreamer-recluster-"));
}

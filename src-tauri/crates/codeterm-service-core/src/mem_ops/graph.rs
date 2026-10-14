use serde::Deserialize;

#[derive(Deserialize)]
pub struct GraphRelateBody {
    pub from: String,
    pub rel: String,
    pub to: String,
    #[serde(default)]
    pub valid_from: Option<String>,
    #[serde(default)]
    pub valid_to: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct GraphRelatedParams {
    pub id: String,
    #[serde(default)]
    pub rel: Option<String>,
    #[serde(default)]
    pub as_of: Option<String>,
    #[serde(default)]
    pub include_invalidated: bool,
}

#[derive(Deserialize)]
pub struct GraphCreateEntityBody {
    pub id: String,
    #[serde(rename = "type")]
    pub entity_type: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub properties: serde_json::Value,
}

#[derive(Deserialize)]
pub struct GraphInvalidateBody {
    pub from: String,
    pub rel: String,
    pub to: String,
    #[serde(default)]
    pub ended: Option<String>,
}

#[derive(Deserialize)]
pub struct GraphTimelineParams {
    pub entity: String,
    #[serde(default)]
    pub include_invalidated: bool,
}

#[derive(Deserialize)]
pub struct GraphTraverseParams {
    pub from: String,
    #[serde(default)]
    pub hops: Option<usize>,
    #[serde(default)]
    pub max_nodes: Option<usize>,
    #[serde(default)]
    pub as_of: Option<String>,
    #[serde(default)]
    pub include_invalidated: bool,
}

#[derive(Deserialize)]
pub struct GraphPathParams {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub max_hops: Option<usize>,
    #[serde(default)]
    pub include_invalidated: bool,
}

#[derive(Deserialize)]
pub struct GraphAliasBody {
    pub alias: String,
    pub canonical: String,
}

pub(crate) fn validate_canonical(id: &str) -> Result<(), String> {
    if id.trim().is_empty() {
        return Err("canonical must not be empty".into());
    }
    if id.chars().any(|c| c.is_whitespace()) {
        return Err("canonical must not contain whitespace".into());
    }
    if !id.contains('_') {
        return Err("canonical must contain '_' (entity ids look like type_slug)".into());
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct GraphTunnelsParams {
    #[serde(default)]
    pub include_invalidated: bool,
}

use crate::mem::graph_repair::{EntitySimilarityStrategy, RelationColumn};

fn default_repair_limit() -> usize {
    50
}

fn default_actor() -> String {
    "user".to_string()
}

#[derive(Deserialize)]
pub struct RepairFindRelationsBody {
    pub group_by: Vec<RelationColumn>,
    #[serde(default = "default_repair_limit")]
    pub limit: usize,
}

#[derive(Deserialize)]
pub struct RepairDeleteRelationsBody {
    pub rowids: Vec<i64>,
    #[serde(default = "default_actor")]
    pub actor: String,
}

#[derive(Deserialize)]
pub struct RepairMergeRelationsBody {
    pub keep: i64,
    pub merge: Vec<i64>,
    #[serde(default = "default_actor")]
    pub actor: String,
}

#[derive(Deserialize)]
pub struct RepairFindEntitiesBody {
    pub strategy: EntitySimilarityStrategy,
    #[serde(default = "default_repair_limit")]
    pub limit: usize,
}

#[derive(Deserialize)]
pub struct RepairAugmentEntityBody {
    pub id: String,
    pub patch: serde_json::Map<String, serde_json::Value>,
    #[serde(default = "default_actor")]
    pub actor: String,
}

#[derive(Deserialize)]
pub struct RepairDeleteEntityBody {
    pub id: String,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub purge_history: bool,
    #[serde(default = "default_actor")]
    pub actor: String,
}

#[derive(Deserialize)]
pub struct RepairOrphanVectorsBody {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default = "default_actor")]
    pub actor: String,
}

#[derive(Deserialize)]
pub struct RepairOrphanBlobsBody {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default = "default_actor")]
    pub actor: String,
}

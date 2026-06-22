use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mem::graph::Relation;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationColumn {
    FromId,
    RelationType,
    ToId,
    Scope,
    ValidFrom,
}

impl RelationColumn {
    pub(crate) fn sql(self) -> &'static str {
        match self {
            Self::FromId => "from_id",
            Self::RelationType => "relation_type",
            Self::ToId => "to_id",
            Self::Scope => "scope",
            Self::ValidFrom => "valid_from",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationMember {
    pub rowid: i64,
    pub relation_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateRelationGroup {
    pub key: Value,
    pub rowids: Vec<i64>,
    /// Per-rowid predicate, so a caller grouping by node-pair can tell which
    /// rowid carries which relation_type (collapse synonyms, keep distinct ones).
    pub members: Vec<RelationMember>,
    pub count: usize,
    pub sample: Relation,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeReport {
    pub merged_into: i64,
    pub deleted_rowids: Vec<i64>,
    pub final_confidence: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EntitySimilarityStrategy {
    /// Same entity_type + same `properties_json.name` (case-insensitive trim).
    ExactName,
    /// Entities that resolve to the same canonical via existing aliases.
    AliasChain,
    /// Vector-similarity pairs: same entity_type, cosine_sim >= threshold.
    /// Only surfaces entities whose embedded text is close — Dreamer
    /// confirms before merging via `mem graph alias`.
    Vector {
        /// Cosine similarity cutoff (0.0-1.0). Audit recommends 0.85+ as
        /// review-worthy, 0.90+ as strong-merge candidate.
        threshold: f64,
        /// Optional filter: only compare entities of this type to each other.
        #[serde(default)]
        entity_type: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateEntityGroup {
    pub canonical_hint: String,
    pub member_ids: Vec<String>,
    pub entity_type: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteEntityReport {
    pub entity_id: String,
    pub cascaded_relations: u64,
    pub cascaded_history: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairOrphanVectorsReport {
    pub orphan_ids: Vec<String>,
    pub deleted: u64,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairOrphanBlobsReport {
    pub orphan_files: Vec<String>,
    pub deleted: u64,
    pub dry_run: bool,
}

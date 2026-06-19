//! Repair primitives for the graph: find/delete/merge duplicates,
//! augment entity properties. All destructive ops audit-log to
//! `graph_purge_log`. No raw SQL passes through this surface —
//! callers (Dreamer, CLI, REST) pick from typed enums only.

mod edge_cleanup;
mod entities;
mod orphans;
mod relations;
mod types;

pub use edge_cleanup::{collapse_synonym_edges, prune_dream_hub_edges};
pub use entities::{augment_entity, delete_entity_by_id, find_duplicate_entities};
pub use orphans::{prune_thin_vectors, repair_orphan_blobs, repair_orphan_vectors};
pub use relations::{
    delete_relations_by_rowids, find_duplicate_relations, load_relation_by_rowid, merge_relations,
};
pub use types::{
    DeleteEntityReport, DuplicateEntityGroup, DuplicateRelationGroup, EntitySimilarityStrategy,
    MergeReport, RelationColumn, RelationMember, RepairOrphanBlobsReport,
    RepairOrphanVectorsReport,
};

#[cfg(test)]
mod tests;

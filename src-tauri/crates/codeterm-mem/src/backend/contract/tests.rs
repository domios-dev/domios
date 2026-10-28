use super::*;
use crate::backend::SqliteMemStore;

fn fixture() -> (tempfile::TempDir, SqliteMemStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteMemStore::open_in_memory(dir.path()).unwrap();
    (dir, store)
}

#[test]
fn sqlite_save_replace_history() {
    let (_dir, store) = fixture();
    save_replace_history(&store);
}
#[test]
fn sqlite_fts_hit() {
    let (_dir, store) = fixture();
    fts_hit(&store);
}
#[test]
fn sqlite_vector_stamp_refusal() {
    let (_dir, store) = fixture();
    vector_stamp_refusal(&store);
}
#[test]
fn sqlite_graph_relate_unrelate() {
    let (_dir, store) = fixture();
    graph_relate_unrelate(&store);
}
#[test]
fn sqlite_area_merge_attribution() {
    let (_dir, store) = fixture();
    area_merge_attribution(&store);
}
#[test]
fn sqlite_archive() {
    let (_dir, store) = fixture();
    archive(&store);
}
#[test]
fn sqlite_guarded_replace_is_atomic() {
    let (_dir, store) = fixture();
    guarded_replace_is_atomic(&store);
}
#[test]
fn sqlite_export_import_round_trip() {
    let (_source_dir, source) = fixture();
    let (_target_dir, target) = fixture();
    export_import_round_trip(&source, &target);
}

#[test]
fn sqlite_blueprint_qualified_save_merge_archive_reuse() {
    let (_dir, store) = fixture();
    blueprint::qualified_save_merge_archive_reuse(&store);
}
#[test]
fn sqlite_blueprint_scoped_save_eviction_and_rollback() {
    let (_dir, store) = fixture();
    blueprint::scoped_save_eviction_and_rollback(&store);
}
#[test]
fn sqlite_blueprint_call_scope_hash_and_immutable_revisions() {
    let (_dir, store) = fixture();
    blueprint::call_scope_hash_and_immutable_revisions(&store);
}
#[test]
fn sqlite_blueprint_dream_merge_and_archived_keeper() {
    let (_dir, store) = fixture();
    blueprint::dream_merge_and_archived_keeper(&store);
}
#[test]
fn sqlite_blueprint_enrichment_index_vectors_with_stamp() {
    let (_dir, store) = fixture();
    blueprint::enrichment_index_vectors_with_stamp(&store);
}
#[test]
fn sqlite_blueprint_curator_skill_metadata_merge() {
    let (_dir, store) = fixture();
    blueprint::curator_skill_metadata_merge(&store);
}
#[test]
fn sqlite_blueprint_export_import_round_trip() {
    let (_source_dir, source) = fixture();
    let (_target_dir, target) = fixture();
    blueprint::blueprint_export_import_round_trip(&source, &target);
}

#[test]
fn sqlite_blueprint_source_filtered_solution_candidates() {
    let (_dir, store) = fixture();
    blueprint::source_filtered_solution_candidates(&store);
}

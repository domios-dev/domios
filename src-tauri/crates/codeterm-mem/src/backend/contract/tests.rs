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

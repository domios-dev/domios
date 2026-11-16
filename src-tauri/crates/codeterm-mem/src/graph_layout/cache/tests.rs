use super::*;

fn tmp_path() -> PathBuf {
    std::env::temp_dir().join(format!("graph_layout_test_{}.json", ulid::Ulid::new()))
}

#[test]
fn round_trip_replace_and_reload() {
    let path = tmp_path();
    let cache = LayoutCache::load(path.clone());
    let mut map = HashMap::new();
    map.insert(
        "a".into(),
        Position {
            x: 1.0,
            y: 2.0,
            cluster_id: 3,
        },
    );
    let v = cache.replace_all(map, "now".into());
    assert_eq!(v, 1);
    assert!(cache.flush_now());

    let reloaded = LayoutCache::load(path.clone());
    assert_eq!(reloaded.layout_version(), 1);
    assert_eq!(
        reloaded.get("a"),
        Some(Position {
            x: 1.0,
            y: 2.0,
            cluster_id: 3
        })
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn corrupt_file_loads_empty_not_panic() {
    let path = tmp_path();
    std::fs::write(&path, b"{ this is : not json").unwrap();
    let cache = LayoutCache::load(path.clone());
    assert!(cache.is_empty());
    assert_eq!(cache.layout_version(), 0);
    std::fs::remove_file(&path).ok();
}

#[test]
fn unknown_format_version_rejected() {
    let path = tmp_path();
    let bogus = serde_json::json!({
        "format_version": 999,
        "layout_version": 7,
        "computed_at": "x",
        "positions": {}
    });
    std::fs::write(&path, serde_json::to_vec(&bogus).unwrap()).unwrap();
    let cache = LayoutCache::load(path.clone());
    assert!(cache.is_empty());
    assert_eq!(cache.layout_version(), 0);
    std::fs::remove_file(&path).ok();
}

#[test]
fn atomic_write_leaves_no_torn_file_when_tmp_garbage_present() {
    let path = tmp_path();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, b"garbage partial write").unwrap();
    let cache = LayoutCache::load(path.clone());
    let mut map = HashMap::new();
    map.insert(
        "x".into(),
        Position {
            x: 5.0,
            y: 6.0,
            cluster_id: 0,
        },
    );
    cache.replace_all(map, "now".into());
    assert!(cache.flush_now());
    let reloaded = LayoutCache::load(path.clone());
    assert_eq!(reloaded.get("x").map(|p| p.x), Some(5.0));
    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&tmp).ok();
}

#[test]
fn upsert_bumps_seq_and_positions_since_filters() {
    let path = tmp_path();
    let cache = LayoutCache::load(path.clone());
    cache.upsert(
        "a".into(),
        Position {
            x: 0.0,
            y: 0.0,
            cluster_id: 0,
        },
    );
    let seq_after_a = cache.current_seq();
    cache.upsert(
        "b".into(),
        Position {
            x: 1.0,
            y: 1.0,
            cluster_id: 0,
        },
    );

    let keep: std::collections::HashSet<String> =
        ["a".to_string(), "b".to_string()].into_iter().collect();
    let delta = cache.positions_since(seq_after_a, &keep);
    assert_eq!(delta.len(), 1);
    assert_eq!(delta[0].0, "b");
    std::fs::remove_file(&path).ok();
}

#[test]
fn positions_filtered_drops_stale_ids() {
    let path = tmp_path();
    let cache = LayoutCache::load(path.clone());
    cache.upsert(
        "live".into(),
        Position {
            x: 0.0,
            y: 0.0,
            cluster_id: 0,
        },
    );
    cache.upsert(
        "dead".into(),
        Position {
            x: 1.0,
            y: 1.0,
            cluster_id: 0,
        },
    );
    let keep: std::collections::HashSet<String> = ["live".to_string()].into_iter().collect();
    let out = cache.positions_filtered(&keep);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "live");
    std::fs::remove_file(&path).ok();
}

#[test]
fn previous_layout_is_discarded_for_full_projection() {
    let path = tmp_path();
    let old = serde_json::json!({
        "format_version": 1,
        "layout_version": 7,
        "computed_at": "old",
        "positions": { "a": [1000.0, 1000.0, 0.0] }
    });
    std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    let cache = LayoutCache::load(path.clone());
    assert!(cache.is_empty());
    assert_eq!(cache.layout_version(), 0);
    std::fs::remove_file(path).ok();
}

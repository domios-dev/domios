use super::*;
use rusqlite::Connection;

fn schema(conn: &Connection) {
    crate::store::init_schema(conn).unwrap();
}

#[test]
fn adjacency_is_undirected_and_active_only() {
    let conn = Connection::open_in_memory().unwrap();
    schema(&conn);
    conn.execute(
            "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, created_at) VALUES ('a','rel','b','global','t','t')",
            [],
        ).unwrap();
    conn.execute(
            "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, created_at) VALUES ('a','rel','c','global','t','ended','t')",
            [],
        ).unwrap();
    let adj = load_active_adjacency(&conn).unwrap();
    assert_eq!(adj.get("a").unwrap(), &vec!["b".to_string()]);
    assert_eq!(adj.get("b").unwrap(), &vec!["a".to_string()]);
    assert!(adj.get("c").is_none(), "invalidated edge must be excluded");
}

#[cfg(feature = "embeddings")]
#[test]
fn extracted_projection_preserves_the_single_vector_origin() {
    let positions = project_positions(
        &[("one".into(), vec![1.0, 0.0, 0.0])],
        &HashMap::new(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(positions.len(), 1);
    let position = positions["one"];
    assert_eq!((position.x, position.y), (0.0, 0.0));
    assert_eq!(position.cluster_id, 0);
}

#[cfg(not(feature = "embeddings"))]
#[test]
fn extracted_projection_preserves_the_feature_capability() {
    assert_eq!(
        project_positions(&[], &HashMap::new(), Vec::new()).unwrap_err(),
        "layout projection unavailable on this build (needs the embeddings feature)"
    );
}

#[cfg(all(feature = "embeddings", not(target_os = "macos")))]
#[test]
fn non_macos_project_2d_pca_seed_and_knn_are_populated() {
    let mut vectors = Vec::new();
    for i in 0..12 {
        vectors.push(vec![1.0 + i as f32 * 0.01, 0.1, 0.0, 0.0]);
        vectors.push(vec![-1.0 - i as f32 * 0.01, -0.1, 0.0, 0.0]);
    }

    let (points, knn) = project_2d(&vectors).unwrap();
    assert_eq!(points.len(), vectors.len());
    assert_eq!(knn.len(), vectors.len());
    assert!(knn.iter().any(|nbs| !nbs.is_empty()));
    assert!(points.iter().all(|(x, y)| x.is_finite() && y.is_finite()));

    let distinct = points
        .iter()
        .any(|p| (p.0 - points[0].0).abs() > 1e-4 || (p.1 - points[0].1).abs() > 1e-4);
    assert!(distinct, "PCA seed must produce distinct coordinates");
}

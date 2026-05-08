//! Track 4a tests — plan_format round-trip + apply pipeline.
//!
//! The heuristic-phase tests (`find_hubs`, `propose_for_hub`) were stripped
//! along with the heuristic itself before v1.1.23 ship — see the cluster.rs
//! module doc.

use rusqlite::Connection;

use super::init_v1_22_schema;

#[test]
fn plan_format_round_trip() {
    let plan = crate::plan_format::ClusterPlan {
        generated_at: "2026-05-08T10:30:00Z".into(),
        clusters: vec![crate::plan_format::ClusterAction {
            topic_id: "topic_test".into(),
            entity_type: "Topic".into(),
            anchor: "anchor_a".into(),
            rel_source: "about".into(),
            rel_anchor: "about".into(),
            reason: Some("heuristic-recluster-2026-05-08".into()),
            rationale: "Test cluster rationale.".into(),
            sources: vec!["s1".into(), "s2".into(), "s3".into()],
        }],
    };
    let md = crate::plan_format::write(&plan);
    assert!(md.contains("```yaml"));
    assert!(md.contains("topic_id: topic_test"));
    assert!(md.contains("- s1"));
    let parsed = crate::plan_format::read(&md).unwrap();
    assert_eq!(parsed.warnings.len(), 0, "clean plan must produce zero warnings");
    assert_eq!(parsed.plan.clusters.len(), 1);
    assert_eq!(parsed.plan.clusters[0].topic_id, "topic_test");
    assert_eq!(parsed.plan.clusters[0].entity_type, "Topic");
    assert_eq!(parsed.plan.clusters[0].anchor, "anchor_a");
    assert_eq!(parsed.plan.clusters[0].sources, vec!["s1", "s2", "s3"]);
    assert_eq!(parsed.plan.clusters[0].reason.as_deref(), Some("heuristic-recluster-2026-05-08"));
    assert_eq!(parsed.plan.generated_at, "2026-05-08T10:30:00Z");
}

#[test]
fn plan_format_handles_malformed_fence_gracefully() {
    let md = "# Re-cluster proposal — generated 2026-05-08T10:30:00Z\n\
              \n\
              ### Cluster: topic_good\n\
              Rationale: keep this one.\n\
              \n\
              ```yaml\n\
              action: introduce_topic\n\
              topic_id: topic_good\n\
              entity_type: Topic\n\
              anchor: anchor_a\n\
              rel_source: about\n\
              rel_anchor: about\n\
              sources:\n\
                - s1\n\
                - s2\n\
              ```\n\
              \n\
              ### Cluster: topic_bad\n\
              Rationale: this one's broken.\n\
              \n\
              ```yaml\n\
              this is not even close to valid yaml\n\
              !!! \n\
              ```\n\
              \n\
              ### Cluster: topic_other_good\n\
              Rationale: still parsed despite the bad neighbour.\n\
              \n\
              ```yaml\n\
              action: introduce_topic\n\
              topic_id: topic_other_good\n\
              entity_type: Topic\n\
              anchor: anchor_b\n\
              rel_source: about\n\
              rel_anchor: about\n\
              sources:\n\
                - s3\n\
              ```\n";
    let report = crate::plan_format::read(md).unwrap();
    assert_eq!(report.plan.clusters.len(), 2, "good clusters must survive a bad neighbour");
    assert!(!report.warnings.is_empty(), "bad fence must produce a warning");
    let topic_ids: Vec<&str> = report.plan.clusters.iter()
        .map(|c| c.topic_id.as_str()).collect();
    assert!(topic_ids.contains(&"topic_good"));
    assert!(topic_ids.contains(&"topic_other_good"));
}

#[test]
fn apply_runs_each_cluster_action() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('anchor','Hub','global','{}','2026-01-01','2026-01-01'),
                   ('s1','Source','global','{}','2026-01-01','2026-01-01'),
                   ('s2','Source','global','{}','2026-01-01','2026-01-01');
         INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
            VALUES ('s1','about','anchor','global','2026-01-01',NULL,1.0,'{}','2026-01-01'),
                   ('s2','about','anchor','global','2026-01-01',NULL,1.0,'{}','2026-01-01');"
    ).unwrap();

    let plan = crate::plan_format::ClusterPlan {
        generated_at: "2026-05-08T10:30:00Z".into(),
        clusters: vec![crate::plan_format::ClusterAction {
            topic_id: "topic_apply_test".into(),
            entity_type: "Topic".into(),
            anchor: "anchor".into(),
            rel_source: "about".into(),
            rel_anchor: "about".into(),
            reason: Some("apply-test".into()),
            rationale: "Test apply path".into(),
            sources: vec!["s1".into(), "s2".into()],
        }],
    };
    let report = crate::cluster::apply(&conn, &plan).unwrap();
    assert_eq!(report.clusters_applied, 1);
    assert_eq!(report.clusters_failed, 0);

    let topic_exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entities WHERE id='topic_apply_test'", [], |r| r.get(0)).unwrap();
    assert_eq!(topic_exists, 1);

    let direct_active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations \
         WHERE from_id IN ('s1','s2') AND to_id='anchor' AND valid_to IS NULL",
        [], |r| r.get(0)).unwrap();
    assert_eq!(direct_active, 0, "direct source->anchor edges must be invalidated");

    let topic_anchor_active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations WHERE from_id='topic_apply_test' AND to_id='anchor' AND valid_to IS NULL",
        [], |r| r.get(0)).unwrap();
    assert_eq!(topic_anchor_active, 1, "topic -> anchor edge must exist");

    let sources_via_topic: i64 = conn.query_row(
        "SELECT COUNT(*) FROM relations \
         WHERE from_id IN ('s1','s2') AND to_id='topic_apply_test' AND valid_to IS NULL",
        [], |r| r.get(0)).unwrap();
    assert_eq!(sources_via_topic, 2, "both sources must route through topic");
}

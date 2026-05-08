//! Track 4a tests — cluster heuristic + plan_format round-trip + apply.

use rusqlite::Connection;

use super::init_v1_22_schema;

fn seed_hub_with_n_active_edges(conn: &Connection, hub: &str, n: usize) {
    conn.execute(
        "INSERT OR IGNORE INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES (?1,'Hub','global','{}','2026-01-01','2026-01-01')",
        rusqlite::params![hub],
    ).unwrap();
    for i in 0..n {
        let peer = format!("{}_peer_{:03}", hub, i);
        conn.execute(
            "INSERT OR IGNORE INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
                VALUES (?1,'Source','global','{}','2026-01-01','2026-01-01')",
            rusqlite::params![peer],
        ).unwrap();
        conn.execute(
            "INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
                VALUES (?1,'about',?2,'global','2026-01-01',NULL,1.0,'{}','2026-01-01')",
            rusqlite::params![peer, hub],
        ).unwrap();
    }
}

#[test]
fn find_hubs_returns_entities_above_threshold() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    seed_hub_with_n_active_edges(&conn, "hub_a", 30);
    seed_hub_with_n_active_edges(&conn, "hub_b", 5);

    let hubs = crate::cluster::find_hubs(&conn, 25).unwrap();
    let ids: Vec<&str> = hubs.iter().map(|h| h.id.as_str()).collect();
    assert!(ids.contains(&"hub_a"), "hub_a (30 edges) must clear threshold 25; got {:?}", ids);
    assert!(!ids.contains(&"hub_b"), "hub_b (5 edges) must NOT clear threshold 25; got {:?}", ids);
}

#[test]
fn propose_for_hub_groups_by_relation_type_dominance() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('hub_a','Hub','global','{}','2026-01-01','2026-01-01');"
    ).unwrap();
    // 18 'about' edges + 12 'related-to' edges. about is 60% (>= 40%), wins;
    // 'related-to' at exactly 40% does NOT (strict-greater rule).
    for i in 0..18 {
        let peer = format!("topic_release_v_{:03}", i);
        conn.execute(
            "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
                VALUES (?1,'Source','global','{}','2026-01-01','2026-01-01')",
            rusqlite::params![peer],
        ).unwrap();
        conn.execute(
            "INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
                VALUES (?1,'about','hub_a','global','2026-01-01',NULL,1.0,'{}','2026-01-01')",
            rusqlite::params![peer],
        ).unwrap();
    }
    for i in 0..12 {
        let peer = format!("misc_node_{:03}", i);
        conn.execute(
            "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
                VALUES (?1,'Source','global','{}','2026-01-01','2026-01-01')",
            rusqlite::params![peer],
        ).unwrap();
        conn.execute(
            "INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
                VALUES (?1,'related-to','hub_a','global','2026-01-01',NULL,1.0,'{}','2026-01-01')",
            rusqlite::params![peer],
        ).unwrap();
    }

    let hub = crate::cluster::Hub { id: "hub_a".into(), degree: 30 };
    let proposals = crate::cluster::propose_for_hub(&conn, &hub).unwrap();
    assert_eq!(proposals.len(), 1, "only 'about' (60%) should qualify; got {:?}", proposals);
    let p = &proposals[0];
    assert_eq!(p.dominant_rel, "about");
    assert_eq!(p.members.len(), 18);
    assert!((p.ratio - 0.6).abs() < 0.01, "expected ~0.6 ratio, got {}", p.ratio);
}

#[test]
fn propose_for_hub_prefers_existing_topic_anchors() {
    let conn = Connection::open_in_memory().unwrap();
    init_v1_22_schema(&conn);
    crate::migrations::run(&conn).unwrap();
    // Hand-off invariant: existing topic_*/role_* anchors are reused, not
    // regenerated. We seed an unrelated topic_codeterm_release_history
    // (whose tail doesn't match the cluster's prefix) and verify the
    // synthesizer wouldn't pick it. Then we seed a matching anchor and
    // verify it DOES get picked.
    conn.execute_batch(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('hub_proj','Hub','global','{}','2026-01-01','2026-01-01'),
                   ('topic_codeterm_release_history','Topic','global','{}','2026-01-01','2026-01-01');"
    ).unwrap();
    for i in 0..10 {
        let peer = format!("learning_v1_1_{:02}_release", i);
        conn.execute(
            "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
                VALUES (?1,'Source','global','{}','2026-01-01','2026-01-01')",
            rusqlite::params![peer],
        ).unwrap();
        conn.execute(
            "INSERT INTO relations (from_id,relation_type,to_id,scope,valid_from,valid_to,confidence,properties_json,created_at)
                VALUES (?1,'about','hub_proj','global','2026-01-01',NULL,1.0,'{}','2026-01-01')",
            rusqlite::params![peer],
        ).unwrap();
    }
    let hub = crate::cluster::Hub { id: "hub_proj".into(), degree: 10 };
    let proposals = crate::cluster::propose_for_hub(&conn, &hub).unwrap();
    assert_eq!(proposals.len(), 1);
    // Now seed a topic that DOES share the members' prefix.
    conn.execute(
        "INSERT INTO entities (id,entity_type,scope,properties_json,created_at,updated_at)
            VALUES ('topic_learning_v1_1_releases','Topic','global','{}','2026-01-01','2026-01-01')",
        [],
    ).unwrap();
    let proposals = crate::cluster::propose_for_hub(&conn, &hub).unwrap();
    assert_eq!(proposals.len(), 1);
    assert_eq!(
        proposals[0].topic_id, "topic_learning_v1_1_releases",
        "must reuse the existing topic_learning_v1_1_* anchor instead of inventing a new slug"
    );
}

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

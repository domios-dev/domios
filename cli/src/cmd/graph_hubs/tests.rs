use std::collections::BTreeSet;

use codeterm_mem_store::hubs::HubQuery;
use rusqlite::{params, Connection};
use serde_json::Value;

use super::hubs_json;

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE relations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_id TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            to_id TEXT NOT NULL,
            valid_to TEXT
        );
        CREATE TABLE entities (
            id TEXT PRIMARY KEY,
            entity_type TEXT NOT NULL,
            properties_json TEXT NOT NULL DEFAULT '{}'
        );
        CREATE TABLE mem_items (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            area TEXT
        );",
    )
    .unwrap();
    conn
}

fn entity(conn: &Connection, id: &str, entity_type: &str) {
    conn.execute(
        "INSERT INTO entities (id, entity_type) VALUES (?1, ?2)",
        params![id, entity_type],
    )
    .unwrap();
}

fn item(conn: &Connection, id: &str, kind: &str, title: &str, area: Option<&str>) {
    conn.execute(
        "INSERT INTO mem_items (id, kind, title, area) VALUES (?1, ?2, ?3, ?4)",
        params![id, kind, title, area],
    )
    .unwrap();
}

fn edge(conn: &Connection, from: &str, rel: &str, to: &str) {
    conn.execute(
        "INSERT INTO relations (from_id, relation_type, to_id) VALUES (?1, ?2, ?3)",
        params![from, rel, to],
    )
    .unwrap();
}

fn star(conn: &Connection, hub: &str, spokes: usize) {
    entity(conn, hub, "Project");
    for n in 0..spokes {
        let id = format!("{hub}_spoke_{n}");
        item(conn, &id, "fact", &format!("spoke {n}"), None);
        edge(conn, &id, "applies_to", hub);
    }
}

fn report(conn: &Connection, threshold: u32, limit: u32, sample: u32) -> Value {
    hubs_json(
        conn,
        HubQuery {
            threshold,
            limit,
            sample,
        },
    )
    .unwrap()
}

fn keys(value: &Value) -> BTreeSet<String> {
    value.as_object().unwrap().keys().cloned().collect()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| item.to_string()).collect()
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn cluster<'a>(hub: &'a Value, basis: &str, key: &str) -> &'a Value {
    hub["clusters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["basis"] == basis && c["key"] == key)
        .unwrap_or_else(|| panic!("no {basis}:{key} cluster in {}", hub["clusters"]))
}

/// A hub whose inbound neighbours split by area, keyword and kind, plus one
/// neighbour of every kind the clustering must not touch.
fn clustered_hub(conn: &Connection) {
    entity(conn, "project_alpha", "Project");
    entity(conn, "topic_auth", "Topic");
    entity(conn, "topic_alpha_old", "Topic");
    entity(conn, "tech_rust", "Tech");
    let inbound = [
        ("fact_login", "fact", "Login flow", Some("auth")),
        ("fact_tokens", "fact", "Token refresh", Some("auth")),
        ("fact_ledger_sync", "fact", "Ledger sync", Some("alpha")),
        ("fact_ledger_export", "fact", "Ledger export", Some("alpha")),
        ("fact_notes", "fact", "Release notes", None),
        ("fact_pipeline", "fact", "Release pipeline", None),
        ("skill_gizmo", "skill", "Gizmo", None),
        ("skill_widget", "skill", "Widget", None),
        ("fact_loner", "fact", "Lonely", None),
    ];
    for (id, kind, title, area) in inbound {
        item(conn, id, kind, title, area);
        edge(conn, id, "applies_to", "project_alpha");
    }
    edge(conn, "topic_alpha_old", "about", "project_alpha");
    edge(conn, "project_alpha", "uses", "tech_rust");
}

#[test]
fn graph_hubs_limit_caps_by_degree() {
    let conn = fixture();
    star(&conn, "project_big", 6);
    star(&conn, "project_mid", 5);
    star(&conn, "project_small", 4);
    star(&conn, "project_dreams", 9);

    let value = report(&conn, 4, 2, 0);

    let ids: Vec<&str> = value["hubs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["project_big", "project_mid"]);
    assert_eq!(value["total_hubs"], 3);
    assert_eq!(value["limit"], 2);
    assert_eq!(value["excluded"][0]["id"], "project_dreams");
    assert_eq!(value["excluded"][0]["degree"], 9);
    assert_eq!(report(&conn, 4, 0, 0)["hubs"].as_array().unwrap().len(), 0);
}

#[test]
fn graph_hubs_clusters_group_by_area_kind_keyword() {
    let conn = fixture();
    clustered_hub(&conn);

    let value = report(&conn, 5, 5, 0);
    let hub = &value["hubs"][0];
    assert_eq!(hub["id"], "project_alpha");

    let area = cluster(hub, "area", "auth");
    assert_eq!(strings(&area["members"]), ["fact_login", "fact_tokens"]);
    assert_eq!(area["suggested_topic"], "topic_auth");
    assert_eq!(area["existing"], true);

    let ledger = cluster(hub, "keyword", "ledger");
    assert_eq!(
        strings(&ledger["members"]),
        ["fact_ledger_export", "fact_ledger_sync"]
    );
    let release = cluster(hub, "keyword", "release");
    assert_eq!(
        strings(&release["members"]),
        ["fact_notes", "fact_pipeline"]
    );
    assert_eq!(release["suggested_topic"], "topic_alpha_release");
    assert_eq!(release["entity_type"], "Topic");
    assert_eq!(release["existing"], false);

    let kind = cluster(hub, "kind", "skill");
    assert_eq!(strings(&kind["members"]), ["skill_gizmo", "skill_widget"]);
    assert_eq!(kind["suggested_topic"], "topic_alpha_skill");

    let clusters = hub["clusters"].as_array().unwrap();
    assert!(
        clusters.iter().all(|c| c["key"] != "alpha"),
        "an area naming the hub itself splits nothing: {clusters:?}"
    );
    let mut claimed = BTreeSet::new();
    for c in clusters {
        for member in strings(&c["members"]) {
            assert!(claimed.insert(member.clone()), "{member} in two clusters");
        }
    }
    assert!(!claimed.contains("topic_alpha_old"));
    assert!(!claimed.contains("tech_rust"));
    assert_eq!(hub["sub_anchors"], 1);
    assert_eq!(hub["outbound_only"], 1);
    assert_eq!(hub["unclustered"], 1);
    assert_eq!(hub["popular_source"], false);
}

#[test]
fn graph_hubs_json_shape_is_stable() {
    let conn = fixture();
    clustered_hub(&conn);
    item(
        &conn,
        "learning_kid_header",
        "learning",
        "JWT kid header",
        None,
    );
    for n in 0..5 {
        let conv = format!("conv_{n}");
        item(&conn, &conv, "conversation", &format!("Session {n}"), None);
        edge(&conn, &conv, "extracted_from", "learning_kid_header");
    }
    star(&conn, "project_dreams", 6);

    let value = report(&conn, 5, 5, 2);

    let top = set(&[
        "threshold",
        "limit",
        "sample",
        "total_hubs",
        "hubs",
        "excluded",
        "existing_anchors",
    ]);
    assert_eq!(keys(&value), top);
    let hub_keys = set(&[
        "id",
        "degree",
        "rel_dist",
        "popular_source",
        "sub_anchors",
        "outbound_only",
        "unclustered",
        "clusters",
        "neighbors",
    ]);
    let cluster_keys = set(&[
        "suggested_topic",
        "entity_type",
        "existing",
        "basis",
        "key",
        "members",
    ]);
    for hub in value["hubs"].as_array().unwrap() {
        assert_eq!(keys(hub), hub_keys);
        assert!(hub["neighbors"].as_array().unwrap().len() <= 2);
        for neighbor in hub["neighbors"].as_array().unwrap() {
            assert_eq!(keys(neighbor), set(&["id", "rel", "dir", "title"]));
        }
        for rel in hub["rel_dist"].as_array().unwrap() {
            assert_eq!(keys(rel), set(&["rel", "count"]));
        }
        for c in hub["clusters"].as_array().unwrap() {
            assert_eq!(keys(c), cluster_keys);
        }
    }
    let source = value["hubs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == "learning_kid_header")
        .unwrap();
    assert_eq!(source["popular_source"], true);
    assert_eq!(source["clusters"].as_array().unwrap().len(), 0);
    assert_eq!(
        keys(&value["excluded"][0]),
        set(&["id", "degree", "reason"])
    );
    assert_eq!(
        keys(&value["existing_anchors"][0]),
        set(&["id", "entity_type"])
    );
}

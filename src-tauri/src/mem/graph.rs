//! Knowledge graph: typed entities + temporal relations.
//! Relations carry `valid_from`/`valid_to` so as-of queries can filter stale facts.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub entity_type: String,
    pub scope: String,
    pub properties: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relation {
    pub from_id: String,
    pub relation_type: String,
    pub to_id: String,
    pub scope: String,
    pub valid_from: String,
    pub valid_to: Option<String>,
    pub confidence: f64,
    pub properties: Value,
}

pub fn insert_entity(conn: &Connection, e: &Entity) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let props = serde_json::to_string(&e.properties).unwrap_or_else(|_| "{}".into());
    conn.execute(
        r#"INSERT OR REPLACE INTO entities
           (id, entity_type, scope, properties_json, created_at, updated_at)
           VALUES (?1, ?2, ?3, ?4,
                   COALESCE((SELECT created_at FROM entities WHERE id = ?1), ?5),
                   ?5)"#,
        params![&e.id, &e.entity_type, &e.scope, &props, &now],
    )
    .map_err(|e| format!("insert entity: {}", e))?;
    Ok(())
}

pub fn insert_relation(conn: &Connection, r: &Relation) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let props = serde_json::to_string(&r.properties).unwrap_or_else(|_| "{}".into());
    conn.execute(
        r#"INSERT OR IGNORE INTO relations
           (from_id, relation_type, to_id, scope, valid_from, valid_to,
            confidence, properties_json, created_at)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
        params![
            &r.from_id, &r.relation_type, &r.to_id, &r.scope,
            &r.valid_from, &r.valid_to, r.confidence, &props, &now,
        ],
    )
    .map_err(|e| format!("insert relation: {}", e))?;
    Ok(())
}

/// Outgoing relations from `id`. Optional type filter and as-of date.
pub fn related(
    conn: &Connection,
    id: &str,
    rel_type: Option<&str>,
    as_of: Option<&str>,
) -> Result<Vec<Relation>, String> {
    let mut sql = String::from(
        r#"SELECT from_id, relation_type, to_id, scope, valid_from, valid_to,
                  confidence, properties_json
           FROM relations WHERE from_id = ?1"#,
    );
    let mut binds: Vec<String> = vec![id.to_string()];
    if let Some(t) = rel_type {
        sql.push_str(" AND relation_type = ?2");
        binds.push(t.to_string());
    }
    if let Some(date) = as_of {
        let n = binds.len() + 1;
        sql.push_str(&format!(
            " AND valid_from <= ?{0} AND (valid_to IS NULL OR valid_to > ?{0})",
            n
        ));
        binds.push(date.to_string());
    }

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let bind_refs: Vec<&dyn rusqlite::ToSql> =
        binds.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(bind_refs), |r| {
            let props_json: String = r.get(7)?;
            Ok(Relation {
                from_id: r.get(0)?,
                relation_type: r.get(1)?,
                to_id: r.get(2)?,
                scope: r.get(3)?,
                valid_from: r.get(4)?,
                valid_to: r.get(5)?,
                confidence: r.get(6)?,
                properties: serde_json::from_str(&props_json).unwrap_or(Value::Null),
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::store::init_schema;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn entity_roundtrip() {
        let conn = fresh();
        let e = Entity {
            id: "person_kai".into(),
            entity_type: "Person".into(),
            scope: "global".into(),
            properties: serde_json::json!({"name": "Kai"}),
        };
        insert_entity(&conn, &e).unwrap();
        // Idempotent on second insert
        insert_entity(&conn, &e).unwrap();
    }

    #[test]
    fn relation_with_as_of_filter() {
        let conn = fresh();
        insert_relation(
            &conn,
            &Relation {
                from_id: "person_kai".into(),
                relation_type: "works_on".into(),
                to_id: "project_a".into(),
                scope: "global".into(),
                valid_from: "2026-01-01T00:00:00Z".into(),
                valid_to: Some("2026-03-01T00:00:00Z".into()),
                confidence: 1.0,
                properties: serde_json::json!({}),
            },
        )
        .unwrap();
        insert_relation(
            &conn,
            &Relation {
                from_id: "person_kai".into(),
                relation_type: "works_on".into(),
                to_id: "project_b".into(),
                scope: "global".into(),
                valid_from: "2026-03-01T00:00:00Z".into(),
                valid_to: None,
                confidence: 1.0,
                properties: serde_json::json!({}),
            },
        )
        .unwrap();

        // As of 2026-02-01: should see project_a only
        let r = related(&conn, "person_kai", None, Some("2026-02-01T00:00:00Z")).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].to_id, "project_a");

        // As of 2026-04-01: should see project_b only
        let r = related(&conn, "person_kai", None, Some("2026-04-01T00:00:00Z")).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].to_id, "project_b");

        // No date filter: should see both
        let r = related(&conn, "person_kai", None, None).unwrap();
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn unique_constraint_dedupes() {
        let conn = fresh();
        let r = Relation {
            from_id: "a".into(),
            relation_type: "rel".into(),
            to_id: "b".into(),
            scope: "global".into(),
            valid_from: "2026-01-01T00:00:00Z".into(),
            valid_to: None,
            confidence: 1.0,
            properties: serde_json::json!({}),
        };
        insert_relation(&conn, &r).unwrap();
        insert_relation(&conn, &r).unwrap();
        let all = related(&conn, "a", None, None).unwrap();
        assert_eq!(all.len(), 1);
    }
}

# Dreamer Self-Repair API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the Dreamer a typed, whitelist-bounded API to find duplicates in the knowledge graph, delete or merge them with information carryover, and augment node properties — without granting raw SQL access.

**Architecture:** Three verbs on the graph layer (`find_*`, `delete_*`, `merge_*`) plus `augment_entity` for enrichment. All destructive ops audit-log to the existing `graph_purge_log` table with full payload for reversibility. CLI and REST surfaces expose the same primitives. The REM Dreamer prompt is extended with a self-repair playbook that uses these via CLI calls during its nightly cycle.

**Tech Stack:** Rust (rusqlite, serde, serde_json), clap for CLI, the existing axum REST layer (`src-tauri/src/api/`), Markdown prompts under `src-tauri/prompts/`.

**Existing infrastructure being reused:**
- `graph_purge_log` table — already created by `crates/mem-store/src/migrations.rs:79`. Has `purged_at, count, scope, before, reason_filter, actor, details_json`. Audit helper `crates/mem-store/src/audit.rs:record_purge`.
- `relations.UNIQUE(from_id, relation_type, to_id, valid_from)` — already in `src-tauri/src/mem/store/schema.rs:66`. New writes can't add exact duplicates; we're cleaning historical pollution.
- `entity_aliases` table + `add_alias()` in `src-tauri/src/mem/graph.rs:396` — already does entity merge + relation rewire. Missing: property carryover from alias to canonical (Task 4 adds this).
- `MemManager` (`src-tauri/src/mem/manager_graph.rs`) — facade for graph ops, where new manager methods will go.

**File structure:**
- Create: `src-tauri/src/mem/graph_repair.rs` (~400 LOC) — keeps `graph.rs` from crossing 1000-line limit.
- Modify: `src-tauri/src/mem/graph.rs` — extend `add_alias` with property carryover only.
- Modify: `src-tauri/src/mem/mod.rs` — register new module.
- Modify: `src-tauri/src/mem/manager_graph.rs` — expose repair ops as manager methods.
- Modify: `cli/src/args/mem.rs` — add `Graph::Repair` subcommand tree.
- Modify: `cli/src/commands/mem/graph.rs` (or wherever graph CLI commands live) — dispatch handlers.
- Modify: `src-tauri/src/api/mem.rs` (or wherever mem REST is) — POST repair endpoints.
- Modify: `src-tauri/prompts/dreaming_rem.md` — add Self-Repair Playbook section.
- Test: `src-tauri/src/mem/graph_repair/tests.rs` — unit tests per function.
- Test: `evals/dreamer-repair/` — new eval set (one drill: seed duplicates, run REM, assert cleanup).

**Non-goals (deliberately):**
- No raw SQL exposure. Dreamer never writes `WHERE` clauses.
- No mem_item content rewrites (Dreamer enriching item bodies is a separate, harder problem — out of scope).
- No automatic policy ("always dedup exact-key"). The CLI/REST are dumb primitives; Dreamer's prompt encodes the policy and can be tuned without code changes.
- No backwards-compat shim for the existing `add_alias` callers — current callers don't expect property carryover, and adding it is strict-better.

---

## Task 1: Add `find_duplicate_relations` primitive

**Files:**
- Create: `src-tauri/src/mem/graph_repair.rs`
- Create: `src-tauri/src/mem/graph_repair/tests.rs`
- Modify: `src-tauri/src/mem/mod.rs:` — add `pub mod graph_repair;`

- [ ] **Step 1: Define the column whitelist and group shape**

Add to `graph_repair.rs`:

```rust
//! Repair primitives for the graph: find/delete/merge duplicates,
//! augment entity properties. All destructive ops audit-log to
//! `graph_purge_log`. No raw SQL passes through this surface —
//! callers (Dreamer, CLI, REST) pick from typed enums only.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::graph::Relation;

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
    fn sql(self) -> &'static str {
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
pub struct DuplicateRelationGroup {
    pub key: Value,
    pub rowids: Vec<i64>,
    pub count: usize,
    pub sample: Relation,
}
```

- [ ] **Step 2: Write the failing test for `find_duplicate_relations`**

Create `src-tauri/src/mem/graph_repair/tests.rs`:

```rust
use rusqlite::Connection;
use crate::mem::graph::{insert_relation, Relation};
use crate::mem::store::schema::ensure_schema;
use super::*;

fn fresh_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    ensure_schema(&conn).unwrap();
    conn
}

fn rel(from: &str, rel: &str, to: &str, vf: &str) -> Relation {
    Relation {
        from_id: from.into(),
        relation_type: rel.into(),
        to_id: to.into(),
        scope: "global".into(),
        valid_from: vf.into(),
        valid_to: None,
        confidence: 1.0,
        properties: serde_json::json!({}),
    }
}

#[test]
fn find_duplicate_relations_groups_by_exact_key() {
    let conn = fresh_conn();
    // Same (from, rel, to, valid_from) — UNIQUE blocks new dupes, so we
    // need a direct INSERT path to simulate historical pollution.
    let now = "2026-01-01T00:00:00Z";
    conn.execute(
        "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at)
         VALUES ('a','knows','b','global',?1,NULL,1.0,'{}',?1),
                ('a','knows','b','global',?1,NULL,0.9,'{\"src\":\"x\"}',?1),
                ('a','knows','b','global',?1,NULL,0.8,'{}',?1)",
        rusqlite::params![now],
    ).unwrap();
    insert_relation(&conn, &rel("c","knows","d", now)).unwrap();

    let groups = find_duplicate_relations(
        &conn,
        &[RelationColumn::FromId, RelationColumn::RelationType, RelationColumn::ToId, RelationColumn::ValidFrom],
        100,
    ).unwrap();

    assert_eq!(groups.len(), 1, "only (a,knows,b) should appear as a duplicate group");
    let g = &groups[0];
    assert_eq!(g.count, 3);
    assert_eq!(g.rowids.len(), 3);
    assert_eq!(g.key["from_id"], "a");
}
```

- [ ] **Step 3: Run the test to confirm it fails**

Run: `./build.sh check 2>&1 | tail -20` then `cargo test --manifest-path src-tauri/Cargo.toml -p codeterm find_duplicate_relations_groups_by_exact_key -- --nocapture`
Expected: compile error `cannot find function find_duplicate_relations`.

- [ ] **Step 4: Implement `find_duplicate_relations`**

Append to `graph_repair.rs`:

```rust
pub fn find_duplicate_relations(
    conn: &Connection,
    group_by: &[RelationColumn],
    limit: usize,
) -> Result<Vec<DuplicateRelationGroup>, String> {
    if group_by.is_empty() {
        return Err("group_by must contain at least one column".into());
    }
    let cols = group_by.iter().map(|c| c.sql()).collect::<Vec<_>>().join(",");

    let sql = format!(
        "SELECT {cols}, GROUP_CONCAT(id) AS rowids, COUNT(*) AS c
         FROM relations
         WHERE valid_to IS NULL
         GROUP BY {cols}
         HAVING c > 1
         ORDER BY c DESC
         LIMIT ?1",
        cols = cols
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query(params![limit as i64]).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let mut key = Map::<String, Value>::new();
        for (i, col) in group_by.iter().enumerate() {
            let v: Option<String> = row.get(i).map_err(|e| e.to_string())?;
            key.insert(col.sql().into(), v.map(Value::String).unwrap_or(Value::Null));
        }
        let rowids_csv: String = row.get(group_by.len()).map_err(|e| e.to_string())?;
        let count: i64 = row.get(group_by.len() + 1).map_err(|e| e.to_string())?;
        let rowids: Vec<i64> = rowids_csv.split(',').filter_map(|s| s.parse().ok()).collect();
        let sample = load_relation_by_rowid(conn, rowids[0])?;
        out.push(DuplicateRelationGroup {
            key: Value::Object(key),
            rowids,
            count: count as usize,
            sample,
        });
    }
    Ok(out)
}

fn load_relation_by_rowid(conn: &Connection, rowid: i64) -> Result<Relation, String> {
    conn.query_row(
        "SELECT from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json
         FROM relations WHERE id = ?1",
        params![rowid],
        |r| {
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
        },
    ).map_err(|e| format!("load relation {}: {}", rowid, e))
}
```

- [ ] **Step 5: Wire the module into `mod.rs`**

Edit `src-tauri/src/mem/mod.rs` (search for `pub mod graph;`):

```rust
pub mod graph;
pub mod graph_repair;
```

- [ ] **Step 6: Add test module declaration at bottom of `graph_repair.rs`**

```rust
#[cfg(test)] mod tests;
```

- [ ] **Step 7: Run the test to confirm it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p codeterm find_duplicate_relations_groups_by_exact_key -- --nocapture`
Expected: PASS, one group with count=3.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/mem/graph_repair.rs src-tauri/src/mem/graph_repair/ src-tauri/src/mem/mod.rs
git commit -m "mem(graph): find_duplicate_relations primitive (read-only)"
```

---

## Task 2: Add `delete_relations_by_rowids` with audit log

**Files:**
- Modify: `src-tauri/src/mem/graph_repair.rs`
- Modify: `src-tauri/src/mem/graph_repair/tests.rs`

- [ ] **Step 1: Write the failing test**

Append to `tests.rs`:

```rust
#[test]
fn delete_relations_by_rowids_audits_payload() {
    let conn = fresh_conn();
    let now = "2026-01-01T00:00:00Z";
    conn.execute(
        "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at)
         VALUES ('a','knows','b','global',?1,NULL,1.0,'{}',?1),
                ('a','knows','b','global',?1,NULL,0.9,'{\"k\":1}',?1)",
        rusqlite::params![now],
    ).unwrap();

    let rowids: Vec<i64> = conn.prepare("SELECT id FROM relations").unwrap()
        .query_map([], |r| r.get(0)).unwrap()
        .collect::<Result<_, _>>().unwrap();
    assert_eq!(rowids.len(), 2);

    let deleted = delete_relations_by_rowids(&conn, &[rowids[1]], "test").unwrap();
    assert_eq!(deleted, 1);

    let remaining: i64 = conn.query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0)).unwrap();
    assert_eq!(remaining, 1);

    let (count, actor, details): (i64, String, String) = conn.query_row(
        "SELECT count, actor, details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    ).unwrap();
    assert_eq!(count, 1);
    assert_eq!(actor, "test");
    let parsed: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert!(parsed["payload"].is_array(), "audit must include payload for reversibility");
    assert_eq!(parsed["payload"][0]["properties_json"], "{\"k\":1}");
}
```

- [ ] **Step 2: Run to confirm it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p codeterm delete_relations_by_rowids_audits_payload`
Expected: compile error.

- [ ] **Step 3: Implement `delete_relations_by_rowids`**

Append to `graph_repair.rs`:

```rust
pub fn delete_relations_by_rowids(
    conn: &Connection,
    rowids: &[i64],
    actor: &str,
) -> Result<u64, String> {
    if rowids.is_empty() { return Ok(0); }

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    let placeholders = std::iter::repeat("?").take(rowids.len()).collect::<Vec<_>>().join(",");
    let payload_sql = format!(
        "SELECT id, from_id, relation_type, to_id, scope, valid_from, valid_to,
                confidence, properties_json, created_at
         FROM relations WHERE id IN ({})",
        placeholders
    );
    let mut stmt = tx.prepare(&payload_sql).map_err(|e| e.to_string())?;
    let bind: Vec<&dyn rusqlite::ToSql> = rowids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let payload: Vec<Value> = stmt.query_map(rusqlite::params_from_iter(bind.iter()), |r| {
        Ok(serde_json::json!({
            "id": r.get::<_, i64>(0)?,
            "from_id": r.get::<_, String>(1)?,
            "relation_type": r.get::<_, String>(2)?,
            "to_id": r.get::<_, String>(3)?,
            "scope": r.get::<_, String>(4)?,
            "valid_from": r.get::<_, String>(5)?,
            "valid_to": r.get::<_, Option<String>>(6)?,
            "confidence": r.get::<_, f64>(7)?,
            "properties_json": r.get::<_, String>(8)?,
            "created_at": r.get::<_, String>(9)?,
        }))
    }).map_err(|e| e.to_string())?
      .collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    drop(stmt);

    let del_sql = format!("DELETE FROM relations WHERE id IN ({})", placeholders);
    let bind: Vec<&dyn rusqlite::ToSql> = rowids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let deleted = tx.execute(&del_sql, rusqlite::params_from_iter(bind.iter()))
        .map_err(|e| e.to_string())? as u64;

    let now = chrono::Utc::now().to_rfc3339();
    let details = serde_json::json!({"op": "delete_relations_by_rowids", "payload": payload}).to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, NULL, NULL, ?3, ?4)",
        params![now, deleted as i64, actor, details],
    ).map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(deleted)
}
```

- [ ] **Step 4: Run to confirm it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p codeterm delete_relations_by_rowids_audits_payload -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/mem/graph_repair.rs src-tauri/src/mem/graph_repair/tests.rs
git commit -m "mem(graph): delete_relations_by_rowids with audit payload"
```

---

## Task 3: Add `merge_relations` (keep + fold-in with carryover)

**Files:**
- Modify: `src-tauri/src/mem/graph_repair.rs`
- Modify: `src-tauri/src/mem/graph_repair/tests.rs`

- [ ] **Step 1: Write the failing test**

Append to `tests.rs`:

```rust
#[test]
fn merge_relations_carries_over_max_confidence_and_merged_properties() {
    let conn = fresh_conn();
    let now = "2026-01-01T00:00:00Z";
    conn.execute(
        "INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at)
         VALUES ('a','knows','b','global',?1,NULL,0.7,'{\"src\":\"x\",\"shared\":1}',?1),
                ('a','knows','b','global',?1,NULL,0.9,'{\"src\":\"y\",\"extra\":2}',?1),
                ('a','knows','b','global',?1,NULL,0.5,'{\"shared\":99}',?1)",
        rusqlite::params![now],
    ).unwrap();
    let ids: Vec<i64> = conn.prepare("SELECT id FROM relations ORDER BY id").unwrap()
        .query_map([], |r| r.get(0)).unwrap()
        .collect::<Result<_, _>>().unwrap();

    let report = merge_relations(&conn, ids[0], &[ids[1], ids[2]], "test").unwrap();
    assert_eq!(report.merged_into, ids[0]);
    assert_eq!(report.deleted_rowids, vec![ids[1], ids[2]]);

    let (conf, props_json): (f64, String) = conn.query_row(
        "SELECT confidence, properties_json FROM relations WHERE id = ?1",
        rusqlite::params![ids[0]], |r| Ok((r.get(0)?, r.get(1)?))
    ).unwrap();
    assert!((conf - 0.9).abs() < 1e-6, "confidence should be max(0.7, 0.9, 0.5) = 0.9");

    let props: serde_json::Value = serde_json::from_str(&props_json).unwrap();
    assert_eq!(props["src"], "x", "kept-side wins on key collision");
    assert_eq!(props["shared"], 1, "kept-side wins on collision");
    assert_eq!(props["extra"], 2, "non-colliding keys are carried over");

    let remaining: i64 = conn.query_row("SELECT COUNT(*) FROM relations", [], |r| r.get(0)).unwrap();
    assert_eq!(remaining, 1);
}
```

- [ ] **Step 2: Run to confirm it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p codeterm merge_relations_carries_over -- --nocapture`
Expected: compile error.

- [ ] **Step 3: Implement `merge_relations` + report struct**

Append to `graph_repair.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeReport {
    pub merged_into: i64,
    pub deleted_rowids: Vec<i64>,
    pub final_confidence: f64,
}

pub fn merge_relations(
    conn: &Connection,
    keep: i64,
    merge: &[i64],
    actor: &str,
) -> Result<MergeReport, String> {
    if merge.is_empty() {
        return Err("merge list cannot be empty".into());
    }
    if merge.contains(&keep) {
        return Err("keep rowid cannot appear in merge list".into());
    }

    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    let mut all_ids = vec![keep];
    all_ids.extend_from_slice(merge);
    let placeholders = std::iter::repeat("?").take(all_ids.len()).collect::<Vec<_>>().join(",");
    let bind: Vec<&dyn rusqlite::ToSql> = all_ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();

    let mut stmt = tx.prepare(&format!(
        "SELECT id, confidence, properties_json FROM relations WHERE id IN ({})", placeholders
    )).map_err(|e| e.to_string())?;
    let rows: Vec<(i64, f64, String)> = stmt.query_map(rusqlite::params_from_iter(bind.iter()), |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    }).map_err(|e| e.to_string())?
      .collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    drop(stmt);

    if rows.len() != all_ids.len() {
        return Err(format!("expected {} rows, got {}", all_ids.len(), rows.len()));
    }

    let keep_row = rows.iter().find(|(id, _, _)| *id == keep)
        .ok_or_else(|| format!("keep row {} not found", keep))?;
    let mut keep_props: Map<String, Value> = serde_json::from_str(&keep_row.2)
        .ok().and_then(|v: Value| v.as_object().cloned()).unwrap_or_default();

    let mut max_conf = keep_row.1;
    for (id, conf, props_json) in &rows {
        if *id == keep { continue; }
        if *conf > max_conf { max_conf = *conf; }
        if let Ok(Value::Object(other)) = serde_json::from_str::<Value>(props_json) {
            for (k, v) in other {
                keep_props.entry(k).or_insert(v);
            }
        }
    }

    let merged_props_json = serde_json::to_string(&Value::Object(keep_props.clone()))
        .map_err(|e| e.to_string())?;

    tx.execute(
        "UPDATE relations SET confidence = ?1, properties_json = ?2 WHERE id = ?3",
        params![max_conf, merged_props_json, keep],
    ).map_err(|e| e.to_string())?;

    // Reuse delete primitive logic inline (transaction continues)
    let merge_placeholders = std::iter::repeat("?").take(merge.len()).collect::<Vec<_>>().join(",");
    let merge_bind: Vec<&dyn rusqlite::ToSql> = merge.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let deleted = tx.execute(
        &format!("DELETE FROM relations WHERE id IN ({})", merge_placeholders),
        rusqlite::params_from_iter(merge_bind.iter()),
    ).map_err(|e| e.to_string())? as i64;

    let now = chrono::Utc::now().to_rfc3339();
    let details = serde_json::json!({
        "op": "merge_relations",
        "kept": keep,
        "merged_in": merge,
        "final_confidence": max_conf,
        "final_properties": Value::Object(keep_props),
    }).to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, ?2, NULL, NULL, NULL, ?3, ?4)",
        params![now, deleted, actor, details],
    ).map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(MergeReport {
        merged_into: keep,
        deleted_rowids: merge.to_vec(),
        final_confidence: max_conf,
    })
}
```

- [ ] **Step 4: Run to confirm it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p codeterm merge_relations_carries_over -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/mem/graph_repair.rs src-tauri/src/mem/graph_repair/tests.rs
git commit -m "mem(graph): merge_relations with confidence/properties carryover"
```

---

## Task 4: Extend `add_alias` with entity property carryover

**Files:**
- Modify: `src-tauri/src/mem/graph.rs:396-431` (`add_alias`)
- Modify: `src-tauri/src/mem/graph.rs:783+` (existing test) — add property-merge assertion

- [ ] **Step 1: Write the failing test**

In `src-tauri/src/mem/graph.rs` tests block, append:

```rust
#[test]
fn add_alias_carries_over_properties_into_canonical() {
    let conn = test_conn();
    insert_entity(&conn, &Entity {
        id: "canonical".into(),
        entity_type: "Person".into(),
        scope: "global".into(),
        properties: serde_json::json!({"name": "Andrey", "shared": "canonical_wins"}),
    }).unwrap();
    insert_entity(&conn, &Entity {
        id: "alias".into(),
        entity_type: "Person".into(),
        scope: "global".into(),
        properties: serde_json::json!({"alt_name": "Дюша", "shared": "alias_loses"}),
    }).unwrap();

    add_alias(&conn, "alias", "canonical").unwrap();

    let props: String = conn.query_row(
        "SELECT properties_json FROM entities WHERE id = 'canonical'",
        [], |r| r.get(0)
    ).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&props).unwrap();
    assert_eq!(parsed["name"], "Andrey");
    assert_eq!(parsed["alt_name"], "Дюша", "non-colliding alias props must carry over");
    assert_eq!(parsed["shared"], "canonical_wins", "canonical wins on collision");
}
```

If `test_conn` doesn't exist in `graph.rs` tests, model it after the existing `add_alias_rewires_relations` test at line 783 — that test already builds a `Connection`.

- [ ] **Step 2: Run to confirm it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p codeterm add_alias_carries_over_properties -- --nocapture`
Expected: FAIL — alias props lost on delete.

- [ ] **Step 3: Modify `add_alias` to merge properties before deleting alias**

In `src-tauri/src/mem/graph.rs`, inside `add_alias` between the relation rewire block and `DELETE FROM entities WHERE id = ?1` (around line 423):

```rust
// Carryover: fold alias props into canonical (canonical wins on collision).
let alias_props: Option<String> = tx.query_row(
    "SELECT properties_json FROM entities WHERE id = ?1",
    params![alias_id], |r| r.get(0),
).ok();
let canonical_props: Option<String> = tx.query_row(
    "SELECT properties_json FROM entities WHERE id = ?1",
    params![canonical_id], |r| r.get(0),
).ok();
if let (Some(a), Some(c)) = (alias_props, canonical_props) {
    let alias_obj = serde_json::from_str::<Value>(&a).ok()
        .and_then(|v| v.as_object().cloned()).unwrap_or_default();
    let mut canon_obj = serde_json::from_str::<Value>(&c).ok()
        .and_then(|v| v.as_object().cloned()).unwrap_or_default();
    for (k, v) in alias_obj {
        canon_obj.entry(k).or_insert(v);
    }
    let merged = serde_json::to_string(&Value::Object(canon_obj))
        .map_err(|e| format!("merge props: {}", e))?;
    tx.execute(
        "UPDATE entities SET properties_json = ?1, updated_at = ?2 WHERE id = ?3",
        params![merged, &now, canonical_id],
    ).map_err(|e| format!("write merged props: {}", e))?;
}
```

Note: the existing code uses `conn.unchecked_transaction()` and binds to the variable name in the existing function — match whatever local name is used (look at lines 401-429 — it's `tx`).

- [ ] **Step 4: Run to confirm it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p codeterm add_alias -- --nocapture`
Expected: both `add_alias_rewires_relations` and `add_alias_carries_over_properties` PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/mem/graph.rs
git commit -m "mem(graph): add_alias carries alias properties into canonical"
```

---

## Task 5: Add `find_duplicate_entities` (exact + alias-chain)

**Files:**
- Modify: `src-tauri/src/mem/graph_repair.rs`
- Modify: `src-tauri/src/mem/graph_repair/tests.rs`

- [ ] **Step 1: Define the similarity strategy enum and result shape**

Append to `graph_repair.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EntitySimilarityStrategy {
    /// Same entity_type + same `properties_json.name` (case-insensitive trim).
    ExactName,
    /// Entities that resolve to the same canonical via existing aliases.
    AliasChain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateEntityGroup {
    pub canonical_hint: String,
    pub member_ids: Vec<String>,
    pub entity_type: String,
    pub reason: String,
}
```

- [ ] **Step 2: Write the failing test**

Append to `tests.rs`:

```rust
#[test]
fn find_duplicate_entities_by_exact_name() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(&conn, &Entity {
        id: "person_andrey".into(), entity_type: "Person".into(),
        scope: "global".into(), properties: serde_json::json!({"name": "Andrey"}),
    }).unwrap();
    insert_entity(&conn, &Entity {
        id: "person_andreyy".into(), entity_type: "Person".into(),
        scope: "global".into(), properties: serde_json::json!({"name": "andrey "}),
    }).unwrap();
    insert_entity(&conn, &Entity {
        id: "person_other".into(), entity_type: "Person".into(),
        scope: "global".into(), properties: serde_json::json!({"name": "Anna"}),
    }).unwrap();

    let groups = find_duplicate_entities(&conn, EntitySimilarityStrategy::ExactName, 100).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].entity_type, "Person");
    assert_eq!(groups[0].member_ids.len(), 2);
    assert!(groups[0].member_ids.contains(&"person_andrey".to_string()));
    assert!(groups[0].member_ids.contains(&"person_andreyy".to_string()));
}
```

- [ ] **Step 3: Run to confirm it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p codeterm find_duplicate_entities`
Expected: compile error.

- [ ] **Step 4: Implement `find_duplicate_entities`**

Append to `graph_repair.rs`:

```rust
pub fn find_duplicate_entities(
    conn: &Connection,
    strategy: EntitySimilarityStrategy,
    limit: usize,
) -> Result<Vec<DuplicateEntityGroup>, String> {
    match strategy {
        EntitySimilarityStrategy::ExactName => {
            let sql = "
                SELECT entity_type,
                       LOWER(TRIM(json_extract(properties_json, '$.name'))) AS norm_name,
                       GROUP_CONCAT(id) AS ids,
                       COUNT(*) AS c
                FROM entities
                WHERE json_extract(properties_json, '$.name') IS NOT NULL
                GROUP BY entity_type, norm_name
                HAVING c > 1
                ORDER BY c DESC
                LIMIT ?1";
            let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
            let rows = stmt.query_map(params![limit as i64], |r| {
                let ids_csv: String = r.get(2)?;
                Ok(DuplicateEntityGroup {
                    canonical_hint: r.get(1)?,
                    member_ids: ids_csv.split(',').map(String::from).collect(),
                    entity_type: r.get(0)?,
                    reason: "exact_name".into(),
                })
            }).map_err(|e| e.to_string())?;
            rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
        }
        EntitySimilarityStrategy::AliasChain => {
            let sql = "
                SELECT canonical_id, GROUP_CONCAT(alias_id) AS aliases, COUNT(*) AS c
                FROM entity_aliases
                GROUP BY canonical_id
                ORDER BY c DESC
                LIMIT ?1";
            let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
            let rows = stmt.query_map(params![limit as i64], |r| {
                let canonical: String = r.get(0)?;
                let aliases_csv: String = r.get(1)?;
                let mut members: Vec<String> = aliases_csv.split(',').map(String::from).collect();
                members.push(canonical.clone());
                Ok(DuplicateEntityGroup {
                    canonical_hint: canonical,
                    member_ids: members,
                    entity_type: "Unknown".into(),
                    reason: "alias_chain".into(),
                })
            }).map_err(|e| e.to_string())?;
            rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
        }
    }
}
```

- [ ] **Step 5: Run to confirm it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p codeterm find_duplicate_entities -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/mem/graph_repair.rs src-tauri/src/mem/graph_repair/tests.rs
git commit -m "mem(graph): find_duplicate_entities (exact-name + alias-chain)"
```

---

## Task 6: Add `augment_entity` (Dreamer enrichment)

**Files:**
- Modify: `src-tauri/src/mem/graph_repair.rs`
- Modify: `src-tauri/src/mem/graph_repair/tests.rs`

- [ ] **Step 1: Write the failing test**

Append to `tests.rs`:

```rust
#[test]
fn augment_entity_patches_properties_and_logs() {
    let conn = fresh_conn();
    use crate::mem::graph::{insert_entity, Entity};
    insert_entity(&conn, &Entity {
        id: "person_andrey".into(),
        entity_type: "Person".into(),
        scope: "global".into(),
        properties: serde_json::json!({"name": "Andrey"}),
    }).unwrap();

    let mut patch = serde_json::Map::new();
    patch.insert("city".into(), serde_json::json!("Riga"));
    patch.insert("description".into(), serde_json::json!("Senior iOS dev, 10y XP"));

    augment_entity(&conn, "person_andrey", patch, "dreamer").unwrap();

    let props: String = conn.query_row(
        "SELECT properties_json FROM entities WHERE id = 'person_andrey'",
        [], |r| r.get(0)
    ).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&props).unwrap();
    assert_eq!(parsed["name"], "Andrey", "existing keys preserved");
    assert_eq!(parsed["city"], "Riga", "new keys added");
    assert_eq!(parsed["description"], "Senior iOS dev, 10y XP");

    let (actor, details): (String, String) = conn.query_row(
        "SELECT actor, details_json FROM graph_purge_log ORDER BY id DESC LIMIT 1",
        [], |r| Ok((r.get(0)?, r.get(1)?))
    ).unwrap();
    assert_eq!(actor, "dreamer");
    let parsed: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(parsed["op"], "augment_entity");
    assert_eq!(parsed["entity_id"], "person_andrey");
}
```

Note the asymmetry: an augment also writes to `graph_purge_log` even though it's not destructive. Rationale: one audit table for all Dreamer-initiated graph mutations is simpler than fanning out into separate logs. The `count` column gets 0 for augments.

- [ ] **Step 2: Run to confirm it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p codeterm augment_entity_patches`
Expected: compile error.

- [ ] **Step 3: Implement `augment_entity`** — PATCH semantics: new keys added, existing keys overwritten by patch (this is the opposite of merge — augment is "I learned new info, write it through").

Append to `graph_repair.rs`:

```rust
pub fn augment_entity(
    conn: &Connection,
    entity_id: &str,
    patch: Map<String, Value>,
    actor: &str,
) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    let existing: String = tx.query_row(
        "SELECT properties_json FROM entities WHERE id = ?1",
        params![entity_id], |r| r.get(0),
    ).map_err(|e| format!("entity {} not found: {}", entity_id, e))?;

    let mut props: Map<String, Value> = serde_json::from_str(&existing)
        .ok().and_then(|v: Value| v.as_object().cloned()).unwrap_or_default();
    for (k, v) in &patch {
        props.insert(k.clone(), v.clone());
    }
    let merged = serde_json::to_string(&Value::Object(props.clone()))
        .map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    tx.execute(
        "UPDATE entities SET properties_json = ?1, updated_at = ?2 WHERE id = ?3",
        params![merged, &now, entity_id],
    ).map_err(|e| e.to_string())?;

    let details = serde_json::json!({
        "op": "augment_entity",
        "entity_id": entity_id,
        "patch": Value::Object(patch),
        "result": Value::Object(props),
    }).to_string();
    tx.execute(
        "INSERT INTO graph_purge_log (purged_at, count, scope, before, reason_filter, actor, details_json)
         VALUES (?1, 0, NULL, NULL, NULL, ?2, ?3)",
        params![now, actor, details],
    ).map_err(|e| e.to_string())?;

    tx.commit().map_err(|e| e.to_string())?;
    Ok(())
}
```

- [ ] **Step 4: Run to confirm it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml -p codeterm augment_entity_patches -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/mem/graph_repair.rs src-tauri/src/mem/graph_repair/tests.rs
git commit -m "mem(graph): augment_entity — patch props with audit"
```

---

## Task 7: Expose repair ops as manager methods

**Files:**
- Modify: `src-tauri/src/mem/manager_graph.rs`

- [ ] **Step 1: Add manager wrappers**

Append to `src-tauri/src/mem/manager_graph.rs`:

```rust
use super::graph_repair::{
    find_duplicate_relations, delete_relations_by_rowids, merge_relations,
    find_duplicate_entities, augment_entity,
    RelationColumn, DuplicateRelationGroup, DuplicateEntityGroup,
    EntitySimilarityStrategy, MergeReport,
};

impl super::MemManager {
    pub fn graph_find_duplicate_relations(
        &self,
        group_by: &[RelationColumn],
        limit: usize,
    ) -> Result<Vec<DuplicateRelationGroup>, String> {
        let conn = self.conn().lock();
        find_duplicate_relations(&conn, group_by, limit)
    }

    pub fn graph_delete_relations(&self, rowids: &[i64], actor: &str) -> Result<u64, String> {
        let conn = self.conn().lock();
        delete_relations_by_rowids(&conn, rowids, actor)
    }

    pub fn graph_merge_relations(&self, keep: i64, merge: &[i64], actor: &str) -> Result<MergeReport, String> {
        let conn = self.conn().lock();
        merge_relations(&conn, keep, merge, actor)
    }

    pub fn graph_find_duplicate_entities(
        &self,
        strategy: EntitySimilarityStrategy,
        limit: usize,
    ) -> Result<Vec<DuplicateEntityGroup>, String> {
        let conn = self.conn().lock();
        find_duplicate_entities(&conn, strategy, limit)
    }

    pub fn graph_augment_entity(
        &self,
        entity_id: &str,
        patch: serde_json::Map<String, serde_json::Value>,
        actor: &str,
    ) -> Result<(), String> {
        let conn = self.conn().lock();
        augment_entity(&conn, entity_id, patch, actor)
    }
}
```

Note: this respects lock ordering (only `mux`-derived `conn` is held, no nested locks).

- [ ] **Step 2: Verify compilation**

Run: `./build.sh check`
Expected: clean compile.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/mem/manager_graph.rs
git commit -m "mem(manager): expose graph repair ops"
```

---

## Task 8: CLI surface — `codeterm mem graph repair *`

**Files:**
- Modify: `cli/src/args/mem.rs` (locate the `Graph` subcommand enum near line 656 where `purge` is documented)
- Modify: the CLI dispatch file for graph commands (find via `grep -rn "graph_alias\|graph_related" cli/src/` and edit there)

- [ ] **Step 1: Locate CLI structure**

Run: `grep -rn "enum.*Graph\|graph_alias\|graph_related" cli/src/ 2>&1 | head -10`
Use the output to find the dispatch file and the Graph subcommand enum. The remaining steps assume:
- Subcommand enum lives in `cli/src/args/mem.rs`
- Dispatch lives in `cli/src/commands/mem.rs` or `cli/src/commands/mem/graph.rs`

If the actual paths differ, adapt — the structure (clap subcommand + dispatch function) is the same.

- [ ] **Step 2: Add the `Repair` subcommand tree**

In `cli/src/args/mem.rs`, inside the `Graph` enum:

```rust
/// Find/delete/merge graph duplicates, augment node properties.
/// Designed for Dreamer self-repair; safe for manual use.
Repair {
    #[command(subcommand)]
    op: GraphRepairOp,
},
```

Then define:

```rust
#[derive(clap::Subcommand, Debug)]
pub enum GraphRepairOp {
    /// Find duplicate relations grouped by selected columns.
    FindRelations {
        /// Comma-separated columns: from_id,relation_type,to_id,scope,valid_from
        #[arg(long, default_value = "from_id,relation_type,to_id,valid_from")]
        group_by: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Delete relations by explicit rowids (audited).
    DeleteRelations {
        /// Comma-separated rowids
        #[arg(long)]
        rowids: String,
        #[arg(long, default_value = "user")]
        actor: String,
    },
    /// Merge relations into `keep`, folding properties/confidence.
    MergeRelations {
        #[arg(long)]
        keep: i64,
        /// Comma-separated rowids to merge in
        #[arg(long)]
        merge: String,
        #[arg(long, default_value = "user")]
        actor: String,
    },
    /// Find duplicate entities (exact name match or alias chain).
    FindEntities {
        #[arg(long, value_enum, default_value = "exact-name")]
        strategy: FindEntitiesStrategyArg,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Merge entity into canonical via aliasing (extends add_alias).
    MergeEntities {
        #[arg(long)]
        alias: String,
        #[arg(long)]
        canonical: String,
    },
    /// Patch properties on an existing entity.
    AugmentEntity {
        #[arg(long)]
        id: String,
        /// JSON object of properties to merge in (e.g. '{"city":"Riga"}')
        #[arg(long)]
        patch: String,
        #[arg(long, default_value = "user")]
        actor: String,
    },
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum FindEntitiesStrategyArg {
    ExactName,
    AliasChain,
}
```

- [ ] **Step 3: Add dispatch handlers**

In the matching commands dispatch file, route each `Repair { op: ... }` arm to the manager method. Use the REST client if the CLI talks over HTTP — match the pattern of existing `graph related` / `graph alias` handlers. Each command prints JSON when `--json` is set; otherwise prints human-readable table.

Example for `FindRelations` (adapt path/imports to match):

```rust
GraphRepairOp::FindRelations { group_by, limit, json } => {
    let cols: Vec<RelationColumn> = group_by.split(',')
        .map(|s| match s.trim() {
            "from_id" => Ok(RelationColumn::FromId),
            "relation_type" => Ok(RelationColumn::RelationType),
            "to_id" => Ok(RelationColumn::ToId),
            "scope" => Ok(RelationColumn::Scope),
            "valid_from" => Ok(RelationColumn::ValidFrom),
            x => Err(format!("unknown column: {}", x)),
        })
        .collect::<Result<_, _>>()?;
    let groups = client.graph_find_duplicate_relations(&cols, limit).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&groups)?);
    } else {
        for g in &groups {
            println!("count={} rowids={:?} key={}", g.count, g.rowids, g.key);
        }
    }
    Ok(())
}
```

Do the same for the other 5 arms.

- [ ] **Step 4: Smoke-test the CLI against dev**

Run:
```bash
./build.sh dev --wait
codeterm mem graph repair find-relations --group-by from_id,relation_type,to_id,valid_from --json
```
Expected: empty array on a clean dev DB; on rollacode-bot dev DB would show actual dupes.

- [ ] **Step 5: Commit**

```bash
git add cli/
git commit -m "mem(cli): graph repair subcommands (find/delete/merge/augment)"
```

---

## Task 9: REST endpoints — POST /api/mem/graph/repair/*

**Files:**
- Modify: locate the mem API router via `grep -rn 'graph/related\|/api/mem' src-tauri/src/api/ 2>&1 | head -5`. Endpoints live alongside existing graph routes.

- [ ] **Step 1: Add the 6 routes**

In the mem API module, register routes following the existing pattern (`POST /api/mem/graph/related` etc.):

```rust
.route("/api/mem/graph/repair/find-relations", post(repair_find_relations))
.route("/api/mem/graph/repair/delete-relations", post(repair_delete_relations))
.route("/api/mem/graph/repair/merge-relations", post(repair_merge_relations))
.route("/api/mem/graph/repair/find-entities", post(repair_find_entities))
.route("/api/mem/graph/repair/merge-entities", post(repair_merge_entities))
.route("/api/mem/graph/repair/augment-entity", post(repair_augment_entity))
```

Each handler:
- Deserializes a typed request body (matching the manager method signature).
- Calls `state.mem.graph_<op>(...)`.
- Returns the result as JSON.
- Auth: same as existing graph endpoints (dev token / api token middleware).

Example handler:

```rust
#[derive(Deserialize)]
struct FindRelationsReq {
    group_by: Vec<RelationColumn>,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize { 50 }

async fn repair_find_relations(
    State(state): State<AppState>,
    Json(req): Json<FindRelationsReq>,
) -> Result<Json<Vec<DuplicateRelationGroup>>, ApiError> {
    let groups = state.mem.graph_find_duplicate_relations(&req.group_by, req.limit)?;
    Ok(Json(groups))
}
```

Repeat for the other 5 routes.

- [ ] **Step 2: Smoke-test with curl**

```bash
TOKEN=$(cat ~/.codeterm-dev/api_token)
curl -sS -X POST http://127.0.0.1:7686/api/mem/graph/repair/find-relations \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"group_by":["from_id","relation_type","to_id","valid_from"],"limit":10}'
```
Expected: `[]` on clean dev DB.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/api/
git commit -m "mem(api): POST /api/mem/graph/repair/* endpoints"
```

---

## Task 10: Update REM Dreamer prompt with Self-Repair Playbook

**Files:**
- Modify: `src-tauri/prompts/dreaming_rem.md`

- [ ] **Step 1: Add a new section to the prompt**

At an appropriate place in `dreaming_rem.md` (after the existing analysis/promotion sections, before the diary-writing section), insert:

```markdown
## Self-Repair Playbook

You have access to graph repair CLI commands. Use them when you find issues. **No raw SQL — only these typed calls.**

### Step 1 — Audit (read-only)

```bash
codeterm mem graph repair find-relations --group-by from_id,relation_type,to_id,valid_from --json --limit 100
codeterm mem graph repair find-entities --strategy exact-name --json --limit 50
codeterm mem graph repair find-entities --strategy alias-chain --json --limit 50
```

Read the output. For each group, decide which action fits.

### Step 2 — Decide per group

For each `find-relations` group:
- **Exact duplicates, identical payload** → `delete-relations --rowids <all-but-one-from-rowids>`. Pick the row with lowest rowid as the keeper.
- **Same key but different `confidence` or `properties_json`** → `merge-relations --keep <best-rowid> --merge <other-rowids>`. Best = highest confidence, or lowest rowid if tied.
- **Unsure (semantics ambiguous)** → skip and note in diary. Do NOT guess.

For each `find-entities` group:
- **Clear single-canonical case** (e.g. `person_andrey` and `person_andreyy` with same name) → pick a canonical, then `merge-entities --alias <other> --canonical <chosen>`. Properties auto-merge.
- **Unsure which is canonical** → skip and note in diary.

### Step 3 — Enrich (optional)

If during your analysis you derived new factual information about an existing entity (e.g. a city, a role, a description) and the source is grounded in mem_items, augment the entity:

```bash
codeterm mem graph repair augment-entity --id person_andrey --patch '{"city":"Riga","role":"Senior iOS dev"}' --actor dreamer
```

Rules:
- Only augment when the patch comes from explicit facts in mem_items, not inference.
- The patch overwrites existing keys — be conservative on collisions.

### Step 4 — Diary

Always write a diary entry summarizing: counts found, actions taken, items skipped (and why). The `graph_purge_log` already captures the rowid-level audit, so the diary should be human-readable narrative.
```

- [ ] **Step 2: Verify the prompt loads**

Run: `./build.sh dev --wait` then trigger a Dreamer REM cycle (via cron-trigger or `codeterm mem dream now --phase rem`). Check the prompt was loaded — log line should show the rendered prompt or the agent transcript should reflect the new section.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/prompts/dreaming_rem.md
git commit -m "prompts(dreaming): add self-repair playbook for REM"
```

---

## Task 11: End-to-end eval — seeded duplicates, verify Dreamer cleans them up

**Files:**
- Create: `evals/dreamer-repair/README.md`
- Create: `evals/dreamer-repair/seed.sh`
- Create: `evals/dreamer-repair/assert.sh`
- Create: `evals/dreamer-repair/run.sh`

- [ ] **Step 1: Create the seed script**

Create `evals/dreamer-repair/seed.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"

# Seeds the dev DB with: 2 entities that should merge,
# 3 exact-duplicate relations, 2 differing-confidence duplicates.
NOW="2026-01-01T00:00:00Z"

dc mem save --kind note --title "andrey context" "Andrey is a senior iOS developer in Riga."

# Force-insert two near-duplicate entities (bypass extractor for determinism)
sqlite3 ~/.codeterm-dev/mem/knowledge.db "
INSERT INTO entities (id, entity_type, scope, properties_json, created_at, updated_at)
VALUES ('seed_person_andrey', 'Person', 'global', '{\"name\":\"Andrey\"}', '$NOW', '$NOW'),
       ('seed_person_andreyy', 'Person', 'global', '{\"name\":\"andrey \",\"alt_name\":\"Дюша\"}', '$NOW', '$NOW');

INSERT INTO relations (from_id, relation_type, to_id, scope, valid_from, valid_to, confidence, properties_json, created_at)
VALUES ('seed_person_andrey','knows','seed_person_anna','global','$NOW',NULL,1.0,'{}','$NOW'),
       ('seed_person_andrey','knows','seed_person_anna','global','$NOW',NULL,1.0,'{}','$NOW'),
       ('seed_person_andrey','knows','seed_person_anna','global','$NOW',NULL,1.0,'{}','$NOW'),
       ('seed_person_andrey','works_on','seed_proj_codeterm','global','$NOW',NULL,0.7,'{\"src\":\"x\"}','$NOW'),
       ('seed_person_andrey','works_on','seed_proj_codeterm','global','$NOW',NULL,0.9,'{\"src\":\"y\"}','$NOW');
"

echo "seeded:"
dc mem graph repair find-relations --group-by from_id,relation_type,to_id,valid_from --json
```

- [ ] **Step 2: Create the assert script**

Create `evals/dreamer-repair/assert.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"

# After Dreamer REM ran, assert:
# 1. No exact-key duplicates remain.
groups=$(dc mem graph repair find-relations --group-by from_id,relation_type,to_id,valid_from --json | jq 'length')
if [[ "$groups" != "0" ]]; then
    echo "FAIL: $groups duplicate groups remain" >&2
    exit 1
fi

# 2. The (works_on) confidence merge happened — final confidence 0.9.
conf=$(sqlite3 ~/.codeterm-dev/mem/knowledge.db \
    "SELECT confidence FROM relations WHERE from_id='seed_person_andrey' AND relation_type='works_on'")
if [[ "$conf" != "0.9" ]]; then
    echo "FAIL: expected merged confidence 0.9, got $conf" >&2
    exit 1
fi

# 3. The entity merge happened — only one Andrey entity remains.
count=$(sqlite3 ~/.codeterm-dev/mem/knowledge.db \
    "SELECT COUNT(*) FROM entities WHERE id LIKE 'seed_person_andrey%'")
if [[ "$count" != "1" ]]; then
    echo "FAIL: expected 1 Andrey entity, got $count" >&2
    exit 1
fi

# 4. Audit log has entries for the actions.
audits=$(sqlite3 ~/.codeterm-dev/mem/knowledge.db \
    "SELECT COUNT(*) FROM graph_purge_log WHERE actor='dreamer'")
if [[ "$audits" -lt "2" ]]; then
    echo "FAIL: expected ≥2 audit entries from dreamer, got $audits" >&2
    exit 1
fi

echo "PASS"
```

- [ ] **Step 3: Create the runner**

Create `evals/dreamer-repair/run.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
source "$(git rev-parse --show-toplevel)/evals/_lib/lib.sh"

bash "$(dirname "$0")/seed.sh"

# Trigger REM Dreamer (this CLI signature may differ — adapt to actual API)
dc mem dream now --phase rem --wait

bash "$(dirname "$0")/assert.sh"
```

- [ ] **Step 4: Add README**

Create `evals/dreamer-repair/README.md`:

```markdown
# Dreamer Self-Repair Eval

Seeds the dev mem DB with intentional graph pollution (duplicate relations,
near-duplicate entities, differing-confidence rows), runs the REM Dreamer
once, and asserts the pollution is cleaned up by the self-repair playbook.

## Run

```bash
./build.sh dev --wait
bash evals/dreamer-repair/run.sh
```

## Assertions

1. No exact-key relation duplicates remain.
2. The differing-confidence duplicates collapsed into one with `confidence=0.9` (max).
3. The two `seed_person_andrey*` entities merged into one.
4. `graph_purge_log` has ≥2 entries with `actor='dreamer'`.

Cleanup is manual — drop the dev DB or `delete-relations` the seed rows.
```

- [ ] **Step 5: Run the eval, iterate on Dreamer prompt until it passes**

```bash
chmod +x evals/dreamer-repair/*.sh
./build.sh dev --wait
bash evals/dreamer-repair/run.sh
```

If FAIL: read the Dreamer's last transcript (`~/.codeterm-dev/logs/codeterm.log` or worker output dir), see where it got confused, tweak `dreaming_rem.md` until the eval passes.

- [ ] **Step 6: Commit**

```bash
git add evals/dreamer-repair/
git commit -m "evals(dreamer-repair): seed graph dupes + assert auto-cleanup"
```

---

## Post-implementation checklist

After Task 11 passes, run all of these before declaring done:

- [ ] `./build.sh test` — full Rust unit test suite passes.
- [ ] `./build.sh check` — no clippy warnings.
- [ ] `scripts/check-file-sizes.sh` — `graph_repair.rs` < 1000 LOC.
- [ ] `bash evals/dreamer-repair/run.sh` — passes against fresh dev.
- [ ] **Real-data dry run on rollacode-bot:**
  - SSH into rollacode-bot.
  - Pull the dev branch with these changes.
  - **Make a backup of `~/.codeterm/mem/knowledge.db` first.**
  - Run `codeterm mem graph repair find-relations --group-by from_id,relation_type,to_id,valid_from --json --limit 1000` and inspect the output volume.
  - Decide whether to run the Dreamer REM cycle on prod or do a controlled manual pass.
- [ ] Update `docs/MEMORY.md` — add a "Graph Self-Repair" section pointing at the new CLI commands.
- [ ] Bump `<!-- revision: N -->` in `docs/MEMORY.md`.
- [ ] Close GitHub issue #112 with a comment linking the eval and the cleanup audit count.

## Risks and mitigations

- **Dreamer mistakes a non-duplicate for a duplicate and merges incorrectly.** Mitigation: `graph_purge_log.details_json` carries full payload of deleted rows. A `mem graph repair undo --log-id N` command (not in this plan — future work) could replay from the audit log. For now, manual SQL recovery from audit if needed.
- **Property carryover semantics surprise existing `add_alias` callers.** Mitigation: the project has no external users; callers are internal. Audit them after Task 4: `grep -rn "add_alias\|graph_alias\b" src-tauri/ cli/`. If any caller relies on the alias's properties being discarded, fix at call site.
- **CLI binding column whitelist drifts from schema.** Mitigation: the `RelationColumn` enum is the single source of truth — adding a column to the table requires adding it to the enum, which is a compile-time check.
- **REST endpoints expose destructive ops to anyone with a valid token.** Mitigation: same auth as existing graph endpoints. Not a regression. Dev token is already required.

---

**Plan complete and saved to `docs/superpowers/plans/2026-05-17-dreamer-self-repair.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints.

**Which approach?**

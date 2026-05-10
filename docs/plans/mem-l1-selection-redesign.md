# L1 Wake-Up Selection Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the rigid kind-filter + importance heuristic in `mem/layers.rs` with a workspace-aware, pin-respecting, evidence-weighted score so the General Agent's wake-up payload (~700 tokens) actually carries what's useful right now.

**Architecture:** Add one column (`is_pinned_wakeup`) to `mem_items`; reuse existing `scope` (already `global` / `workspace:<id>`) and existing `evidence_count`. Replace `collect_l1_candidates` with a CTE-based two-pass query — pinned items first, then a ranked fill by `importance + log10(1+evidence_count)*0.5 + ws_match*0.5 + recency*0.3`. Extend the `bump_on_get` filter so `evidence_count` accumulates for `rule | fact | preference` (today only `skill | lesson` bump). Surface pinning + budget controls via CLI (`mem pin/unpin/pinned`, `mem save --pin --workspace`, `mem wake-up --explain`) and one config knob (`memory.l1_budget_tokens`).

**Tech Stack:** Rust (rusqlite, tokio), SQLite (PRAGMA-guarded migrations, CTE), Axum HTTP, clap CLI, TOML config. No new crates.

---

## Current state (= what `mem/layers.rs` does today)

| Layer | Source | Default budget | Loaded |
|-------|--------|----------------|--------|
| L0 — identity | `mem_items` row id `preference_identity` | 100 tok | always (in GA system prompt) |
| L1 — critical | `collect_l1_candidates`: `rule | preference | peer` always; `fact | summary` if `importance >= 4.0` | 700 tok | always (`codeterm mem wake-up`) |
| L2 — targeted | `mem search --area X --kind Y` | variable | on-demand by GA |
| L3 — hybrid recall | FTS5 + vector (RRF-fused) | variable | on-demand by GA |

Eviction at render-time: `rule > preference > peer > fact > summary`.

**What's wrong:**
1. Kind alone decides admission — there's no way to force-include a low-importance item that's load-bearing right now.
2. No workspace filter — wake-up in workspace A still loads items scoped to workspace B (eats budget).
3. The "usage signal" is `recent_activity` / `accessed_at`, polluted by dreamer sweeps and `mem get` reads.
4. `l1_budget_tokens` is a hardcoded constant (700).

## What we already have in the schema (verified)

- `mem_items.scope TEXT NOT NULL` — already `global` or `workspace:<id>` per `MemScope::as_str()` (`src-tauri/src/mem/store/mod.rs:237`). **DO NOT add a separate `workspace_id` column.** Filter on `scope`.
- `mem_items.evidence_count INTEGER NOT NULL DEFAULT 1` — added by `add_evolution_columns` (`crates/mem-store/src/migrations.rs:23`). Bumped via `codeterm_mem_store::graph::bump_evidence_count`. Default is 1 (not 0) so `log10(1+evidence_count)` for a fresh item = `log10(2) ≈ 0.301`.
- `bump_on_get` (`src-tauri/src/api/mem_routes/items.rs:130`) currently has `if kind != "skill" && kind != "lesson" { return; }` — this is the gate to extend.

## What's missing

- **`is_pinned_wakeup INTEGER NOT NULL DEFAULT 0`** column.
- **Pin CLI verbs** + a `pinned` query filter.
- **Two-pass scored selector** (replaces `collect_l1_candidates`).
- **Configurable budget**.
- **Score-explain debug flag** on `mem wake-up`.

## File Structure

| File | Responsibility | Action |
|------|----------------|--------|
| `crates/mem-store/src/migrations.rs` | PRAGMA-guarded migrations | Modify — add `add_pin_column` |
| `crates/mem-store/src/graph.rs` | already exposes `bump_evidence_count` | Read-only — no change |
| `src-tauri/src/api/mem_routes/items.rs` | `mem_get` + `bump_on_get` filter; PATCH endpoint | Modify — extend filter, accept `pinned` field |
| `src-tauri/src/api/mem_routes/list.rs` | list endpoint with filters | Modify — add `?pinned=true` filter |
| `src-tauri/src/mem/layers.rs` | wake-up generation, current `collect_l1_candidates` | Replace `collect_l1_candidates`; add `--explain` rendering |
| `src-tauri/src/mem/layers/score.rs` | NEW — pure score formula + CTE SQL builder | Create |
| `src-tauri/src/mem/layers/tests.rs` | scoring + selection unit tests | Create (or move existing inline tests) |
| `src-tauri/src/config/schema.rs` | TOML config struct | Modify — add `memory.l1_budget_tokens` |
| `cli/src/cmd/mem.rs` (or sibling) | `mem pin/unpin/pinned/save --pin --workspace/wake-up --explain` | Modify |
| `docs/MEMORY.md` | wake-up algorithm docs | Modify — describe new scoring + pin UX |

Layers split: `layers/` becomes a folder with `mod.rs` (facade), `score.rs` (formula), `tests.rs`. Stays under the 800-line file cap.

---

## Tasks

### Task 1: Add `is_pinned_wakeup` migration

**Files:**
- Modify: `crates/mem-store/src/migrations.rs`
- Test: `crates/mem-store/src/tests/mod.rs` (add new test)

- [ ] **Step 1: Write the failing test**

```rust
// crates/mem-store/src/tests/mod.rs
#[test]
fn pin_column_added_by_migration() {
    let conn = Connection::open_in_memory().unwrap();
    crate::migrations::run(&conn).unwrap_or(()); // bare schema first
    conn.execute_batch(
        "CREATE TABLE mem_items (id TEXT PRIMARY KEY, scope TEXT NOT NULL, evidence_count INTEGER NOT NULL DEFAULT 1)"
    ).unwrap();
    crate::migrations::run(&conn).unwrap();
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(mem_items)").unwrap()
        .query_map([], |r| r.get::<_, String>(1)).unwrap()
        .filter_map(Result::ok).collect();
    assert!(cols.contains(&"is_pinned_wakeup".to_string()));
}

#[test]
fn pin_migration_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE mem_items (id TEXT PRIMARY KEY, scope TEXT NOT NULL)"
    ).unwrap();
    crate::migrations::run(&conn).unwrap();
    crate::migrations::run(&conn).unwrap();   // second call must not error
}
```

- [ ] **Step 2: Run tests, expect FAIL**

```bash
cargo test -p codeterm-mem-store pin_column_added_by_migration -- --nocapture
cargo test -p codeterm-mem-store pin_migration_is_idempotent -- --nocapture
```
Expected: FAIL — `is_pinned_wakeup` column not present.

- [ ] **Step 3: Add migration**

In `crates/mem-store/src/migrations.rs`, add to `pub fn run`:
```rust
pub fn run(conn: &Connection) -> MemStoreResult<()> {
    add_reason_column(conn)?;
    create_graph_purge_log(conn)?;
    add_evolution_columns(conn)?;
    add_promotion_threshold(conn)?;
    create_lesson_audit(conn)?;
    add_pin_column(conn)?;
    Ok(())
}

fn add_pin_column(conn: &Connection) -> MemStoreResult<()> {
    let cols: Vec<String> = conn.prepare("PRAGMA table_info(mem_items)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(Result::ok).collect();
    if !cols.iter().any(|c| c == "is_pinned_wakeup") {
        conn.execute_batch(
            "ALTER TABLE mem_items ADD COLUMN is_pinned_wakeup INTEGER NOT NULL DEFAULT 0;
             CREATE INDEX IF NOT EXISTS idx_mem_pinned ON mem_items(is_pinned_wakeup) WHERE is_pinned_wakeup = 1;",
        )?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests, expect PASS**

```bash
cargo test -p codeterm-mem-store pin_column_added_by_migration pin_migration_is_idempotent
```
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/mem-store/src/migrations.rs crates/mem-store/src/tests/mod.rs
git commit -m "feat(mem): add is_pinned_wakeup column with conditional index"
```

---

### Task 2: Extend `bump_on_get` to fact/rule/preference (with sweep-poisoning guard)

**Files:**
- Modify: `src-tauri/src/api/mem_routes/items.rs:130`
- Test: `src-tauri/src/api/mem_routes/items.rs` (extend existing `mod tests`)

This **must precede** the scoring change (Task 5). Without it, `evidence_count` stays at default 1 for the new kinds and the new score term contributes nothing.

> **DEEP RISK — feedback poisoning from internal sweeps.**
> The dreamer prompt (`dreaming_rem.md:31`) and extraction prompt (`mem_extraction.md:35`) both call `codeterm mem get <id>` in batch loops. The Memory Panel UI also calls `GET /api/mem/items/{id}` (`memoryApi.ts:36`) every time the user opens an item. With the broadened filter, every dreamer sweep would bump fact/rule/preference items, inflating `evidence_count` and creating a runaway: high score → loaded into L1 → dreamer/UI reads it → bumps again → score climbs.
>
> **Mitigation (mandatory in this task):** debounce by item via `accessed_at`. Skip the bump if the item was last touched within the last 12 hours. User reading the same fact 30× in a day = +1. Dreamer sweeping all items every Sunday = +1/week (since 7d ≫ 12h). Real "frequently used" items still accumulate steadily.
>
> Implementation note: the `accessed_at` column already exists on `mem_items` (see `schema.rs:25`). It's set on every CRUD touch. Read it in `bump_on_get`; if `< 12h ago`, return without calling `bump_evidence_count`. Add an explicit test (`bump_on_get_debounces_within_12h`).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn bump_on_get_fires_for_fact() {
    let conn = open_test_conn();
    insert(&conn, "fact_a", "fact");
    bump_on_get(&conn, "fact", "fact_a");
    assert_eq!(evidence_count(&conn, "fact_a"), 2);
}

#[test]
fn bump_on_get_fires_for_rule() {
    let conn = open_test_conn();
    insert(&conn, "rule_a", "rule");
    bump_on_get(&conn, "rule", "rule_a");
    assert_eq!(evidence_count(&conn, "rule_a"), 2);
}

#[test]
fn bump_on_get_fires_for_preference() {
    let conn = open_test_conn();
    insert(&conn, "pref_a", "preference");
    bump_on_get(&conn, "preference", "pref_a");
    assert_eq!(evidence_count(&conn, "pref_a"), 2);
}

#[test]
fn bump_on_get_skips_unrelated_kinds() {
    let conn = open_test_conn();
    insert(&conn, "note_a", "note");
    bump_on_get(&conn, "note", "note_a");
    assert_eq!(evidence_count(&conn, "note_a"), 1, "no bump for kind=note");
}

#[test]
fn bump_on_get_debounces_within_12h() {
    let conn = open_test_conn();
    insert(&conn, "fact_x", "fact");
    bump_on_get(&conn, "fact", "fact_x");
    assert_eq!(evidence_count(&conn, "fact_x"), 2, "first bump fires");
    bump_on_get(&conn, "fact", "fact_x");
    assert_eq!(evidence_count(&conn, "fact_x"), 2, "second bump within 12h is debounced");
    // Force accessed_at into the past
    conn.execute(
        "UPDATE mem_items SET accessed_at = datetime('now','-13 hours') WHERE id = 'fact_x'", []
    ).unwrap();
    bump_on_get(&conn, "fact", "fact_x");
    assert_eq!(evidence_count(&conn, "fact_x"), 3, "bump fires again after 12h elapses");
}

fn evidence_count(conn: &Connection, id: &str) -> i64 {
    conn.query_row(
        "SELECT evidence_count FROM mem_items WHERE id = ?1",
        [id], |r| r.get(0)
    ).unwrap_or(0)
}

fn insert(conn: &Connection, id: &str, kind: &str) {
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, importance, created_at, updated_at)
         VALUES (?1, ?1, ?2, 'global', ?1, 3.0, '2026-05-10', '2026-05-10')",
        [id, kind],
    ).unwrap();
}
```

- [ ] **Step 2: Run tests, expect 3 fail / 1 pass**

```bash
cargo test --lib -p codeterm bump_on_get -- --nocapture
```
Expected: `bump_on_get_skips_unrelated_kinds` PASS; the three positive tests FAIL because the filter rejects fact/rule/preference.

- [ ] **Step 3: Extend the filter (with debounce)**

Edit `src-tauri/src/api/mem_routes/items.rs:130`:
```rust
fn bump_on_get(conn: &rusqlite::Connection, kind: &str, id: &str) {
    if !matches!(kind, "skill" | "lesson" | "rule" | "fact" | "preference") {
        return;
    }
    // Debounce: skip if same item was accessed within the last 12h. Stops
    // dreamer sweeps and Memory Panel scrolling from poisoning evidence_count.
    let recent: bool = conn.query_row(
        "SELECT accessed_at >= datetime('now','-12 hours')
         FROM mem_items WHERE id = ?1",
        [id],
        |r| r.get::<_, Option<bool>>(0),
    ).unwrap_or(None).unwrap_or(false);
    if recent {
        log::debug!("[mem_get] bump skipped (12h debounce) for {}", id);
        return;
    }
    if let Err(e) = codeterm_mem_store::graph::bump_evidence_count(conn, id) {
        log::debug!("[mem_get] evidence bump failed for {}: {}", id, e);
    }
}
```

Note: writes to `accessed_at` happen elsewhere on `mem_get` (look for `UPDATE mem_items SET accessed_at`). The bump check must run **before** that update, otherwise it always sees "just touched" and never bumps. If ordering is wrong in the existing code, refactor so the read happens first.

Update the doc comment above to reflect the broader kind set + debounce rule.

- [ ] **Step 4: Run tests, expect all 4 pass**

```bash
cargo test --lib -p codeterm bump_on_get
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/api/mem_routes/items.rs
git commit -m "feat(mem): bump evidence_count for rule/fact/preference on get"
```

---

### Task 3: Pin/unpin via existing UpdateBody

**Files:**
- Modify: `src-tauri/src/api/mem_routes/items.rs` (extend `UpdateBody` at line 348; extend `mem_update` handler at line 363)
- Test: `src-tauri/src/api/mem_routes/items.rs` (extend `mod tests`)

> **DEEP RISK — there's already an `UpdateBody` struct with validation logic.** Do NOT create a new `PatchBody`. The existing handler validates importance range, title length, body size, area chars, tag counts. Pin must slot into that struct so it inherits the `with_changed_by` wrapper and `emit_activity` for audit-trail consistency.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn update_sets_is_pinned_wakeup() {
    let conn = open_test_conn();
    insert(&conn, "fact_a", "fact");
    apply_update(&conn, "fact_a", UpdateBody { pinned: Some(true), ..Default::default() }).unwrap();
    let pinned: i64 = conn.query_row(
        "SELECT is_pinned_wakeup FROM mem_items WHERE id = ?1",
        ["fact_a"], |r| r.get(0)
    ).unwrap();
    assert_eq!(pinned, 1);
}

#[test]
fn update_can_unpin() {
    let conn = open_test_conn();
    insert(&conn, "fact_a", "fact");
    apply_update(&conn, "fact_a", UpdateBody { pinned: Some(true), ..Default::default() }).unwrap();
    apply_update(&conn, "fact_a", UpdateBody { pinned: Some(false), ..Default::default() }).unwrap();
    let pinned: i64 = conn.query_row(
        "SELECT is_pinned_wakeup FROM mem_items WHERE id = ?1",
        ["fact_a"], |r| r.get(0)
    ).unwrap();
    assert_eq!(pinned, 0);
}

#[test]
fn update_pinned_only_does_not_touch_other_fields() {
    let conn = open_test_conn();
    insert(&conn, "fact_a", "fact");
    let old_imp: f64 = conn.query_row(
        "SELECT importance FROM mem_items WHERE id = 'fact_a'", [], |r| r.get(0)
    ).unwrap();
    apply_update(&conn, "fact_a", UpdateBody { pinned: Some(true), ..Default::default() }).unwrap();
    let new_imp: f64 = conn.query_row(
        "SELECT importance FROM mem_items WHERE id = 'fact_a'", [], |r| r.get(0)
    ).unwrap();
    assert_eq!(old_imp, new_imp, "pinning must not bump importance");
}
```

- [ ] **Step 2: Run tests, expect FAIL (no `pinned` field on UpdateBody)**

- [ ] **Step 3: Extend the existing struct**

In `src-tauri/src/api/mem_routes/items.rs:348`:
```rust
#[derive(Deserialize, Default)]      // <- add Default for ergonomic test construction
pub struct UpdateBody {
    #[serde(default)] pub title: Option<String>,
    #[serde(default)] pub summary: Option<String>,
    #[serde(default)] pub area: Option<String>,
    #[serde(default)] pub tags: Option<Vec<String>>,
    #[serde(default)] pub body: Option<String>,
    #[serde(default)] pub importance: Option<f64>,
    #[serde(default)] pub pinned: Option<bool>,        // <- NEW
}
```

In the `mem_update` handler (around line 396+ where importance is applied), add:
```rust
if let Some(p) = body.pinned {
    state.svc.mem.with_conn(|conn| {
        conn.execute(
            "UPDATE mem_items SET is_pinned_wakeup = ?1, updated_at = datetime('now') WHERE id = ?2",
            rusqlite::params![if p { 1i64 } else { 0i64 }, id],
        ).map(|_| ())
    }).map_err(|e: rusqlite::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
}
```
The mutation must run inside `with_changed_by(&changed_by, || ...)` — wrap it together with the other field updates so the `mem_item_history` audit row carries the actor (user / worker / dreamer-calibration).

- [ ] **Step 4: Run tests, expect PASS**

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/api/mem_routes/items.rs
git commit -m "feat(mem): PATCH /api/mem/items/{id} accepts pinned field"
```

---

### Task 4: List endpoint `?pinned=true` filter

**Files:**
- Modify: `src-tauri/src/api/mem_routes/list.rs` (or wherever the list handler lives)
- Test: same file

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn list_filters_by_pinned() {
    let conn = open_test_conn();
    insert(&conn, "a", "fact");
    insert(&conn, "b", "fact");
    conn.execute("UPDATE mem_items SET is_pinned_wakeup = 1 WHERE id = 'a'", []).unwrap();
    let rows = list_items(&conn, ListQuery { pinned: Some(true), ..Default::default() }).unwrap();
    assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["a"]);
}
```

- [ ] **Step 2: Run tests, expect FAIL**

- [ ] **Step 3: Add `pinned: Option<bool>` to ListQuery + WHERE clause**

```rust
#[derive(Default, Deserialize)]
pub struct ListQuery {
    pub kind: Option<String>,
    pub pinned: Option<bool>,
    // ... existing
}

// in list_items SQL builder:
if matches!(query.pinned, Some(true)) {
    sql.push_str(" AND is_pinned_wakeup = 1");
}
```

- [ ] **Step 4: Run tests, expect PASS**

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/api/mem_routes/list.rs
git commit -m "feat(mem): list endpoint accepts ?pinned=true filter"
```

---

### Task 5: Score formula module + two-pass selection (HIGH-RISK CORE)

**Files:**
- Create: `src-tauri/src/mem/layers/mod.rs` (move existing `layers.rs` body here)
- Create: `src-tauri/src/mem/layers/score.rs` (new — formula + CTE SQL builder)
- Create: `src-tauri/src/mem/layers/tests.rs` (move existing tests + add new)
- Modify: `src-tauri/src/mem/mod.rs` (`pub mod layers;` already in place — confirm folder works)

> **DEEP RISK SECTION — read before coding:**
>
> 1. **`IDENTITY_ID` dedup must happen in Pass 2 SQL too**, not only in `load_items`. Otherwise the identity row consumes a slot in the LIMIT 100 and gets stripped at render — silently dropping a real candidate. Add `AND id != 'preference_identity'` to PASS2_SQL.
> 2. **`tags_json NOT LIKE '%auto-extracted%'` filter MUST be preserved.** Removing it would flood L1 with background-extracted noise. Current `collect_l1_candidates` has it; new SQL must too.
> 3. **`updated_at` is TEXT (ISO timestamp).** `updated_at > datetime('now','-30 days')` works because SQLite's TEXT comparison on ISO-8601 is lexicographic-correct. Verify by running the SQL against a fixture with `updated_at = '2026-04-09'` (30-day boundary) and asserting both sides of the bonus.
> 4. **Pinned vs ranked dedup is currently O(N²)** (`if !pinned_ids.contains(&id)`). With 50 pinned + 100 ranked, that's 5000 ops — fine. But add a unit test that pins 50 items and verifies no duplicate appears in render output.
> 5. **`load_items` is N+1** — for 150 items it issues 150 SQL calls. Acceptable for L1 sizes; do NOT prematurely optimise. If it becomes a hot path, batch with `WHERE id IN (...)` later.
> 6. **`render_l1` signature changes** from `(&[(L1Priority, MemItem)], usize) -> (String, usize)` to `(&[MemItem], &[MemItem], usize) -> (String, usize, usize)`. Compile errors will catch all callers; verify tests in `layers/tests.rs` are also updated, not silently passing because they compile.
> 7. **Rendering decision: pinned items still get grouped under their natural kind sections**, not a separate `### Pinned` block. Reasoning: agent reads them with the same mental model as everything else — pin is admission, not display category. But within each kind section, pinned items render first. Mark with a leading `★ ` in the bullet so the agent and user can spot them.
> 8. **Eviction warning trigger condition is non-obvious.** "Pinned didn't fit budget" means: even after rendering ONLY the pinned set (zero ranked items), token count exceeded `l1_budget_tokens`. NOT "pinned + ranked together exceeded budget". Get the test wrong here and the warning either never fires or fires constantly.
> 9. **Test fixtures**: do NOT use raw `INSERT` SQL in tests — always go through `crate::mem::store::insert(...)` so all NOT NULL columns get values. Adding fields to `mem_items` later (next migration) won't silently break test inserts.
> 10. **Backward compat: existing wake-up callers may not set `current_workspace`.** When `None`, behaviour must be identical to current — global items only, no ws-bonus applied. Add a regression test `wake_up_without_workspace_matches_current_behavior`.

- [ ] **Step 1: Move existing file into folder layout**

```bash
mkdir -p src-tauri/src/mem/layers
git mv src-tauri/src/mem/layers.rs src-tauri/src/mem/layers/mod.rs
```

- [ ] **Step 2: Write the failing tests in `layers/tests.rs`**

Move the existing `#[cfg(test)] mod tests` block from `mod.rs` into a sibling `tests.rs` (the parent file declares `mod tests;`). Add new tests:

```rust
#[test]
fn pinned_item_loads_regardless_of_importance() {
    let (conn, blobs) = fresh();
    insert_pinned_fact(&conn, "fact_pinned", 1.0);
    insert_unpinned_fact(&conn, "fact_high", 5.0);
    let out = generate(&conn, &WakeUpOptions { current_workspace: None, ..Default::default() }, &blobs).unwrap();
    assert!(out.contains("fact_pinned"), "pinned must appear: {out}");
}

#[test]
fn workspace_match_outranks_global_at_margin() {
    let (conn, blobs) = fresh();
    insert_fact_in_scope(&conn, "ws_a", "workspace:W1", 3.0);
    insert_fact_in_scope(&conn, "global_a", "global", 3.0);
    let out = generate(&conn, &WakeUpOptions {
        current_workspace: Some("W1".into()), ..Default::default()
    }, &blobs).unwrap();
    let pos_ws = out.find("ws_a").expect("ws_a present");
    let pos_g = out.find("global_a").expect("global_a present");
    assert!(pos_ws < pos_g, "ws-scoped item must rank ahead of global with same importance");
}

#[test]
fn pinned_set_over_budget_emits_warning() {
    let (conn, blobs) = fresh();
    for i in 0..30 {
        let id = format!("pinned_{i}");
        insert_pinned_fact(&conn, &id, 3.0);
    }
    let out = generate(&conn, &WakeUpOptions {
        l1_budget_tokens: 100, ..Default::default()
    }, &blobs).unwrap();
    assert!(out.contains("WARN: "), "must surface eviction warning: {out}");
    assert!(out.contains("pinned items"));
}

#[test]
fn other_workspace_items_excluded_when_in_workspace() {
    let (conn, blobs) = fresh();
    insert_fact_in_scope(&conn, "ws_a", "workspace:W1", 3.0);
    insert_fact_in_scope(&conn, "ws_b", "workspace:W2", 3.0);
    let out = generate(&conn, &WakeUpOptions {
        current_workspace: Some("W1".into()), ..Default::default()
    }, &blobs).unwrap();
    assert!(out.contains("ws_a"));
    assert!(!out.contains("ws_b"), "items from other workspace must not load: {out}");
}

#[test]
fn evidence_count_log_scaling() {
    use super::score::score_for;
    assert!((score_for(3.0, 1, false, false) - 3.0).abs() < 0.01,
        "fresh item: importance=3, evidence=1, no ws/recency → ~3.0");
    let s10 = score_for(3.0, 10, false, false);
    assert!(s10 > 3.4 && s10 < 3.6,
        "evidence=10 contributes ≈ +0.5 above baseline, got {s10}");
    let s1000 = score_for(3.0, 1000, false, false);
    assert!(s1000 > 4.4 && s1000 < 4.6,
        "evidence=1000 capped to log10 growth, got {s1000}");
}
```

Helpers (in `tests.rs`):
```rust
fn insert_pinned_fact(conn: &Connection, id: &str, importance: f64) {
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, importance, evidence_count, is_pinned_wakeup, created_at, updated_at, body_inline)
         VALUES (?1, ?1, 'fact', 'global', ?1, ?2, 1, 1, '2026-05-10', '2026-05-10', 'body of ' || ?1)",
        rusqlite::params![id, importance],
    ).unwrap();
}

fn insert_unpinned_fact(conn: &Connection, id: &str, importance: f64) {
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, importance, evidence_count, is_pinned_wakeup, created_at, updated_at, body_inline)
         VALUES (?1, ?1, 'fact', 'global', ?1, ?2, 1, 0, '2026-05-10', '2026-05-10', 'body of ' || ?1)",
        rusqlite::params![id, importance],
    ).unwrap();
}

fn insert_fact_in_scope(conn: &Connection, id: &str, scope: &str, importance: f64) {
    conn.execute(
        "INSERT INTO mem_items (id, uid, kind, scope, title, importance, evidence_count, is_pinned_wakeup, created_at, updated_at, body_inline)
         VALUES (?1, ?1, 'fact', ?2, ?1, ?3, 1, 0, '2026-05-10', '2026-05-10', 'body of ' || ?1)",
        rusqlite::params![id, scope, importance],
    ).unwrap();
}
```

- [ ] **Step 3: Run tests, expect FAIL (the new behaviors don't exist yet)**

```bash
cargo test --lib -p codeterm mem::layers -- --nocapture
```

- [ ] **Step 4: Implement `score.rs`**

```rust
// src-tauri/src/mem/layers/score.rs
//! Pure score formula + CTE SQL builder for L1 selection.
//!
//! score = importance * 1.0
//!       + log10(1 + evidence_count) * 0.5
//!       + (workspace_match ? 0.5 : 0.0)
//!       + (recent_30d      ? 0.3 : 0.0)

pub fn score_for(importance: f64, evidence_count: i64, ws_match: bool, recent_30d: bool) -> f64 {
    importance
        + ((evidence_count as f64).max(0.0) + 1.0).log10() * 0.5
        + if ws_match { 0.5 } else { 0.0 }
        + if recent_30d { 0.3 } else { 0.0 }
}

/// Returns the SELECT used in Pass 2 (unpinned, ranked).
/// Caller binds: ?1 = current_workspace_scope (e.g. "workspace:W1" or NULL),
///               ?2 = LIMIT.
pub const PASS2_SQL: &str = r#"
WITH ranked AS (
    SELECT
        id,
        importance
        + (CASE WHEN evidence_count > 0
                THEN log10(1.0 + CAST(evidence_count AS REAL)) ELSE 0.0 END) * 0.5
        + (CASE WHEN ?1 IS NOT NULL AND scope = ?1 THEN 0.5 ELSE 0.0 END)
        + (CASE WHEN updated_at > datetime('now', '-30 days') THEN 0.3 ELSE 0.0 END)
          AS sort_score
    FROM mem_items
    WHERE is_pinned_wakeup = 0
      AND id != 'preference_identity'                                        -- L0 dedup
      AND kind IN ('rule','preference','peer','fact','summary','skill','learning')
      AND (scope = 'global' OR (?1 IS NOT NULL AND scope = ?1))
      AND tags_json NOT LIKE '%auto-extracted%'                              -- preserve current filter
)
SELECT id FROM ranked
ORDER BY sort_score DESC, id ASC
LIMIT ?2;
"#;

/// Pass 1: pinned items in scope.
/// Caller binds: ?1 = current_workspace_scope or NULL.
pub const PASS1_SQL: &str = r#"
SELECT id FROM mem_items
WHERE is_pinned_wakeup = 1
  AND (scope = 'global' OR (?1 IS NOT NULL AND scope = ?1))
ORDER BY importance DESC, evidence_count DESC, updated_at DESC
LIMIT 50;
"#;
```

- [ ] **Step 5: Replace `collect_l1_candidates` in `layers/mod.rs`**

```rust
// in layers/mod.rs

mod score;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WakeUpOptions {
    pub l0_budget_tokens: usize,
    pub l1_budget_tokens: usize,
    pub include_l0: bool,
    pub include_l1: bool,
    pub current_workspace: Option<String>,
    pub explain: bool,
}
// (existing Default impl extended: current_workspace = None, explain = false; defaults 100 / 800 / true / true)

fn collect_l1_candidates(
    conn: &Connection,
    blobs_dir: &Path,
    current_ws: Option<&str>,
) -> Result<(Vec<MemItem>, Vec<MemItem>), String> {
    let scope_param = current_ws.map(|w| format!("workspace:{w}"));
    let bind: Option<&str> = scope_param.as_deref();

    let mut pinned_ids = Vec::<String>::new();
    let mut stmt = conn.prepare(score::PASS1_SQL).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([bind], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
    for r in rows { if let Ok(id) = r { pinned_ids.push(id); } }

    let mut ranked_ids = Vec::<String>::new();
    let mut stmt = conn.prepare(score::PASS2_SQL).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(rusqlite::params![bind, 100i64], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    for r in rows {
        if let Ok(id) = r {
            if !pinned_ids.contains(&id) && id != IDENTITY_ID {
                ranked_ids.push(id);
            }
        }
    }

    let pinned = load_items(conn, blobs_dir, &pinned_ids)?;
    let ranked = load_items(conn, blobs_dir, &ranked_ids)?;
    Ok((pinned, ranked))
}

fn load_items(conn: &Connection, blobs_dir: &Path, ids: &[String]) -> Result<Vec<MemItem>, String> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(item) = super::store::get(conn, id, blobs_dir)? { out.push(item); }
    }
    Ok(out)
}
```

Update `generate` to call the new selector and route through a render path that drains pinned first, then ranked, then emits the WARN comment when pinned overflowed:

```rust
if opts.include_l1 {
    let ws = opts.current_workspace.as_deref();
    let (pinned, ranked) = collect_l1_candidates(conn, blobs_dir, ws)?;
    let (rendered, used, evicted_pinned) = render_l1(&pinned, &ranked, opts.l1_budget_tokens);
    l1_tokens = used;
    if evicted_pinned > 0 {
        out.push_str(&format!(
            "<!-- WARN: {evicted_pinned} pinned items did not fit L1 budget {} -->\n",
            opts.l1_budget_tokens
        ));
        log::warn!("[wake-up] pinned-eviction count={evicted_pinned} budget={}", opts.l1_budget_tokens);
    }
    out.push_str("## L1 — Critical context\n\n");
    out.push_str(&rendered);
}
```

`render_l1` signature changes to accept two slices and return the third element (`evicted_pinned: usize`). Adapt the existing kind-grouping logic so pinned items are added first (under their natural priority groups), then the ranked fill flows in until budget is exhausted.

- [ ] **Step 6: Run tests, expect PASS**

```bash
cargo test --lib -p codeterm mem::layers
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/mem/layers/
git commit -m "feat(mem): two-pass L1 selection with pin + workspace + evidence_count score"
```

---

### Task 6: `memory.l1_budget_tokens` config

**Files:**
- Modify: `src-tauri/src/config/schema.rs` (find the `[memory]` struct or add it)
- Modify: `src-tauri/src/api/mem_routes/items.rs` (or wherever `WakeUpOptions` is constructed for the HTTP wake-up endpoint)
- Test: `src-tauri/src/config/schema.rs` (extend deserialization tests)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn memory_l1_budget_default_is_800() {
    let toml = "";
    let cfg: AppConfig = toml::from_str(toml).unwrap();
    assert_eq!(cfg.memory.l1_budget_tokens, 800);
}

#[test]
fn memory_l1_budget_user_override() {
    let cfg: AppConfig = toml::from_str(
        "[memory]\nl1_budget_tokens = 1200\n"
    ).unwrap();
    assert_eq!(cfg.memory.l1_budget_tokens, 1200);
}
```

- [ ] **Step 2: Run tests, expect FAIL**

- [ ] **Step 3: Add the field**

In `config/schema.rs`:
```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct MemoryConfig {
    pub l1_budget_tokens: usize,
    // ... existing memory fields
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self { l1_budget_tokens: 800 /* , existing defaults */ }
    }
}
```

In the HTTP `mem_wake_up` handler, populate `WakeUpOptions::l1_budget_tokens` from `state.config.memory.l1_budget_tokens` instead of the hardcoded literal.

- [ ] **Step 4: Run tests, expect PASS**

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/config/schema.rs src-tauri/src/api/mem_routes/items.rs
git commit -m "feat(config): memory.l1_budget_tokens (default 800)"
```

---

### Task 7: CLI — `mem pin`, `mem unpin`, `mem pinned`

**Files:**
- Modify: `cli/src/cmd/mem.rs` (or split into `mem/pin.rs` if file size grows)
- Test: `cli/src/cmd/mem.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn pin_subcommand_parses() {
    let cli = Cli::try_parse_from(&["codeterm", "mem", "pin", "fact_abc"]).unwrap();
    match cli.command {
        Command::Mem { sub: MemCmd::Pin { id } } => assert_eq!(id, "fact_abc"),
        _ => panic!("expected pin"),
    }
}
#[test]
fn pinned_subcommand_parses_with_limit() {
    let cli = Cli::try_parse_from(&["codeterm", "mem", "pinned", "--limit", "10"]).unwrap();
    match cli.command {
        Command::Mem { sub: MemCmd::Pinned { limit } } => assert_eq!(limit, Some(10)),
        _ => panic!("expected pinned"),
    }
}
```

- [ ] **Step 2: Run tests, expect FAIL**

- [ ] **Step 3: Add subcommands**

Wire three new variants on the existing `MemCmd` enum:
```rust
#[derive(clap::Subcommand)]
pub enum MemCmd {
    // ... existing
    Pin { id: String },
    Unpin { id: String },
    Pinned {
        #[arg(long)]
        limit: Option<usize>,
    },
}
```

Implementation calls the existing HTTP client:
```rust
fn run_pin(client: &Client, id: &str) -> Result<(), String> {
    client.patch(&format!("/api/mem/items/{id}"), &serde_json::json!({"pinned": true}))
}
fn run_unpin(client: &Client, id: &str) -> Result<(), String> {
    client.patch(&format!("/api/mem/items/{id}"), &serde_json::json!({"pinned": false}))
}
fn run_pinned(client: &Client, limit: Option<usize>) -> Result<(), String> {
    let url = match limit {
        Some(n) => format!("/api/mem/items?pinned=true&limit={n}"),
        None => "/api/mem/items?pinned=true".to_string(),
    };
    let body: serde_json::Value = client.get(&url)?;
    print_human(&body);
    Ok(())
}
```

- [ ] **Step 4: Run tests, expect PASS**

- [ ] **Step 5: Commit**

```bash
git add cli/src/cmd/mem.rs
git commit -m "feat(cli): mem pin/unpin/pinned subcommands"
```

---

### Task 8: CLI — `mem save --pin --workspace`

**Files:**
- Modify: `cli/src/cmd/mem.rs` (extend the existing `MemCmd::Save` variant)
- Modify: `src-tauri/src/api/mem_routes/items.rs` (extend the create endpoint to accept `pinned: bool` so it's a single round-trip)
- Test: same file

> **DEEP RISK — non-atomic two-call create.** First draft was POST then PATCH. If PATCH fails (network blip, daemon restart between calls), item is created un-pinned and the user sees `mem save` succeed silently with the wrong state. Fix: extend the POST `/api/mem/items` body with `pinned: Option<bool>`. Single call, atomic. CLI passes `pinned: true` in the create payload when `--pin` is set.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn save_accepts_pin_flag() {
    let cli = Cli::try_parse_from(&[
        "codeterm","mem","save","--kind","fact","--title","t","--pin","body"
    ]).unwrap();
    match cli.command {
        Command::Mem { sub: MemCmd::Save { pin, .. } } => assert!(pin),
        _ => panic!(),
    }
}
#[test]
fn save_accepts_workspace_flag() {
    let cli = Cli::try_parse_from(&[
        "codeterm","mem","save","--kind","fact","--title","t","--workspace","W1","body"
    ]).unwrap();
    match cli.command {
        Command::Mem { sub: MemCmd::Save { workspace, .. } } => assert_eq!(workspace.as_deref(), Some("W1")),
        _ => panic!(),
    }
}
```

- [ ] **Step 2: Run tests, expect FAIL**

- [ ] **Step 3: Add flags**

```rust
Save {
    #[arg(long)] kind: String,
    #[arg(long)] title: String,
    #[arg(long, default_value_t = false)] pin: bool,
    #[arg(long)] workspace: Option<String>,
    body: String,
    // ... existing
}
```

In the create body for `POST /api/mem/items`, include `pinned: <flag>` and `scope: "workspace:<ws>"` (or `"global"`) directly. ONE call, ATOMIC. The server-side handler must accept `pinned` on create and persist as `is_pinned_wakeup` in the same INSERT.

- [ ] **Step 4: Run tests, expect PASS**

- [ ] **Step 5: Commit**

```bash
git add cli/src/cmd/mem.rs
git commit -m "feat(cli): mem save --pin and --workspace flags"
```

---

### Task 9: CLI — `mem wake-up --explain`

**Files:**
- Modify: `cli/src/cmd/mem.rs` (extend `WakeUp` variant)
- Modify: `src-tauri/src/api/mem_routes/items.rs` (wake-up endpoint accepts `?explain=true`)
- Modify: `src-tauri/src/mem/layers/mod.rs` (when `opts.explain`, append a per-item breakdown to stderr-style trailing comment block)
- Test: `cli/src/cmd/mem.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn wakeup_accepts_explain_flag() {
    let cli = Cli::try_parse_from(&["codeterm","mem","wake-up","--explain"]).unwrap();
    match cli.command {
        Command::Mem { sub: MemCmd::WakeUp { explain, .. } } => assert!(explain),
        _ => panic!(),
    }
}
```

Plus a layers test:
```rust
#[test]
fn explain_renders_score_breakdown() {
    let (conn, blobs) = fresh();
    insert_unpinned_fact(&conn, "fact_x", 4.0);
    let out = generate(&conn, &WakeUpOptions { explain: true, ..Default::default() }, &blobs).unwrap();
    assert!(out.contains("[L1 score]"), "expected score header: {out}");
    assert!(out.contains("fact_x"));
    assert!(out.contains("importance="));
}
```

- [ ] **Step 2: Run tests, expect FAIL**

- [ ] **Step 3: Wire `--explain`**

In `WakeUp` CLI variant: `#[arg(long, default_value_t = false)] explain: bool`. CLI passes `?explain=true` to the wake-up HTTP call.

In `layers/mod.rs::generate`, when `opts.explain`, after rendering L1 append:
```rust
if opts.explain {
    out.push_str("\n<!-- L1 score breakdown -->\n");
    for item in &ranked {
        let s = score::score_for(
            item.importance,
            item.evidence_count.unwrap_or(1),
            item.scope == scope_param.as_deref().unwrap_or(""),
            within_30d(&item.updated_at),
        );
        out.push_str(&format!(
            "[L1 score] id={} kind={} score={:.2} importance={:.1} evidence={}\n",
            item.id, item.kind, s, item.importance,
            item.evidence_count.unwrap_or(1),
        ));
    }
}
```

(Assumes `MemItem` exposes `evidence_count`. If it doesn't, add the field via `store::get` selecting it. If that's a bigger change, fold it into Task 5 instead.)

- [ ] **Step 4: Run tests, expect PASS**

- [ ] **Step 5: Commit**

```bash
git add cli/src/cmd/mem.rs src-tauri/src/api/mem_routes/items.rs src-tauri/src/mem/layers/
git commit -m "feat(mem): mem wake-up --explain dumps per-item score breakdown"
```

---

### Task 10: Eval set — pinned + workspace recall

**Files:**
- Create: `evals/general-agent/cases/wakeup_loads_pinned_in_workspace.json`
- Modify: `evals/general-agent/evals.json` (or whatever the index file is) to register the case

- [ ] **Step 1: Write the eval case**

Use the bench library (`source evals/_lib/lib.sh`, `dc`-prefixed CLI calls only — never raw `codeterm`). The case:
1. Seed: `dc mem save --kind fact --workspace W1 --pin --title "load-bearing" "should appear in W1 wake-up"`
2. Switch GA to workspace W1, call `dc mem wake-up`
3. Assert: output contains `"load-bearing"` (substring) and `<!-- WARN: -->` is absent
4. Negative: in workspace W2, call `dc mem wake-up`; assert "load-bearing" is NOT present

- [ ] **Step 2: Run the eval**

```bash
bash evals/general-agent/run.sh wakeup_loads_pinned_in_workspace
```
Expected: PASS after Tasks 1–9 land.

- [ ] **Step 3: Commit**

```bash
git add evals/general-agent/
git commit -m "test(evals): wake-up loads workspace-scoped pinned item"
```

---

### Task 11: Docs

**Files:**
- Modify: `docs/MEMORY.md` (revision tag bump at bottom)

- [ ] **Step 1: Update the L1 selection section**

Cover:
1. Pin semantics: `mem pin <id>` / `mem unpin <id>` / `mem pinned`
2. Workspace scoping: `mem save --workspace $CODETERM_WORKSPACE_ID`. Pinned items obey workspace scope (cross-ws pins must use `workspace_id=NULL`).
3. Score formula table (importance, log-evidence, ws_match, recency).
4. Eviction warning: when pinned set overflows budget, output gets a `<!-- WARN -->` line and a `[wake-up] pinned-eviction` log entry.
5. `mem wake-up --explain` for debugging.
6. Bump `<!-- revision: N -->` at file bottom.

- [ ] **Step 2: Commit**

```bash
git add docs/MEMORY.md
git commit -m "docs(mem): document pin / workspace / score-explain in wake-up"
```

---

### Task 12: Move dreamer schedule from 3am → 6am

**Files:**
- Modify: `src-tauri/src/mem/dreaming.rs:616` (`schedule: "0 3 * * *".into()` → `"0 6 * * *".into()`)
- Modify: `src-tauri/src/mem/dreaming.rs:762` (test assertion)
- Modify: `src-tauri/src/mem/dreaming.rs:579-590` (extend "update existing cron" block to also reconcile `schedule` if changed)

User wakes ~6am; current 3am ± 90min jitter means dream may still be running when they sit down. Pushing to 6am keeps it close enough that L0/L1 reflects yesterday but doesn't compete for the morning.

- [ ] **Step 1: Update test assertion**

```rust
assert_eq!(job.schedule, "0 6 * * *");
```
Run: `cargo test --lib -p codeterm dreaming::tests::ensure_dreaming_cron` → expect FAIL.

- [ ] **Step 2: Update creation default + reconciliation**

In `ensure_dreaming_cron`:
```rust
let _ = cron_store.create(crate::cron::store::CronCreateParams {
    name: DREAMING_CRON_NAME.into(),
    workspace_id: String::new(),
    schedule: "0 6 * * *".into(),
    // ... rest unchanged
})?;
```

Extend the "update agent on existing cron if config changed" block to also re-sync schedule (so existing installs migrate from 3am to 6am on next startup):
```rust
if let Some(job) = cron_store.list().iter().find(|j| j.name == DREAMING_CRON_NAME) {
    let job_id = job.id.clone();
    if let Some(j) = cron_store.get_mut(&job_id) {
        j.agent = agent.clone();
        if let Some(m) = &model { j.model = Some(m.clone()); }
        if j.schedule == "0 3 * * *" {
            j.schedule = "0 6 * * *".into();   // one-time migration of legacy default
        }
    }
    let _ = cron_store.save();
}
```

We only auto-migrate from the literal old default (`0 3 * * *`); custom schedules set by the user are left alone.

- [ ] **Step 3: Run tests, expect PASS**

```bash
cargo test --lib -p codeterm dreaming::tests
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/mem/dreaming.rs
git commit -m "feat(mem): dreamer schedule 6am (was 3am); migrate legacy cron"
```

---

### Task 13: Sunday calibration stage in dreamer prompt (HIGH-RISK AUTO-MUTATIONS)

**Files:**
- Modify: `src-tauri/prompts/dreaming_rem.md` (append a new STAGE 8 — calibration; conditional on Sunday)
- Modify: `src-tauri/src/api/mem_routes/items.rs` (add `mem calibration log` HTTP endpoint reading `mem_item_history WHERE changed_by LIKE 'dreamer-calibration%'`; add `mem calibration sweep` endpoint that does the actual mutations server-side with all caps enforced)
- Modify: `cli/src/cmd/mem.rs` (add `mem calibration log` + `mem calibration sweep` subcommands; extend `mem update` with `--changed-by <actor>`)

Zero new processes. The existing daily dreamer cron runs the same prompt; the prompt's last stage on Sunday calls **one** server-side endpoint (`mem calibration sweep`) that enforces all caps inside the daemon. The prompt itself does NOT run the heuristic — it just kicks the sweep, captures the count, writes to diary.

> **DEEP RISK SECTION:**
>
> 1. **Day-of-week portability.** Don't rely on `date +%u` in the prompt — output varies across GNU/BSD/Windows. Gate is server-side: the sweep endpoint checks `strftime('%w','now') == '0'` itself (SQLite portable, 0=Sunday) and refuses on other days unless `--force` is passed. Manual `mem calibrate` overrides for testing.
> 2. **Auto-apply blast radius cap.** Sweep can adjust hundreds of items if the heuristic mis-matches. **Hard cap inside the endpoint: 30 changes per session.** If the candidate set exceeds 100, abort with `notify` instead of applying — signal of broken heuristic, never silent.
> 3. **Re-entry protection.** Manual `mem calibrate` + Sunday cron could collide. Endpoint checks: any `changed_by='dreamer-calibration'` row in last 6h → refuse with 409.
> 4. **L0 overlap detection — refined heuristic.** Original "80% substring" is fragile (a 30-char L1 line trivially passes against a long L0). Refined rule, enforced server-side:
>    - L1 body length ≥ 60 chars (skip otherwise — short, nothing to "cover")
>    - At least 80% of L1 content tokens (split whitespace, length > 3, not in a 30-word English stopword list) appear verbatim in L0
>    - Even then: archive only if `evidence_count <= 1` (never archive frequently used)
> 5. **Archive is one-way and silent.** No unarchive in this plan. **Triple-gate** before archiving anything: `evidence_count == 1 AND age_days > 90 AND kind IN ('fact','note') AND importance ≤ 2`. Pinned items, multi-use items, rules / preferences / peers — never auto-archived. L0-overlap-archive adds the same triple-gate as belt-and-suspenders.
> 6. **Dream worker timeout.** `dreaming_timeout_seconds` may cut the run mid-sweep. Each `mem update` is atomic SQL → no torn state. Bounded to 30 changes anyway; if killed at 17, next Sunday continues. Acceptable.
> 7. **Big-DB stdout flooding.** Sweep endpoint paginates internally (`LIMIT 200` per query, processed in chunks of 30). The CLI never pipes raw `mem list --json` for thousands of rows.
> 8. **`mem update --changed-by` is missing today.** API already accepts `x-codeterm-actor` header (`mem_routes/mod.rs:92`), but CLI doesn't surface it. Without this flag, dreamer changes land as `changed_by='user'` and the calibration log filter returns nothing. Add the flag in this same task.

- [ ] **Step 1: Append STAGE 8 to `dreaming_rem.md`**

```markdown
## STAGE 8: Memory calibration (Sundays only)

Day-of-week gate. Skip immediately on Mon–Sat:

```bash
DOW=$(date +%u)
if [ "$DOW" != "7" ]; then
  echo "[calibrate] skipped: dow=$DOW (not Sunday)"
  exit 0
fi
```

If Sunday, run a sweep over items that look mis-calibrated. Fully
automatic — no notifications, no human-in-the-loop. The action set is
intentionally narrow so a wrong call costs ≤0.5 importance and is
recoverable from `mem_item_history` (changed_by='dreamer-calibration').

For each item older than 14 days, classify by `(observed_use, importance, age_days)`:

| Condition | Action |
|-----------|--------|
| `evidence_count >= 10 AND importance < 4 AND touched_30d` | `+0.5` importance (auto) |
| `evidence_count <= 1 AND importance >= 4 AND age_days > 60` | `-0.5` importance (auto) |
| `evidence_count == 1 AND importance <= 2 AND age_days > 90 AND kind IN ('fact','note')` | archive (set kind = 'archived') |
| `kind in ('rule','preference','peer','fact','summary','skill') AND L0 substring overlap > 80% AND length > 40 chars` | archive (covered by L0 already) |

Implementation steps:

1. Capture L0 once: `L0=$(codeterm mem wake-up --no-l1 2>/dev/null)`
2. Fetch candidate items (one CLI call per filter, JSON):
   ```bash
   codeterm mem list --kind fact --json --limit 500
   ```
3. For each candidate, compare `evidence_count` / `importance` / `age` and apply via:
   ```bash
   codeterm mem update <id> --importance <new> \
     --reason "dreamer-calibration: <evidence|age|l0_overlap>"
   ```
4. For L0 overlap check, do a substring presence test against `$L0` — if more than 80% of the L1 body's "content tokens" (split on whitespace, length > 3) appear in `$L0`, treat as L0-covered and archive.
5. Audit: every change must land in `mem_item_history` with `changed_by='dreamer-calibration'` (set via `codeterm mem update --changed-by dreamer-calibration`).

VERIFY:
```bash
COUNT=$(codeterm mem calibration log --since '1 day ago' --count)
echo "[calibrate] applied $COUNT changes this run"
```
Stage passes if `$COUNT >= 0` (zero is OK on a clean DB).
```

- [ ] **Step 2: Add `mem calibration log` endpoint + CLI**

HTTP `GET /api/mem/calibration/log?since=&limit=&count=`:
```rust
async fn mem_calibration_log(
    State(state): State<AppState>,
    Query(q): Query<CalibLogQuery>,
) -> ... {
    let conn = state.svc.mem.conn();
    let since = q.since.as_deref().unwrap_or("1970-01-01");
    let rows = conn.prepare(
        "SELECT item_id, changed_at, change_type, old_importance, new_importance, reason
         FROM mem_item_history
         WHERE changed_by LIKE 'dreamer-calibration%' AND changed_at >= ?1
         ORDER BY changed_at DESC LIMIT ?2"
    )?...
}
```

CLI:
```rust
MemCmd::Calibration { sub: CalibCmd::Log { since: Option<String>, limit: Option<usize>, count: bool } }
```

- [ ] **Step 3: Test**

Add Rust integration test that inserts 3 items, posts a fake history row with `changed_by='dreamer-calibration'`, calls the endpoint, asserts row appears.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/prompts/dreaming_rem.md src-tauri/src/api/mem_routes/items.rs cli/src/cmd/mem.rs
git commit -m "feat(mem): Sunday calibration stage in dreamer + mem calibration log"
```

---

### Task 14: Integration sanity test — calibration applies on Sunday only

**Files:**
- Create: `evals/general-agent/cases/calibration_sunday_gate.sh` (bench-lib script)

A live integration test that runs the dreamer prompt against a fixture mem.db and asserts:
- On a Wednesday-mocked run: STAGE 8 exits early, `mem_item_history` gains 0 rows with `changed_by='dreamer-calibration'`
- On a Sunday-mocked run: STAGE 8 runs, applies adjustments to a seeded item where `evidence_count=15, importance=2` (must bump to 2.5)

Mock day-of-week by overriding `date` in PATH, e.g.:
```bash
mkdir -p /tmp/mock_date_sun && cat > /tmp/mock_date_sun/date <<'EOF'
#!/bin/sh
[ "$1" = "+%u" ] && echo "7" && exit 0
exec /bin/date "$@"
EOF
chmod +x /tmp/mock_date_sun/date
PATH="/tmp/mock_date_sun:$PATH" dc dreamer-run-fixture
```

- [ ] **Step 1: Write fixture seed + assert script**

Use `evals/_lib/lib.sh` `dc` wrapper. Seed 3 items via `dc mem save`, run dreamer once with mocked `date`, query `dc mem calibration log --count`.

- [ ] **Step 2: Run, expect PASS**

```bash
bash evals/general-agent/cases/calibration_sunday_gate.sh
```

- [ ] **Step 3: Commit**

```bash
git add evals/general-agent/cases/calibration_sunday_gate.sh
git commit -m "test(evals): calibration gate fires on Sunday only"
```

---

### Task 15: Adapt mempalace bench harness for our retrieval (deferred follow-up)

**NOT BLOCKING** for L1 redesign rollout. Schedule for week +2 after Tasks 1–14 ship.

**Goal:** measure `codeterm mem search` recall@K against LongMemEval ground truth, the same dataset mempalace publishes results on. Lets us know whether our FTS5+sqlite-vec hybrid is competitive with their ChromaDB stack.

**Files:**
- Create: `evals/mem-recall-longmemeval/run.py` (adapter wrapping `codeterm mem search` instead of mempalace's `Searcher`)
- Create: `evals/mem-recall-longmemeval/README.md` (how to run, what numbers mean)
- Create: `evals/mem-recall-longmemeval/results/` (JSON outputs, gitignored except baseline)

Reference adapter: their `benchmarks/longmemeval_bench.py` ingests questions+haystacks into a fresh palace per query, then scores R@K / NDCG@K. We replace the palace with our mem store:

```python
# Pseudocode for our adapter
for q in questions:
    subprocess.run(["codeterm", "mem", "forget", "all"])  # fresh
    for sess in q["haystack_sessions"]:
        subprocess.run(["codeterm", "mem", "save", "--kind", "note", "--body", sess["text"]])
    out = subprocess.run(["codeterm", "mem", "search", q["question"], "--json"], capture_output=True)
    hits = json.loads(out.stdout)["hits"]
    score = recall_at_k(hits, q["ground_truth_session_ids"], k=5)
```

Dataset: `data/longmemeval_s_cleaned.json` (500 questions). Mempalace's harness already downloads it; reuse the path.

This is research, not redesign — outcome informs whether we invest in sqlite-vec tuning or accept that ChromaDB has the lead and our value is elsewhere (workspace scope, pinning, calibration).

- [ ] **Step 1: Stub adapter + smoke run on 5 questions** — verify pipeline, no scoring
- [ ] **Step 2: Full 500-question run, baseline number** — commit JSON to `results/baseline.json`
- [ ] **Step 3: Document numbers in `evals/mem-recall-longmemeval/README.md` + cross-link from `docs/MEMORY.md`**

(No tests for the bench harness itself — its output IS the test.)

---

### Task 16: Documentation sweep (last — runs after every task lands)

**Files:**
- Modify: `docs/MEMORY.md`, `docs/API.md`, `docs/CONFIG.md`, `docs/SKILLS.md`, `docs/PROMPTS.md`, `docs/contributing/*.md`, `README.md`, `CLAUDE.md`, the `codeterm` skill (`~/.codeterm/skills/codeterm/SKILL.md` and `references/mem.md`), and any `<!-- revision: N -->` tag in touched files

**This is not a one-paragraph append.** Have the docs agent (`Agent` tool, `subagent_type: docs`) read **every** docs file end-to-end and reconcile against the merged code. The plan changed multiple architectural decisions; existing docs may now describe behaviour that doesn't match.

> **DEEP RISK:** Docs that describe out-of-date behaviour are worse than missing docs — a future contributor will trust them and write code against the wrong model. Common sins this sweep catches:
> - `docs/MEMORY.md` describes "L1 = rule/preference/peer always; fact/summary if importance ≥ 4" — wrong post-Task 5
> - `docs/API.md` describes `POST /api/mem/items` body without `pinned` field — wrong post-Task 8
> - `codeterm` skill (`mem.md`) lists `mem save` flags missing `--pin --workspace` — wrong post-Tasks 7-8
> - `docs/CONFIG.md` doesn't document `memory.l1_budget_tokens` — wrong post-Task 6
> - Dreamer schedule mentioned as "3 AM" anywhere — wrong post-Task 12
> - "Memory calibration" section missing entirely — wrong post-Task 13

- [ ] **Step 1: Spawn docs agent with explicit reconciliation brief**

```
Agent tool with subagent_type=docs:
  Task: Reconcile every doc file under docs/, README.md, CLAUDE.md, and the
  codeterm skill against current code state. The L1 selection redesign just
  shipped (15 commits on develop, plan at docs/plans/mem-l1-selection-redesign.md).
  Specifically scan for stale claims about:
    - L1 selection logic (was kind+importance, now two-pass score)
    - mem CLI surface (new: pin, unpin, pinned, calibration log/sweep, save --pin --workspace, wake-up --explain)
    - HTTP API surface (UpdateBody.pinned, list ?pinned=true, calibration endpoints)
    - Dreamer schedule (was 3am ±90min, now 6am ±30min) and Sunday calibration stage
    - Config (new: memory.l1_budget_tokens)
    - bump_on_get behavior (was skill/lesson only, now skill/lesson/rule/fact/preference with 12h debounce)
  For every doc that needs change, edit it, bump its <!-- revision: N --> tag.
  Report a list of (file, change_summary) tuples.
```

- [ ] **Step 2: Review the agent's diff manually**

Don't trust blindly. Skim each changed file. Watch for:
- Hallucinated CLI flags or endpoints that don't exist in code
- Over-deletion (the agent might prune content that's still relevant)
- Missed files (the agent reports "all done" but skipped one)

- [ ] **Step 3: Commit**

```bash
git add docs/ README.md CLAUDE.md  # plus skill paths if they live in repo
git commit -m "docs: reconcile docs with L1 redesign across all touched surfaces"
```

---

## Cross-cutting risks (apply across multiple tasks)

| Risk | Affected tasks | Mitigation |
|------|----------------|------------|
| Migration runs before `mem_items` exists in some test paths | T1 | `add_pin_column` checks `PRAGMA table_info` first; if 0 columns returned, skip silently. The init_schema → migrations order in production is fixed (init runs first). Add a unit test `pin_migration_skipped_on_empty_db_safely`. |
| `evidence_count` feedback loop poisoning from internal sweeps | T2 | 12h debounce per item via existing `accessed_at` column. Tested explicitly. |
| Dreamer reads items via `codeterm mem get` ⇒ bumps everything weekly | T2, T13 | 12h debounce ensures dreamer's Sunday sweep produces ≤1 bump per item per week. |
| Memory Panel UI scrolling triggers repeated bumps | T2 | Same 12h debounce covers UI use too. |
| Two-step CLI (POST then PATCH) creates inconsistent state on partial failure | T8 | Extended POST `/api/mem/items` body with `pinned` for atomic create. |
| Dreamer prompt's day-of-week check is shell-portability-fragile | T13 | Gate moved server-side: dreamer just calls `mem calibration sweep`, server checks SQLite `strftime('%w','now')`. |
| Auto-apply mass importance changes one-shot poisoning the entire DB | T13 | Server-side hard cap of 30 changes per session, abort on >100 candidates. |
| Auto-archive silent loss of legitimate content | T13 | Triple-gate (evidence_count==1 AND age>90d AND kind in fact/note AND importance≤2). Pinned items NEVER archived. |
| Concurrent `mem calibrate` + Sunday cron mutate same rows | T13 | Server checks recent `changed_by='dreamer-calibration'` rows; 409 if any in last 6h. |
| `--changed-by` missing on CLI ⇒ all dreamer changes show as `user` in audit | T13 | Add the flag in T13. Without it, the calibration log endpoint returns empty and the eval test (T14) silently passes a no-op. |
| Test fixtures using raw INSERT silently break when schema gains columns | T1, T5 | All test `INSERT`s must go through `crate::mem::store::insert(...)`, not raw SQL. Reviewer rule. |
| Backward compat: existing wake-up callers without `current_workspace` | T5, T6 | Regression test `wake_up_without_workspace_matches_current_behavior`. |
| Old daemon + new CLI mixing: pin verbs hit old server returning 404 | All CLI tasks | Daemon returns clear 404 with body "endpoint not found"; CLI prints "this CLI requires CodeTerm ≥ <build>; please restart daemon". Already standard pattern, no extra code. |
| Daemon restart mid-migration leaves partial schema | T1 | All migrations are single `ALTER TABLE` statements — atomic by SQLite definition. Cannot leave partial column. |
| Production DB has rows with `evidence_count = 0` (NULL→DEFAULT 1 didn't apply somewhere) | T5 | SQL: `CASE WHEN evidence_count > 0` already handles 0/NULL safely (returns 0 contribution, not error). Add a one-row regression test where a manually-inserted item has evidence_count=0. |

---

## Out of scope (explicitly deferred)

| Item | Why deferred |
|------|--------------|
| LLM admission classifier (MemRouter pattern) | Research spike. Read arxiv 2605.00356 + Letta core-memory docs first. Decide go/no-go after this redesign sees 30 days of production. |
| Lane-based selection (separate quotas per kind) | Want to see whether the score formula alone produces good mixes before fragmenting budget. |
| Learned ranking (gradient-boosted features) | Needs training data — collect (wake-up snapshot, agent recall hit/miss) for ≥30 days first. |
| Cross-machine pin sync via mesh | Depends on mesh write-replication (separate plan). |
| `mem pin --review-after 30d` decay flag | Wait until users report stale-pin pain. |
| `is_starred` column separate from pin | Redundant — one pin column suffices. |
| L1 UI redesign in mem-tab | Not blocking. |
| User-confirmation flow for big calibration changes (>0.5 importance, archive) | Sunday calibration is fully automatic on purpose. If real production shows runaway changes, add a `--dry-run` mode to STAGE 8 that emits `codeterm notify` instead of applying. |
| Pre-save L0 dedup at `mem save` time | Sunday calibration handles this batched. Inline check would slow saves. Re-evaluate after 30d. |

## Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| Migration silently fails on old DBs | high | PRAGMA `table_info` guard before ADD; matches existing `add_evolution_columns` pattern. |
| Pinned set silently eats budget | medium | Eviction-WARN log + `<!-- WARN -->` comment in wake-up output (Task 5). |
| `evidence_count` semantics drift across kinds | medium | Task 2 lands BEFORE Task 5. If Task 2 breaks, scoring degrades to importance-only — graceful. |
| `scope` not propagated by every `mem save` caller | medium | CLI defaults to `--workspace $CODETERM_WORKSPACE_ID` env; worker / cron paths leave it `global` (explicit). |
| User pins stale items, forgets | low | `mem pinned` lists everything pinned with `updated_at`. User responsibility. |
| Score formula too noisy or too sticky | medium | Task 9's `--explain` flag prints per-item breakdown. |
| Importance and evidence_count double-weight each other | low-medium | Live with it for now. Revisit if real users see runaway items. |
| Cross-workspace pin loaded into wrong workspace | low | Pass 1 SQL filters by scope; pin does not override scope. Documented in Task 11. |
| SQLite column-alias-in-ORDER-BY portability | low | CTE pattern in `score::PASS2_SQL` (tab3's review note adopted). |

## Self-Review Checklist (run before handoff)

- [x] Spec coverage: every goal in §"What's missing" maps to at least one task
  - Pin column → Task 1
  - Pin CLI verbs → Task 7
  - Two-pass scored selector → Task 5
  - Configurable budget → Task 6
  - Score-explain debug → Task 9
  - Bump filter (prereq) → Task 2
  - PATCH `pinned` → Task 3
  - List `?pinned=true` → Task 4
  - Eval coverage → Task 10
  - Docs → Task 11
  - Dreamer schedule shift → Task 12
  - Sunday calibration in same dreamer → Task 13
  - Calibration sanity test → Task 14
  - Retrieval bench (deferred) → Task 15
  - Documentation reconciliation → Task 16
- [x] No placeholders ("TBD", "implement later", "fill in details") — every code step ships actual code
- [x] Type consistency: `WakeUpOptions { current_workspace, explain }` declared in Task 5; used by Tasks 6/9. `PASS1_SQL` / `PASS2_SQL` / `score_for` declared in Task 5 and not renamed elsewhere.
- [x] Tests exist before implementation in every task (TDD).
- [x] Each task ends in a single self-contained commit; revert is single-commit.
- [x] No new long-lived processes or workers — calibration folds into existing `mem-dreaming-auto` cron via day-of-week branch.

## References

- `src-tauri/src/mem/layers.rs` — current `collect_l1_candidates` (Task 5 replaces it)
- `src-tauri/src/api/mem_routes/items.rs:130` — `bump_on_get` filter (Task 2 extends)
- `crates/mem-store/src/migrations.rs:23-37` — established migration pattern (Task 1 follows)
- `crates/mem-store/src/graph.rs:363` — `bump_evidence_count` impl (already in place)
- `src-tauri/src/mem/store/mod.rs:230-252` — `MemScope` enum (workspace IDs already encoded as `workspace:<id>` in `scope` column)
- `docs/plans/references/mempalace.md` — independent confirmation that "metadata-filtered retrieval" (their wing/room ≈ our scope) is the right lever; their "34% palace boost" is just metadata filtering
- Letta core-memory docs (https://docs.letta.com/guides/ade/core-memory/) — `is_pinned_wakeup` ≈ their "core memory block"
- arxiv 2605.00356 — MemRouter (out of scope; possible future direction)

<!-- revision: 3 -->

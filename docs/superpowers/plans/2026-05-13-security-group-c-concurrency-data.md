# Security Group C — Concurrency & Data Integrity Fixes

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 6 CRITICAL concurrency and data integrity issues: lock ordering violation in PTY lifecycle, orphaned pane on spawn failure, and three multi-step SQL operations without transactions.

**Architecture:** Pure fix-only changes. Non-overlapping with Group A and B files. TDD where possible; `./build.sh check` + compilation verification for changes that can't be unit tested.

**Tech Stack:** Rust, Tauri v2, parking_lot Mutex, rusqlite, `cargo test`, `./build.sh check`

---

### Task 1: Fix lock ordering in handle_pty_death (pty_lifecycle.rs:166-167)

**Files:**
- Modify: `src-tauri/src/service/pty_lifecycle.rs` (function `handle_pty_death`)

**Root cause:** `handle_pty_death` acquires `agent_tracker` (position 4 in CLAUDE.md order) before `mux` (position 1). The mandated order is mux → output_buf → screen_state → agent_tracker → pty. Even though these are sequential (non-nested) acquisitions, any future refactor that nests them would create a deadlock potential. The convention must be maintained consistently.

- [ ] **Step 1: Locate the exact lines**

```bash
grep -n "agent_tracker\|mux.lock" src-tauri/src/service/pty_lifecycle.rs | head -20
```

Expected output includes:
```
166: svc.agent_tracker.lock().set_dead(pty_id);
167: let pane_id = svc.mux.lock().pane_id_for_pty(pty_id);
```

- [ ] **Step 2: Apply fix — swap lock acquisition order**

In `src-tauri/src/service/pty_lifecycle.rs`, in `handle_pty_death`, swap the first two lines:

```rust
// BEFORE:
fn handle_pty_death(svc: &ServiceState, pty_id: &str) {
    svc.agent_tracker.lock().set_dead(pty_id);
    let pane_id = svc.mux.lock().pane_id_for_pty(pty_id);

// AFTER:
fn handle_pty_death(svc: &ServiceState, pty_id: &str) {
    let pane_id = svc.mux.lock().pane_id_for_pty(pty_id);
    svc.agent_tracker.lock().set_dead(pty_id);
```

- [ ] **Step 3: Verify compilation**

```bash
./build.sh check 2>&1 | tail -5
```
Expected: no errors. The fix is a pure line-swap with no API changes.

- [ ] **Step 4: Run existing tests**

```bash
./build.sh test 2>&1 | tail -10
```
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/service/pty_lifecycle.rs
git commit -m "fix(service): acquire mux before agent_tracker in handle_pty_death — follow lock ordering convention"
```

---

### Task 2: Verify spawn.rs has no independent lock ordering violation

**Files:**
- Read-only: `src-tauri/src/service/spawn.rs`

**Root cause (from review):** The review flagged spawn.rs as having a potential inversion via the call chain spawn → pty_lifecycle → handle_pty_death. After Task 1 fixes handle_pty_death, verify spawn.rs itself has no direct inversion.

- [ ] **Step 1: Check agent_tracker usage in spawn.rs**

```bash
grep -n "agent_tracker" src-tauri/src/service/spawn.rs
```
Expected: 0 matches (confirmed in pre-plan analysis).

- [ ] **Step 2: Check lock acquisition order in spawn_hidden_agent_impl**

```bash
grep -n "\.mux\.lock\|\.pty\.lock\|agent_tracker" src-tauri/src/service/spawn.rs
```

Verify: mux is locked first, pty locked after, no agent_tracker acquisitions. If the output confirms no inversions, no fix needed.

- [ ] **Step 3: Document finding**

If no `agent_tracker` in spawn.rs: the C2 finding from the review is fully resolved by Task 1 (fixing handle_pty_death). No further changes required.

If any `agent_tracker` acquisition found BEFORE a `mux` acquisition: apply the same swap pattern as Task 1.

---

### Task 3: Clean up orphaned pane on background PTY spawn failure (orchestrator.rs)

**Files:**
- Modify: `src-tauri/src/service/orchestrator.rs` (function `launch_agent_in_background`)
- Modify: `src-tauri/src/service/orchestrator.rs` (call site of `launch_agent_in_background`)

**Root cause:** In `launch_agent_in_background`, when the backend PTY spawn fails (the `Err(e)` branch around line 450), the function returns without removing the pane that was already created in the mux. This leaks a dead pane visible in the UI forever.

- [ ] **Step 1: Locate the failure branch**

```bash
grep -n "backend PTY spawn failed\|Err(e)" src-tauri/src/service/orchestrator.rs | head -10
```
Expected: finds the `log::error!("launch_agent: backend PTY spawn failed")` line.

- [ ] **Step 2: Locate the `launch_agent_in_background` function signature**

```bash
grep -n "pub.* fn launch_agent_in_background" src-tauri/src/service/orchestrator.rs
```

Note the function signature. We will add `tab_id: String, ws_id: String` parameters.

- [ ] **Step 3: Update `launch_agent_in_background` signature**

In `src-tauri/src/service/orchestrator.rs`, update the `launch_agent_in_background` function signature to include `tab_id` and `ws_id`:

```rust
// BEFORE:
pub(crate) fn launch_agent_in_background(
    svc: ServiceContext,
    pane_id: String,
    launch_cmd: String,
    agent_cmd: String,
    task: Option<String>,
    ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
) {

// AFTER:
pub(crate) fn launch_agent_in_background(
    svc: ServiceContext,
    pane_id: String,
    tab_id: String,
    ws_id: String,
    launch_cmd: String,
    agent_cmd: String,
    task: Option<String>,
    ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
) {
```

- [ ] **Step 4: Add cleanup in the PTY spawn failure branch**

In the background thread inside `launch_agent_in_background`, find the `Err(e)` arm of `svc.pty.lock().spawn(...)`. Add pane cleanup before `return`:

```rust
// BEFORE:
                    Err(e) => {
                        log::error!("launch_agent: backend PTY spawn failed: {}", e);
                        if let Some(tx) = ready_tx {
                            let _ = tx.send(Err(format!("backend PTY spawn failed: {}", e)));
                        }
                        return;
                    }

// AFTER:
                    Err(e) => {
                        log::error!("launch_agent: backend PTY spawn failed for pane {}: {}", pane_id, e);
                        // Clean up the orphaned pane — it has no PTY and is unreachable
                        let _ = svc.mux.lock().close_tab(&ws_id, &tab_id);
                        let _ = svc.app_handle.emit("mux-state-changed", ());
                        if let Some(tx) = ready_tx {
                            let _ = tx.send(Err(format!("backend PTY spawn failed: {}", e)));
                        }
                        return;
                    }
```

- [ ] **Step 5: Update all call sites of `launch_agent_in_background`**

Find all call sites:

```bash
grep -n "launch_agent_in_background" src-tauri/src/service/orchestrator.rs
```

For each call site, add the `tab_id` and `ws_id` arguments. In `spawn_agent_tab` (the main spawn path), the `tab_id` and `ws_id` are already computed before the call:

```rust
// BEFORE:
launch_agent_in_background(self.clone(), pane_id.clone(), launch_cmd, agent_cmd.to_string(), bg_task, ready_tx);

// AFTER:
launch_agent_in_background(self.clone(), pane_id.clone(), tab_id.clone(), ws_id.clone(), launch_cmd, agent_cmd.to_string(), bg_task, ready_tx);
```

For `spawn_hidden_agent_impl` (in spawn.rs), if it also calls `launch_agent_in_background`, similarly add the arguments. Check:

```bash
grep -n "launch_agent_in_background" src-tauri/src/service/spawn.rs
```

Add `tab_id` and `ws_id` from the variables already computed at the spawn site.

- [ ] **Step 6: Verify compilation**

```bash
./build.sh check 2>&1 | tail -10
```
Expected: no errors. Fix any remaining call sites with missing arguments.

- [ ] **Step 7: Run existing tests**

```bash
./build.sh test 2>&1 | tail -10
```
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/service/orchestrator.rs src-tauri/src/service/spawn.rs
git commit -m "fix(service): close orphaned pane tab on backend PTY spawn failure in launch_agent_in_background"
```

---

### Task 4: Add transaction to add_alias in mem/graph.rs (C4)

**Files:**
- Modify: `src-tauri/src/mem/graph.rs` (function `add_alias`)

**Root cause:** `add_alias` runs 5 SQL statements (INSERT, UPDATE, UPDATE, DELETE, DELETE) without a transaction. A crash between any two steps leaves the graph in a partially-consistent state: relations may point to an alias entity that no longer exists, or be deleted before the alias entity is removed.

- [ ] **Step 1: Locate the function**

```bash
grep -n "fn add_alias" src-tauri/src/mem/graph.rs
```
Expected: `396: pub fn add_alias(conn: &Connection, alias_id: &str, canonical_id: &str) -> Result<(), String>`

- [ ] **Step 2: Check existing tests**

```bash
grep -n "#\[test\]" src-tauri/src/mem/graph.rs | head -10
```

Note existing tests to avoid duplication.

- [ ] **Step 3: Write test for add_alias atomicity**

Add to `src-tauri/src/mem/graph.rs` in the `#[cfg(test)]` block (or create one at the end of the file):

```rust
#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("
            CREATE TABLE entities (id TEXT PRIMARY KEY, entity_type TEXT, scope TEXT, properties TEXT, created_at TEXT);
            CREATE TABLE entity_aliases (alias_id TEXT PRIMARY KEY, canonical_id TEXT, created_at TEXT);
            CREATE TABLE relations (
                from_id TEXT, relation_type TEXT, to_id TEXT, scope TEXT,
                valid_from TEXT, valid_to TEXT, confidence REAL, properties TEXT
            );
        ").unwrap();
        conn
    }

    #[test]
    fn add_alias_rewires_relations() {
        let conn = setup_db();
        let now = "2026-01-01T00:00:00Z";
        conn.execute("INSERT INTO entities VALUES ('alias', 'Person', 'global', '{}', ?1)", rusqlite::params![now]).unwrap();
        conn.execute("INSERT INTO entities VALUES ('canonical', 'Person', 'global', '{}', ?1)", rusqlite::params![now]).unwrap();
        conn.execute("INSERT INTO relations VALUES ('alias', 'knows', 'other', 'global', ?1, NULL, 1.0, '{}')", rusqlite::params![now]).unwrap();

        add_alias(&conn, "alias", "canonical").unwrap();

        // Alias entity should be gone
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM entities WHERE id = 'alias'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0, "alias entity should be deleted");

        // Relation should now point from canonical
        let from: String = conn.query_row("SELECT from_id FROM relations WHERE relation_type = 'knows'", [], |r| r.get(0)).unwrap();
        assert_eq!(from, "canonical", "relation should be rewired to canonical");
    }
}
```

- [ ] **Step 4: Run test — verify it passes with current code**

```bash
cargo test -p codeterm add_alias_rewires_relations 2>&1 | tail -5
```
Expected: PASS (the logic is correct; we're adding the transaction wrapper for crash safety).

- [ ] **Step 5: Apply fix — wrap in transaction**

In `src-tauri/src/mem/graph.rs`, update `add_alias` to use a transaction:

```rust
pub fn add_alias(conn: &Connection, alias_id: &str, canonical_id: &str) -> Result<(), String> {
    if alias_id == canonical_id {
        return Err("alias cannot equal canonical".into());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction().map_err(|e| format!("add_alias transaction: {}", e))?;
    tx.execute(
        "INSERT OR REPLACE INTO entity_aliases (alias_id, canonical_id, created_at) VALUES (?1, ?2, ?3)",
        params![alias_id, canonical_id, &now],
    ).map_err(|e| format!("add_alias: {}", e))?;
    tx.execute(
        "UPDATE OR IGNORE relations SET from_id = ?1 WHERE from_id = ?2",
        params![canonical_id, alias_id],
    ).map_err(|e| format!("rewire from: {}", e))?;
    tx.execute(
        "UPDATE OR IGNORE relations SET to_id = ?1 WHERE to_id = ?2",
        params![canonical_id, alias_id],
    ).map_err(|e| format!("rewire to: {}", e))?;
    tx.execute(
        "DELETE FROM relations WHERE from_id = ?1 OR to_id = ?1",
        params![alias_id],
    ).map_err(|e| format!("cleanup alias relations: {}", e))?;
    tx.execute(
        "DELETE FROM entities WHERE id = ?1",
        params![alias_id],
    ).map_err(|e| format!("delete alias entity: {}", e))?;
    tx.commit().map_err(|e| format!("add_alias commit: {}", e))?;
    Ok(())
}
```

Note: `conn` binding changes from `conn.execute(...)` → `tx.execute(...)` for all 5 SQL ops. `tx.commit()` at the end. The `Transaction` from `unchecked_transaction()` auto-rolls back on drop if `commit()` is not called.

- [ ] **Step 6: Run test — verify it still passes**

```bash
cargo test -p codeterm add_alias_rewires_relations 2>&1 | tail -5
```
Expected: PASS.

- [ ] **Step 7: Verify compilation**

```bash
./build.sh check 2>&1 | tail -5
```
Expected: no errors.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/mem/graph.rs
git commit -m "fix(mem): wrap add_alias in transaction — prevent partial graph state on crash"
```

---

### Task 5: Add transaction to add_alias_with_rewire in crates/mem-store (C5)

**Files:**
- Modify: `crates/mem-store/src/graph.rs` (function `add_alias_with_rewire`, line 384)

**Root cause:** Same as C4 but in the `mem-store` crate. 5 destructive SQL statements without a transaction. Crash between statements leaves the graph permanently inconsistent.

- [ ] **Step 1: Locate and read the function**

```bash
sed -n '384,430p' crates/mem-store/src/graph.rs
```

Verify the 5 SQL ops: INSERT entity_aliases, UPDATE from_id, UPDATE to_id, DELETE alias relations, DELETE alias entity.

- [ ] **Step 2: Check existing tests**

```bash
grep -n "#\[test\]" crates/mem-store/src/graph.rs | head -10
```

- [ ] **Step 3: Apply fix — wrap in transaction**

In `crates/mem-store/src/graph.rs`, update `add_alias_with_rewire` (line 384):

```rust
pub fn add_alias_with_rewire(
    conn: &Connection,
    alias_id: &str,
    canonical_id: &str,
) -> MemStoreResult<()> {
    if alias_id == canonical_id {
        return Err(MemStoreError::InvalidArgument(
            "alias cannot equal canonical".into(),
        ));
    }
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT OR REPLACE INTO entity_aliases (alias_id, canonical_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![alias_id, canonical_id, &now],
    )?;
    tx.execute(
        "UPDATE OR IGNORE relations SET from_id = ?1 WHERE from_id = ?2",
        rusqlite::params![canonical_id, alias_id],
    )?;
    tx.execute(
        "UPDATE OR IGNORE relations SET to_id = ?1 WHERE to_id = ?2",
        rusqlite::params![canonical_id, alias_id],
    )?;
    tx.execute(
        "DELETE FROM relations WHERE from_id = ?1 OR to_id = ?1",
        rusqlite::params![alias_id],
    )?;
    tx.execute(
        "DELETE FROM entities WHERE id = ?1",
        rusqlite::params![alias_id],
    )?;
    tx.commit()?;
    Ok(())
}
```

Same pattern as C4: replace `conn.execute(...)` → `tx.execute(...)`, add `tx.commit()` at end.

- [ ] **Step 4: Verify compilation**

```bash
cargo check -p mem-store 2>&1 | tail -5
```
Expected: no errors.

- [ ] **Step 5: Run crate tests**

```bash
cargo test -p mem-store 2>&1 | tail -10
```
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/mem-store/src/graph.rs
git commit -m "fix(mem-store): wrap add_alias_with_rewire in transaction — prevent partial graph state"
```

---

### Task 6: Add transaction to canonicalize_relation_types in crates/mem-store (C6)

**Files:**
- Modify: `crates/mem-store/src/graph.rs` (function `canonicalize_relation_types`, line 280)

**Root cause:** The function loops over synonym pairs, running `UPDATE OR IGNORE` + `DELETE` for each. No transaction per pair or across the loop. A crash leaves both canonical and synonym relation types active on the same edges — a corrupt graph state.

- [ ] **Step 1: Read the function**

```bash
sed -n '280,320p' crates/mem-store/src/graph.rs
```

Verify: a loop over `synonyms`, 2 SQL ops per iteration, no transaction.

- [ ] **Step 2: Apply fix — wrap entire loop in a single transaction**

In `crates/mem-store/src/graph.rs`, update `canonicalize_relation_types` (line 280):

```rust
pub fn canonicalize_relation_types(
    conn: &Connection,
    synonyms: &[(&str, &str)],
) -> MemStoreResult<u64> {
    let tx = conn.unchecked_transaction()?;
    let mut total: u64 = 0;
    for (from, to) in synonyms {
        let updated = tx.execute(
            "UPDATE OR IGNORE relations SET relation_type = ?1 \
             WHERE relation_type = ?2 AND valid_to IS NULL",
            rusqlite::params![to, from],
        )? as u64;
        let dropped = tx.execute(
            "DELETE FROM relations WHERE relation_type = ?1 AND valid_to IS NULL",
            rusqlite::params![from],
        )? as u64;
        total += updated + dropped;
    }
    tx.commit()?;
    Ok(total)
}
```

Change: add `let tx = conn.unchecked_transaction()?` before the loop, replace `conn.execute` → `tx.execute` inside the loop, add `tx.commit()` after the loop.

- [ ] **Step 3: Verify compilation**

```bash
cargo check -p mem-store 2>&1 | tail -5
```
Expected: no errors.

- [ ] **Step 4: Run crate tests**

```bash
cargo test -p mem-store 2>&1 | tail -10
```
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/mem-store/src/graph.rs
git commit -m "fix(mem-store): wrap canonicalize_relation_types in transaction — atomic synonym loop"
```

---

### Final verification

```bash
./build.sh check 2>&1 | tail -5
./build.sh test 2>&1 | tail -10
cargo test -p mem-store 2>&1 | tail -5
```
Expected: no errors, all tests green across all crates.

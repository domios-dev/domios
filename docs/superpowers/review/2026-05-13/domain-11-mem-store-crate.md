# Domain Review: mem-store Crate

## Files Reviewed

- `crates/mem-store/Cargo.toml`
- `crates/mem-store/src/lib.rs`
- `crates/mem-store/src/error.rs`
- `crates/mem-store/src/graph.rs`
- `crates/mem-store/src/migrations.rs`
- `crates/mem-store/src/cluster.rs`
- `crates/mem-store/src/lesson.rs`
- `crates/mem-store/src/plan_format.rs`
- `crates/mem-store/src/audit.rs`
- `crates/mem-store/src/tests/mod.rs`
- `crates/mem-store/src/tests/cluster.rs`
- `crates/mem-store/src/tests/lesson.rs`

---

## Findings

### `crates/mem-store/src/graph.rs`

#### CRITICAL

- **`graph.rs:384–416` — `add_alias_with_rewire` has no transaction.** Five separate `conn.execute()` calls run without a wrapping transaction. If any step after the first fails (e.g. the second `UPDATE OR IGNORE` or the `DELETE FROM relations`), the `entity_aliases` row and the partial `from_id` rewires are already committed. The alias entity still exists in `entities`, alias references remain in `relations`, and the graph is permanently inconsistent. Every other multi-step write in this file wraps in an `unchecked_transaction()`; this function is the sole exception and it is the most structurally destructive operation in the crate.

- **`graph.rs:280–298` — `canonicalize_relation_types` has no transaction.** The loop over `synonyms` issues an `UPDATE OR IGNORE` followed by a `DELETE` for each pair, all bare on `&Connection`. If the `DELETE` fails after the `UPDATE`, some relations are renamed but their old-type counterparts are not cleaned up, leaving both the canonical and the synonym name active for the same edge set. There is also no transaction wrapping the entire synonym sweep, so a failure mid-loop leaves previous pairs committed and later ones un-applied.

#### HIGH

- **`graph.rs:80–104` — `purge_invalidated`: count is taken outside the transaction; audit log records a stale count.** The `SELECT COUNT(*)` runs against the live connection (no transaction), then the `DELETE` runs inside an `unchecked_transaction`. If rows matching the filter are inserted or deleted by another writer between these two statements, the `count` in the `PurgeReport` and in the `graph_purge_log` audit row diverges from the number of rows actually deleted. The audit record is authoritative for compliance tracing; a wrong count there is misleading. The fix is to run the count inside the same transaction as the delete (SQLite's default isolation would then ensure consistency).

- **`graph.rs:151–184` — `revive_broken` has the same pre-tx count pattern.** The `SELECT COUNT(*)` is outside the transaction; the `UPDATE` is inside. `ReviveBrokenReport::revived` can overcount or undercount actual rows touched. Unlike `purge_invalidated`, there is no audit write at all, so there is no audit-log corruption — but the returned count is unreliable for callers that act on it.

- **`graph.rs:224–231` — `introduce_topic` silently accepts a nonexistent anchor.** When the anchor is absent from `entities`, the `query_row` returns `Err`, and the `unwrap_or_else` swallows the error, defaulting `scope` to `"global"`. The function then proceeds to create the topic entity and wire source edges to an anchor that does not exist in `entities`. SQLite has no FK enforcement by default, so the inserts succeed. The result is graph edges pointing to a ghost anchor. The function should return `MemStoreError::NotFound` when the anchor is missing.

- **`graph.rs:148` — `route()` always reports `edges_created: 2` regardless of `INSERT OR IGNORE` outcome.** Both segment inserts use `INSERT OR IGNORE`. If the segments already existed (e.g. `route` is called twice on the same triple), both inserts are silently skipped but the report still claims `2` edges were created. Callers observing the report for idempotency checking will get a false positive.

#### MEDIUM

- **`graph.rs:325–343` — `cleanup_orphan_entities` panics on empty `protected_types`.** When `protected_types` is `&[]`, `placeholders` becomes an empty string, producing `WHERE entity_type NOT IN ()`, which is a SQLite syntax error. The function returns `Err(MemStoreError::Sqlite(...))` rather than panicking, but the error message is opaque and callers have no way to know the API precondition. There is no guard or documentation for this constraint. The current production call site uses a non-empty constant, so no live bug exists today, but the precondition is invisible to future callers.

- **`graph.rs` — Multiple `///` doc-comment blocks span 2–3 lines**, violating the project's one-line-max comment rule (`CLAUDE.md`). Affected functions: `canonicalize_relation_types` (3 lines), `list_entities_with_degree` (2 lines), `cleanup_orphan_entities` (2 lines), `invalidate_broken_relations` (3 lines), `bump_evidence_count` (3 lines), `bump_false_fire_count` (2 lines), `add_alias_with_rewire` (3 lines).

---

### `crates/mem-store/src/lesson.rs`

#### HIGH

- **`lesson.rs:59` — `serde_json::to_string` failure silently discards existing tags.** When serializing the updated `tags` `Vec<String>`, if `to_string` fails (theoretically impossible for `Vec<String>` but guarded anyway), the fallback produces `["high-confidence"]`, dropping all pre-existing tags. Because `serde_json::to_string` cannot fail for `Vec<String>` in practice, the `unwrap_or_else` branch is unreachable dead code, but its semantics are data-destructive. It should either be `.expect("serializing Vec<String> is infallible")` (documents the invariant) or removed with a comment explaining why the error is impossible.

#### LOW

- **`lesson.rs` — Module-level `//!` block spans 5 lines**, violating the one-line-max rule. Contains version references (`v1.1.23`, `v1.1.24`, `Issue #59`) that belong in git commit messages, not inline comments.

---

### `crates/mem-store/src/plan_format.rs`

#### MEDIUM

- **`plan_format.rs:137–178` — Unclosed yaml fence is silently swallowed with no warning.** If the input markdown ends with an open ` ```yaml ` fence (no closing ` ``` `), `in_fence` remains `true` at EOF, `fence_body` is discarded, and no warning is emitted. A partial fence containing valid fields is silently lost. The `read` function should push a warning when `in_fence` is true at end of input.

- **`plan_format.rs:39–73` — `write()` does not escape field values containing newlines or colons.** If a `topic_id`, `anchor`, `entity_type`, `rel_source`, `rel_anchor`, or source string contains a newline character, the serialized output breaks the hand-parser's line-oriented assumption. A roundtrip `write → read` on such a value would either silently drop the field or produce a warning. No validation occurs at entry or at serialization time.

#### LOW

- **`plan_format.rs` — Module-level `//!` block spans 7 lines**, violating the one-line-max rule.

---

### `crates/mem-store/src/migrations.rs`

All five column-addition functions guard with `PRAGMA table_info` before issuing `ALTER TABLE`, making them idempotent. `CREATE TABLE IF NOT EXISTS` and `CREATE INDEX IF NOT EXISTS` are used correctly. No issues found.

#### LOW

- **`migrations.rs:14–20` — `add_reason_column` uses `.filter_map(Result::ok)` to silently ignore row-fetch errors from `PRAGMA table_info`.** If the PRAGMA query itself returns an error for an individual row, the column name is silently dropped from the `cols` vector, which could lead to a double `ALTER TABLE` attempt that fails. This pattern repeats in `add_evolution_columns`, `add_promotion_threshold`, and `add_pin_column`. Using `collect::<Result<Vec<_>, _>>()?` would surface errors explicitly. In practice, `PRAGMA table_info` never returns row-level errors, so this is advisory only.

---

### `crates/mem-store/src/cluster.rs`

No correctness issues. The `apply` loop correctly isolates per-cluster failures and continues processing remaining clusters. Internally delegates all writes to `graph::introduce_topic`, which is transactional.

#### LOW

- **`cluster.rs` — Module-level `//!` block spans 9 lines** containing a version roadmap (`v1.1.23`, `v1.1.24`) and historical rationale. Version roadmap belongs in git history, not inline source comments.

---

### `crates/mem-store/src/error.rs`

Single error enum, `thiserror`-derived, covers the four cases callers need. No catch-all variant that would hide detail. The `Sqlite` variant wraps `rusqlite::Error` via `#[from]`, which leaks the `rusqlite` type into the public error API — callers must depend on `rusqlite` transitively to match on the inner error. This is a deliberate design choice (all public functions take `&Connection`) but is noted below.

#### HIGH

- **`error.rs` + `lib.rs` — The entire public API surface is bound to `rusqlite::Connection` and `rusqlite::Transaction`.** Every public function in `graph`, `migrations`, `lesson`, and `cluster` takes `&rusqlite::Connection`. `audit::record_purge` takes `&rusqlite::Transaction`. This means all downstream crates must depend on `rusqlite` directly to call any function. There is no abstraction layer — SQLite is the crate's external interface, not an implementation detail. This is acceptable for an intentionally narrow internal crate (the crate is not published externally), but it means the crate cannot be swapped for a different storage backend without touching every callsite in `src-tauri` and `cli`. This is noted as an architectural observation rather than a defect.

---

### `crates/mem-store/src/audit.rs`

Single function, correctly scoped to `&Transaction`. No issues found.

---

### `crates/mem-store/src/lib.rs`

#### MEDIUM

- **`lib.rs:5–11` — All modules are `pub mod`**, including `audit`, which exposes `record_purge(&Transaction, ...)`. This is an internal helper that graph mutations call within their own transactions. Exposing it publicly means callers outside the crate can write inconsistent audit rows (e.g., calling `record_purge` with a count that doesn't match any actual delete). `audit` should be `pub(crate)`.

---

### `crates/mem-store/src/tests/mod.rs`

#### MEDIUM

- **`tests/mod.rs` at 653 lines** contains all graph-layer tests (migrations, purge, route, revive, introduce_topic, canonicalize, list_entities, cleanup, invalidate, alias/rewire, bump_evidence, bump_false_fire, pin migration). These 26 tests cover functionality spread across `graph.rs`, `migrations.rs`, and `audit.rs`. The tests for `lesson` and `cluster` were correctly moved to `tests/lesson.rs` and `tests/cluster.rs`, but graph-layer tests remain monolithic in `mod.rs`. At 653 lines, `mod.rs` already approaches the 1000-line limit and will exceed it when new graph functions are added. Tests for each concern (`purge`, `route`, `alias`, `cleanup`) belong in separate files: `tests/graph_purge.rs`, `tests/graph_route.rs`, etc.

- **`tests/mod.rs:6–46` — `init_v1_22_schema` is a 40-line test fixture** that replicates the v1.22 schema. This inline SQL schema will need updating every time a table changes, and it diverges from the actual schema created by the main daemon's migration path. The fixture should be extracted to `tests/fixtures.rs` and documented as "schema as of v1.22, before migrations apply".

#### LOW

- **No test for `cleanup_orphan_entities` with `protected_types = &[]`.** The function will return a `rusqlite::Error` (syntax error) in that case, but this path is undocumented and untested.

- **No test for unclosed yaml fence in `plan_format::read`.** The silent-discard behavior (no warning emitted) is not covered.

- **No test for `introduce_topic` with a nonexistent anchor.** The current behavior (silent `scope` fallback to `"global"`, edges to a ghost anchor) is the bug described above; the absence of a test means the regression will not be caught.

---

## Summary Table

| Severity | Count |
|----------|-------|
| CRITICAL | 2     |
| HIGH     | 6     |
| MEDIUM   | 6     |
| LOW      | 7     |
| **Total**| **21**|

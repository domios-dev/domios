# mem-replication-v0

Cross-machine mem replication between paired CodeTerm peers. v0 = shippable minimum; v1+ refinements deferred.

## Motivation

Andrey runs two paired CodeTerm peers (BLACKSTAR + Mac). He wants shared memory: facts, notes, learnings, skills, the entity graph — all visible from either side, modulo a small set of machine-specific items he flags. Today every `mem save` lands locally only. Manual sync is friction; today's `codeterm-shared-mem` git repo handles the skills subset only and forces hourly cron lag.

Quote: «хочу просто общую память общие скилы общие все ну может специфичное что то будет но явно надо как то шарить память».

Designed jointly by BLACKSTAR (Rollo) and Mac (Rollo) over a brainstorming + research pass on 2026-05-03. Both peers researched the codeterm tree (BLACKSTAR: mem internals; Mac: platform plumbing) before drafting.

## Agreed decisions

### D1 — Replication identity is `mem_items.uid` (ULID), not `id`

`mem_items.id` is a slug derived from title + collision hash; collisions are resolved per-machine and not content-addressed across peers. `mem_items.uid` is already a ULID — globally unique, time-ordered, immutable, never rewritten on rename. Use `uid` as the canonical cross-machine key for everything: replication, history, blob fetch, conflict detection. The `id` column stays for human-readable references and FTS hits.

### D2 — `mem_item_history` is the event log, not a new table

`mem_item_history` (`store.rs:371`) is already append-only with `INTEGER PRIMARY KEY AUTOINCREMENT`. Add three columns and we have ~80% of the event log for free:

```diff
 CREATE TABLE IF NOT EXISTS mem_item_history (
   id              INTEGER PRIMARY KEY AUTOINCREMENT,
   item_id         TEXT NOT NULL,
+  uid             TEXT,                 -- cross-machine canonical key (NULL = legacy pre-replication)
   changed_at      TEXT NOT NULL,
   changed_by      TEXT NOT NULL,        -- semantic only: "user", "librarian", "dream", "agent:<id>"
+  origin_machine_id TEXT,               -- peer that originated; NULL = legacy
+  parent_uid_hash TEXT,                 -- sha256(uid + previous changed_at) for divergence detection
+  replicated      INTEGER NOT NULL DEFAULT 0,  -- 1 = mesh fanout target; 0 = legacy or local-only audit
   change_type     TEXT NOT NULL,
   old_title, new_title, old_body, new_body, old_importance, new_importance, old_kind, new_kind, reason
 );
 CREATE INDEX idx_history_item ON mem_item_history(item_id, changed_at DESC);
 CREATE INDEX idx_history_changed_by ON mem_item_history(changed_by);
+CREATE INDEX idx_history_uid_changed ON mem_item_history(uid, changed_at DESC);
+CREATE INDEX idx_history_replicated ON mem_item_history(replicated, id) WHERE replicated = 1;
```

Existing rows backfill cleanly: `uid = mem_items.uid WHERE mem_items.id = item_id`, `origin_machine_id = NULL`, `replicated = 0`. New helper `history_log_replicated()` writes fatal (returns `Result`) so replication-relevant rows are guaranteed-or-aborted; legacy `history_log()` stays best-effort for non-replicated audits.

### D3 — Two-tier replication transport

* **Live overlay**: Tauri in-process `EventBroadcaster` gets new variants `MemItemSaved{uid}`, `MemItemDeleted{uid}`, `MemItemMerged{from_uid, into_uid}`. A new replicator task subscribes, looks up the matching history row, signs + POSTs to each paired peer's `/api/mesh/relay → /api/mem/events`. Best-effort; failures get cursor-recovered.
* **Cursor pull (catch-up)**: peer reconnects after partition. Pull `/api/mem/replicate/since/{last_history_id}` from each peer. NDJSON streaming response; receiver applies in id order. Cursor advances on success.

Both layers write to the same `mem_item_history` rows; peers dedup on `(uid, parent_uid_hash, change_type)` so receiving the same event twice is a no-op.

### D4 — Deterministic `uid` for regex-extracted items

`mem/extract.rs` regex auto-extraction stays per-machine (deterministic + cheap), but the extracted child `uid` is computed from `(parent_uid, pattern_name, match_position)` instead of a fresh ULID. Same input on both peers → same `uid` → no duplicate item, no conflict-detection noise.

```rust
fn deterministic_uid_for_extraction(parent_uid: &str, pattern_name: &str, match_position: usize) -> String {
    let h = sha256(format!("{}|{}|{}", parent_uid, pattern_name, match_position));
    // ULID-shaped output for consistency: take 26 base32 chars from hash
    base32_encode(&h[..16])
}
```

LLM-driven extraction (`auto_extract.rs::extract_with_llm`) stays primary-only because output is non-deterministic AND token-cost.

### D5 — Deterministic entity IDs (P0)

Today: caller-supplied entity ids, no scheme; LLM extraction produces inconsistent slugs. Two peers extracting `Person: Andrey` independently → `person_andrey` vs `Person_Andrey` vs `andrey-kovalev` — three different entities, dangling relations on replicate.

```rust
fn deterministic_entity_id(entity_type: &str, canonical_name: &str) -> String {
    format!("{}:{}", entity_type, slugify_lossy(canonical_name))
    // Person:andrey-kovalev, Project:codeterm, Tech:postgres
}

fn slugify_lossy(s: &str) -> String {
    // unicode-normalise NFD, strip diacritics, lowercase, [^a-z0-9-] -> '-', collapse '-+', trim '-'
}
```

Migration: one-off pass on schema upgrade scans `entities`, computes the canonical id, updates `relations.from_id`/`to_id` atomically, marks superseded entries via `entity_aliases`.

### D6 — Per-relation conflict detection (P0)

`relations` table needs `origin_machine_id` + parent-hash check on `(from_uid, to_uid, relation_type, valid_from)`. Concurrent invalidate (BLACKSTAR sets `valid_to = T0`, Mac sets `valid_to = T1`) → divergence detected, notification fires "concurrent invalidate on relation X — peer set valid_to=A, you set valid_to=B". LWW resolves table state; user reconciles via `mem graph relate --valid-to ...` if needed.

```diff
 CREATE TABLE IF NOT EXISTS relations (
   from_id, to_id, relation_type,
   valid_from, valid_to,
   confidence, properties_json,
   created_at,
+  origin_machine_id TEXT,    -- peer that opened this relation; NULL = legacy
   UNIQUE(from_id, relation_type, to_id, valid_from)
 );
```

### D7 — Lazy blob fetch via sha-addressed `/api/mem/blob/<sha>`

Items with `body_blob_path` (>2KB) replicate metadata + sha + size; peer fetches blob on first access. Two distinct items holding the same blob → one fetch on receiver. Blob endpoint is content-addressed, mesh-signed.

### D8 — Re-vectorize on receive

Same fastembed model + same body = deterministic vector. Receiver re-vectorizes asynchronously via existing `embed_queue.rs`. No vector bytes in replication payload.

### D9 — `dream_role` config (per-machine, primary | follower | auto)

```toml
[integrations.mem]
dream_role = "auto"   # Primary | Follower | Auto (default)
shared_areas_blacklist = []   # e.g. ["_dreams"] if user opts out
embedding_model = "all-MiniLM-L6-v2"   # pinned, validated on handshake
```

**Auto detection**: workstation-class machines (BLACKSTAR) default to Primary; laptop-class default to Follower. Andrey's override is the literal config value.

**Determinism rule** (refined): gate by trigger model + cost.

| Trigger             | Cost level   | Gating       |
|---------------------|--------------|--------------|
| Reactive on local-save event | deterministic + cheap | per-machine |
| Reactive on local-save event | LLM-driven            | primary-only |
| Scheduled / cron-fired       | any                   | primary-only |
| User-invoked one-off         | any                   | wherever invoked |

So:
* `extract.rs` (regex) — per-machine.
* `auto_extract.rs` (LLM) — primary-only.
* `dreaming.rs` (cron) — primary-only (current SQL impl is deterministic, but cycle-savings + history-noise hygiene wins).
* User-typed `mem save` — wherever the user typed it.

**Multi-primary safeguard**: peers exchange resolved `dream_role` on every mesh announce. Two primaries detected → log-warn + first-by-instance-id wins. Soft hint, not enforcement; surfaces misconfig.

### D10 — Bootstrap not replay

First pair-up: peer downloads a snapshot of current state via `POST /api/mem/replicate/bootstrap` (NDJSON streaming) instead of replaying multi-month history. Cursor advances to the post-bootstrap `last_history_id`. Subsequent updates flow via the event log.

Streaming format (NDJSON):
```
{"kind":"item","payload":{...full mem_items row...}}
{"kind":"entity","payload":{...full entities row...}}
{"kind":"relation","payload":{...full relations row...}}
{"kind":"item","payload":{...}}
...
{"kind":"complete","cursor":12345}
```

Receiver streams-applies in transaction batches of ~500 rows. ~30s for 50K rows on local network.

## New endpoints

```
POST /api/mem/events                     — apply replicated event (mesh-only, X-Mesh-Sender header signed)
POST /api/mem/replicate/bootstrap        — accept NDJSON snapshot stream
GET  /api/mem/replicate/since/{cursor}   — return events since cursor (NDJSON)
GET  /api/mem/blob/{sha}                 — content-addressed blob fetch
```

All four require `X-Mesh-Sender` + signature, validated against paired-peer registry. The relay path at `/api/mesh/relay` already does this for inbound; we expose the same guard at the route level so internal callers can't bypass.

## New `SystemEvent` variants

```rust
pub enum SystemEvent {
    // ...existing...
    MemItemSaved { uid: String, kind: String },
    MemItemDeleted { uid: String },
    MemItemMerged { from_uid: String, into_uid: String },
}
```

## Announce extension

`broadcast_announce` payload (`mesh/mod.rs:461`) gains:

```rust
struct AnnouncePayload {
    // ...existing peer info...
    current_timestamp_ms: u64,           // for clock-skew detection (warn at >5s delta)
    dream_role: String,                  // resolved Primary | Follower
    schema_version: u32,                 // refuse replication on mismatch
    embedding_model: String,             // refuse replication on mismatch
}
```

Mismatch on any handshake field → log error, skip replication for that peer, surface notification "peer X on schema_version Y, ours Z; replication paused until upgraded".

## Conflict resolution

Per-item LWW with parent-hash divergence detection:

* Each replicated event carries `parent_uid_hash = sha256(uid + previous_changed_at_or_genesis)`.
* On receive, compute expected hash from local state. Match → apply. Mismatch → divergent edit.
* Divergent edit: take by `changed_at` (RFC3339); ties broken alphabetical machine_id. Loser's body archived in `mem_item_history` with `change_type='replaced_by_lww'`. Notification fires.
* User can restore via `mem rewind <uid>` which scans history and offers to restore prior state. (CLI lands in v1; v0 just notifies.)

## P0 risks (block ship)

* **Schema migration coordination**. Both peers must run the same `schema_version`. Migration script is single transaction, idempotent, runs at codeterm startup if `mem_items.schema_version < expected`.
* **Embedding model lock-step**. Refuse replication if `embedding_model` differs. Coordinated upgrade story documented separately.
* **Entity-id determinism**. The migration MUST normalise existing entities to deterministic ids and rewire relations. Without this, replication produces orphan entities on Day 1.

## P1 risks (correctness)

* **Slug-id collision avoidance** — resolved by uid-as-canonical (D1).
* **Concurrent edit detection on items** — `parent_uid_hash` mismatch (D2).
* **Concurrent invalidate on relations** — symmetric detection (D6).
* **Initial bulk-import** — bootstrap endpoint (D10) avoids history replay.

## P2 risks (ops hygiene)

* Clock skew detection on announce; warn at >5s delta.
* Mesh fanout failure visibility; surface notification after N failed cycles.
* Cron drift on dream_role announce — same NTP assumption.

## P3 risks (deferred)

* Lease-based dream_role (third peer scenario).
* LLM-augmented Librarian gating model.
* Compaction of `mem_item_history` (size grows linearly with mutations).
* Bulk-import streaming for >50K rows.
* Schema-version downgrade story (only forward migration considered).

## Deliverable split

* **BLACKSTAR (Rollo)**: schema migration script (mem_item_history additions + new mem_replication_cursors + entity_id determinism + relations origin_machine_id), bootstrap endpoint with NDJSON streaming, deterministic_uid_for_extraction helper, slugify_lossy helper.
* **Mac (Rollo)**: replicator task (subscribes to bus, fanout via mesh), `broadcast_announce` extension, `/api/mem/events` receiver, `/api/mem/blob/<sha>` content-addressed endpoint, `/api/mem/replicate/since/{cursor}` cursor pull, `dream_role` config field + worker gating.

Both take ~200-400 lines of Rust + SQL + migration script. Self-loopback integration test: BLACKSTAR pairs with itself (alt-port instance) and exercises the full save→broadcast→apply→conflict path.

## Out of scope for v0

* Multi-peer (3+). Designed for 2; generalize when third peer pairs.
* Trust model beyond paired-peer signature. Future: per-skill ACL, signed-commit verification.
* `mem rewind` CLI. v0 only surfaces conflicts via notification; rewind is v1.
* Privacy: `shared_areas_blacklist` is the only knob. Per-item `shared:false` flag is v1.
* GDPR-style hard purge. v0 = tombstone propagation; hard purge is a separate command in v1.
* CRDT body merging. Per-item LWW only.
* Rqlite / Litestream evaluation. Decided: in-process replicator + SQLite history is the right grain for 2-peer trusted setup.

## Sequencing

1. PR this doc → develop.
2. Andrey reviews, "ship" or "rework".
3. On ship: both branch `fix/mem-replication-v0` from master.
4. BLACKSTAR's migration + bootstrap PRs first (no behaviour change without the mac side; safe to ship to develop).
5. Mac's replicator task PR depends on schema being on develop.
6. Self-loopback integration test green → enable replication via config flag (default off).
7. Andrey flips `integrations.mem.replication_enabled = true` on both peers when ready.

## Decisions remaining (asked Andrey, awaiting)

Andrey punted to us with «сами короч решайте». Defaults locked:

* Transport — existing `/api/mesh/relay`.
* Real-time vs eventual — eventual within 5min + mesh-push for live.
* Machine-specific — areas-list (`shared_areas_blacklist`) + per-item override (v1).
* Delete — tombstone propagation (hard purge v1).
* Dreamer — `auto` (workstation primary), Andrey overrides via config.
* Determinism rule — trigger × cost as in D9.
* `_dreams` — shared by default; `shared_areas_blacklist = ["_dreams"]` if Andrey wants local-only.

If Andrey wants to revisit any of these on review, the doc is the single revision target.

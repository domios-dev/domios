# Phase 6 — Chunk vector layer (sqlite-vec, reused from mem)

**Status**: plan
**Owner**: Mac-Air
**Depends on**: Phase 1 (chunk content lives next to nodes), Phase 2 (auto-build keeps vectors fresh), Phase 3 Step A (the search path).

## Goal

Make `codeterm map search` semantic. Instead of substring-only lookup
on `display_name`, embed every code chunk locally and run hybrid
search: vector top-k seed → expand by k-hop in the structural graph →
union with the substring path. So the user can ask "auth flow" or
"что делает декодер аудио" and we surface the right region of the
graph even if the query never appears as a literal symbol.

## Reuse, not new dependency

CodeTerm already ships everything we need:
- `fastembed` (ONNX, local, all-MiniLM-L6-v2 / 384 dims) — runs in the
  `codeterm-embed` sidecar binary (`src-tauri/src/bin/codeterm_embed.rs`).
- `sqlite-vec` extension — already in `cli/Cargo.toml` and used by `mem`.
- Tree-sitter parsers for chunk boundaries — already vendored for
  Phase 1.

LanceDB is **not** added. The plan that recommended it predated the
audit of in-repo infra. Andrey: "просто зареюзать и все, потом улучшим
если не будет хватать". Memory: `feedback_reuse_over_new_deps.md`.

## SOLID seam — don't break prod mem

The `mem` subsystem is production-critical. Phase 6 must NOT modify
its public API or its sidecar protocol. The reuse path:

- `mem::embeddings::inner::embed(text) -> Vec<f32>` stays untouched.
- The `codeterm-embed` sidecar binary stays untouched.
- CLI gets its **own** thin client that talks to the **same** binary
  over the **same** stdin/stdout JSON protocol. The protocol is the
  contract; no shared crate, no peeking inside `mem`. If Phase 6
  changes how it talks to the sidecar, the change is in CLI only.

This pins the seam at the binary's stdio interface. mem and map
become two independent consumers of the same sidecar — clean SRP.

## Files

```
cli/src/cmd/map/embed/mod.rs                    # NEW — facade
cli/src/cmd/map/embed/client.rs                 # NEW — EmbedChild client (mirror of mem::embeddings::inner::EmbedChild)
cli/src/cmd/map/embed/path.rs                   # NEW — sidecar path resolution
cli/src/cmd/map/chunks.rs                       # NEW — tree-sitter chunking
cli/src/cmd/map/store.rs                        # EDIT — load sqlite-vec, add chunk + chunk_vec tables (CREATE IF NOT EXISTS — keeps schema v1)
cli/src/cmd/map/index.rs                        # EDIT — call chunker + embed pipeline; degrade gracefully if sidecar unavailable
cli/src/cmd/map/search.rs                       # EDIT — hybrid search: vec top-k → graph expand → merge w/ substring
cli/src/cmd/map/mod.rs                          # EDIT — export embed module
cli/src/args/map.rs                             # EDIT — `map search --semantic-only`, `map search --vec-k N` flags
```

All new files ≤ 250 lines.

## Schema additions (still v1, additive)

```sql
CREATE TABLE IF NOT EXISTS chunk (
  id          INTEGER PRIMARY KEY,
  file_id     INTEGER NOT NULL REFERENCES file(id) ON DELETE CASCADE,
  start_line  INTEGER NOT NULL,
  end_line    INTEGER NOT NULL,
  contents    TEXT NOT NULL,
  embedded_at INTEGER             -- ms epoch; NULL if not yet embedded
);
CREATE INDEX IF NOT EXISTS chunk_file_idx ON chunk(file_id);

-- sqlite-vec virtual table — same pattern as mem_vectors in mem
CREATE VIRTUAL TABLE IF NOT EXISTS chunk_vec USING vec0(
  chunk_id INTEGER PRIMARY KEY,
  embedding FLOAT[384]
);
```

Adding tables is a backwards-compatible change to schema v1. We
already use `CREATE TABLE IF NOT EXISTS` everywhere, so an existing
v1 db just gains the tables on next open. No migration; no v2 bump.

When the indexer re-extracts a file, `delete_file_data(file_id)` now
also drops chunks for that file. CASCADE handles `chunk` rows; the
virtual table needs an explicit `DELETE FROM chunk_vec WHERE chunk_id IN
(SELECT id FROM chunk WHERE file_id = ?)`.

## Chunking strategy (cli/src/cmd/map/chunks.rs)

Tree-sitter AST-aware (Continue.dev pattern from
`references/continue/core/indexing/`):

1. Re-parse the file (we already do this for tags — could pipe the
   tree through, but a second parse on already-cached source is cheap
   compared to embedding cost).
2. Walk top-level definitions (function / class / method / module).
   Each becomes a chunk **if** it's between
   `MIN_CHUNK_LINES=4` and `MAX_CHUNK_LINES=80`.
3. For definitions larger than `MAX_CHUNK_LINES`, split at body
   boundaries — for now, simple line-window split with overlap.
4. For lines outside any def (top-of-file imports, free statements),
   batch into a single sliding-window chunk.

Soft cap: skip files larger than `MAX_FILE_BYTES = 200_000` (200KB)
to avoid OOM on generated bundles.

## Indexing pipeline integration

In `index_file()`, after the tag extraction + bridge signal write:

```rust
let chunks = chunk_file(wf.lang, &source, file_id);
for c in chunks {
    let chunk_id = store.insert_chunk(file_id, c.start, c.end, &c.text)?;
    pending_embeds.push((chunk_id, c.text));
}
```

After the file loop, before `derive_edges()`, drain `pending_embeds`
through the embed client in batches of `BATCH=8`. On embed error per
chunk, log to `error` table and leave `embedded_at=NULL`. The
substring search keeps working regardless.

If the sidecar can't be found / spawned, log warn and skip the embed
phase entirely. Tag indexing still completes. Vectors get filled in
on the next index run when the sidecar becomes available.

## Hybrid search

`map search` becomes:

```
codeterm map search "auth flow"
  → embed("auth flow")
  → vec0 nearest neighbours, top K=20
  → for each chunk, resolve to its enclosing node (by file_id +
    start_line falling within node range)
  → graph_expand(node, k_hops=1)  — Phase 3 GraphQuery::focus_subgraph
  → union with display_name substring matches
  → rank: vec_distance × 0.6 + structural_score × 0.4
  → return top `--limit`
```

Flags:
- `--semantic-only` — skip the substring path, vector + graph only.
- `--vec-k <N>` — override the vec0 top-k seed count.

If the embed pipeline failed (sidecar missing), `--semantic-only`
errors out cleanly; the default mode silently skips vectors and
falls back to the existing substring path. Always-available behaviour
preserved.

## CLI seam to the sidecar

`cli/src/cmd/map/embed/path.rs::resolve_embed_path()` order:
1. `CODETERM_EMBED_PATH` env override.
2. `<data_dir>/bin/codeterm-embed` (post-build copy).
3. Sibling of `current_exe()` named `codeterm-embed`.
4. `codeterm-embed` on `PATH`.
5. Error: not found (caller gracefully degrades).

`cli/src/cmd/map/embed/client.rs::EmbedClient`:
- Spawns the sidecar with stdin/stdout pipes.
- `embed(text) -> Vec<f32>` — line-delimited JSON request, parse
  response. Same protocol as `mem::embeddings::inner::EmbedChild`.
- Recycles the child every 200 calls (matches mem's bound on the ort
  Windows leak).
- `Drop` kills + waits child.

This is a **mirror**, not a fork. The protocol is documented in
`src-tauri/src/bin/codeterm_embed.rs` lines 1–11; if the protocol
ever changes (it shouldn't — it's stable contract), both clients
need to be updated. That's the trade-off vs sharing a workspace
crate, which we explicitly avoided to keep the SOLID seam at the
process boundary.

## Acceptance criteria

**Functional**
- `codeterm map index` on this repo produces non-zero `chunk` and
  `chunk_vec` rows. `chunk.embedded_at` populated for at least 95%
  of chunks.
- `codeterm map search "auth flow"` returns at least one node from
  `src-tauri/src/secrets/` or similar auth-touching code, even
  though the query never appears as a literal symbol.
- `codeterm map search "tauri command bridge"` returns
  `src-tauri/src/api/map_routes.rs` (Phase 3A code) in the top 5.
- Sidecar binary missing → tag indexing still finishes, exit 0.
  Empty `chunk_vec`. Substring search keeps working.

**Structural**
- `mem::embeddings` source unchanged. `codeterm-embed` source unchanged.
- `mem` test suite still green: `cargo test --features embeddings -p codeterm`.
- New CLI files ≤ 250 lines. `mod.rs` files ≤ 200.
- `./build.sh check` clean. `cd cli && cargo test` clean.

**Performance**
- Cold index of this repo: under 60s (current 9s without vectors;
  embedding ~2k chunks at ~20ms/chunk on this hardware = ~40s expected).
  If we exceed 90s, tighten chunk size or batch the sidecar protocol.
- Warm re-index hits the same mtime cache as Phase 1; chunks for
  unchanged files are skipped. Target: under 2s warm.
- Hybrid search query: under 250ms end-to-end (embed + vec0 + graph
  expand) on this repo.

**Tests**
- `chunks.rs`: tree-sitter chunking unit tests with inline source
  for Rust + TS + Python. Assert chunk count and line ranges.
- `embed/client.rs`: spawn a stub binary that echoes a fixed vector;
  assert protocol round-trip. Skip on platforms where the real
  sidecar is required.
- `search.rs`: when vec0 is empty, hybrid search degrades to
  substring (current behaviour). Add a fixture index where vec0 has
  one row and verify it influences rank.

## Risks / open questions

1. **Sidecar dependency at index time.** If the daemon hasn't built
   the embed binary yet, the indexer prints a warn and skips
   vectors. Acceptable; users see substring search until the daemon
   is rebuilt.
2. **384-dim vs 768-dim.** mem ships with all-MiniLM-L6-v2 (384). We
   reuse the same. If we ever want a larger model, both consumers
   migrate together by changing the sidecar.
3. **Schema additivity.** Adding `chunk` + `chunk_vec` to a v1 db is
   safe via `IF NOT EXISTS`. Confirmed against existing v1 dbs in
   manual smoke.
4. **Keeping vectors fresh.** The mtime cache hit path skips embed
   too. So changing a file → cache invalidates → chunks re-extracted
   and re-embedded automatically. The same Phase 2 watcher already
   triggers this.
5. **Long files.** `MAX_FILE_BYTES=200KB` skips generated bundles
   (e.g. `Cargo.lock`, large JSON). Indexer logs `skipped_size: N`
   to errors. v1.2 acceptable.

## Out of scope (Phase 6)

- Reranker stage (mem already has a cross-encoder reranker; we may
  reuse later, but for v1 the vector top-k is good enough).
- Frontend search box / overlay → Phase 3.5 (left rail).
- Embedding upgrades (768-dim, multilingual) → backlog.

## Done definition

- All acceptance criteria met.
- mem subsystem regression: `cargo test --features embeddings -p codeterm` green.
- Plan + code committed on `feat/project-map`.
- `codeterm agent report "phase 6 vector layer done: …"`.

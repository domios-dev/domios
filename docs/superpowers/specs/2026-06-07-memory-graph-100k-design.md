# Memory Graph: 100k-Node Viewer + Web-Mirror Fetch Fix

## Problem

1. **Bug:** The Memory panel fails with `TypeError: Load failed` in the web mirror.
   `memoryApi.ts#baseUrl()` hardcodes `http://${api.host}:${api.port}` from the
   daemon's `ApiConfig` — in web mode that resolves to `127.0.0.1` *on the
   viewing device* (e.g. an iPhone), so every fetch hits the phone itself and
   dies at the network level. HTTPS mirrors additionally hit mixed-content
   blocking.

2. **Scale:** The graph inspect view loads the entire graph via
   `GET /api/mem/graph/full` (no limits) and lays it out with a client-side
   force simulation. Fine at ~600 nodes; at 100k nodes the layout takes
   minutes, the JSON payload reaches tens of MB, and edge/label rendering
   kills FPS — especially on mobile.

## Goals

- Memory panel fully functional in the web mirror (graph, list, dreams,
  activity, workers).
- Graph inspect view stays responsive up to ~100k nodes / ~300k edges on
  desktop and usable on a phone.
- Layout is computed once on the backend, cached on disk, and updated
  incrementally — the client only renders.
- Zero DB changes: no migrations, no new tables, no writes to SQLite. The
  layout cache lives in a sidecar file next to the DB.

## Non-Goals

- Map-tile server / server-side rendering (overkill below ~500k nodes).
- Client-side WASM dimensionality reduction.
- Keeping the physics simulation as a fallback path for small graphs.

## Part 1 — Web-mirror fetch fix (independent, ships first)

`src/components/MemoryPanel/memoryApi.ts`:

- `baseUrl(api)`: when `IS_WEB` (from `src/lib/webApi.ts`), return `apiHost()`
  (same-origin or peer-picker target); otherwise keep the current
  `http://${host}:${port}` construction for Tauri.
- `authHeaders(api)`: when `IS_WEB`, use `currentToken()` (per-host web token)
  instead of `api.token`.

All `baseUrl` consumers are fixed by this single change: `MemoryCanvas`,
`MemoryListTab`, `MemoryDreamsTab`, `MemoryActivityTab`, `useActivityPulses`,
`useGraphWorkers`. Audit `useAgentRunStream` / `AgentRunLogPopup` for the same
hardcoded pattern and route them through the shared helpers.

## Part 2 — Scalable graph architecture

**Principle: backend computes positions from embeddings; the client only
renders.** Physics is removed entirely — one pipeline for every graph size.

### Layout semantics

Node positions come from a 2D projection of the item's embedding vector
(`mem_vectors` already stores one per item). The map is *semantic*: similar
memories cluster spatially. Graph relations are drawn on top of the semantic
layout but do not drive it. Entities without vectors are placed at the
centroid of their related items' positions (plus jitter).

### Backend

**1. Sidecar layout cache file** — no SQLite changes at all. Positions live
in `<mem-dir>/graph_layout.json` next to the DB (same precedent as body
blobs):

```json
{
  "layout_version": 3,
  "computed_at": "...",
  "positions": { "<mem_id>": [x, y, cluster_id], ... }
}
```

- Written only via atomic replace (write tmp file, `rename`) — the cache is
  always either the complete old state or the complete new state.
- Old binaries don't know the file exists → rollback is a no-op. Corrupt or
  version-mismatched file → delete and rebuild. No migration, ever.
- The daemon holds the layout in RAM; incremental placements mutate the
  in-memory map and flush to disk debounced (e.g. 5s). A crash loses only
  recent increments — recoverable by k-NN re-placement or full rebuild.
- Deleted items need no cascade: stale ids are filtered at read time and
  pruned on the next full rebuild.

`graph_pinned_positions` (user drag-pins) already exists in SQLite, stays
as-is, and overrides computed positions at read time. Note: legacy
`graph_x`/`graph_y` columns on `mem_items` were deliberately dropped in an
earlier migration — persisted positions deliberately stay out of `mem_items`.

**2. Full layout job** — background worker (same pattern as the Dreamer):
reads all primary-chunk vectors from `mem_vectors` (skip `*_chunk_N` ids),
projects to 2D via `annembed` 0.1.6. **Spike-validated** (2026-06-07):
604 real vectors → 0.3 s; 100k synthetic → 80 s / 1.55 GB peak RSS;
2D neighbor purity retains ~80% of the 384-dim structure. Pipeline:
`Hnsw<f32, DistCosine>` → `kgraph_from_hnsw_all` (nbng=15) →
`Embedder` with `asked_dim=2` → `get_embedded_reindexed()`.
Build notes: `ndarray = "0.17"` pin is load-bearing; macOS uses the
`macos-accelerate` feature, Windows/Linux use the default pure-Rust path.
Cluster ids assigned by grid/density bucketing of the projected points.
The job bumps `layout_version` and atomically replaces the sidecar file.

Triggers:
- on startup, if `graph_layout` is empty and vectors exist;
- when the share of vectorized items without a position exceeds a threshold
  (e.g. 20%);
- manually via `POST /api/mem/graph/layout/rebuild`.

**3. Incremental placement** — on item save (after embedding): position =
distance-weighted average of the k nearest neighbors' positions (k≈8, via the
existing vector index) + small jitter; written into the in-memory layout map
(flushed to the sidecar file debounced). One k-NN query per new item; no
global recompute.

**4. Snapshot API** — replaces `GET /api/mem/graph/full` (no external users;
delete the old endpoint):

- `GET /api/mem/graph/snapshot` → `{ layout_version, nodes, clusters, stats }`
  - node: `{ id, x, y, kind, importance, cluster_id, flags }` — **no
    title/summary** (labels load lazily).
  - `clusters`: `{ id, x, y, size, label? }` aggregates for zoomed-out
    rendering, plus aggregated cluster→cluster edge weights.
- `GET /api/mem/graph/snapshot?since_version=N&since=<ts>` → delta:
  added/moved/removed nodes only. If `layout_version` changed, the client
  refetches the full snapshot.
- `GET /api/mem/graph/labels?ids=a,b,c` → `{ id, title }[]` for the current
  viewport.
- `GET /api/mem/graph/edges?ids=a,b,c` → full edges touching the given nodes
  (viewport / selection).
- Responses gzip-compressed (tower-http `CompressionLayer`). Budget: 100k
  nodes ≈ 4 MB JSON ≈ <1 MB gzipped.

### Frontend

**5. Renderer** — Sigma.js stays (WebGL handles 100k nodes). Deleted:
`physicsSim.ts`, `physics.worker.ts`, `physicsProtocol.ts`,
`useGraphLayout.ts`, `louvainLayout.ts`, `seedLayout.ts` and their tests.
Positions come from the snapshot as-is.

**6. Zoom LOD** (Sigma reducers + data layering):
- zoomed out: cluster aggregates + top-importance nodes only; aggregated
  cluster edges; no labels.
- zoomed in: all nodes in viewport; labels fetched for visible high-rank
  nodes; real edges fetched for the viewport.
- selection: ego edges for the selected node always shown.
- Activity pulses (`mem-activity` events) keep working as today.

**7. Client cache** — snapshot persisted in IndexedDB keyed by
`layout_version`: panel opens rendering the cached snapshot instantly, then
fetches a delta (`since_version`/`since`) in the background. The existing
800ms-debounced `mem-activity` refresh becomes a delta fetch instead of a
full reload.

**8. Drag = pin** — unchanged (`graph_pinned_positions`).

### Data safety (priority requirement)

- **Zero SQLite changes**: no migrations, no new tables, no new writes. The
  DB cannot be broken by this feature because nothing touches it.
- Sidecar cache safety tests:
  1. atomic write — kill mid-flush, file is either old or new, never torn;
  2. corrupt/garbage file → loader rejects, schedules rebuild, daemon stays up;
  3. version mismatch (`layout_version` unknown) → full rebuild;
  4. stale ids in cache (items deleted from DB) → filtered at read time.

### Testing (beyond migrations)

- Rust: layout job unit tests on synthetic vectors (clusters stay separated,
  deterministic given a seed); k-NN placement test (new node lands near its
  neighbors); snapshot/delta endpoint tests.
- Frontend: LOD reducer logic unit tests; IndexedDB cache version-mismatch
  test (stale version → full refetch).
- Bench: synthetic 100k-vector DB fixture → measure layout job wall time,
  snapshot payload size, and first-render time; record in the PR.

## Rollout order

1. Part 1 fetch fix (small PR, fixes the live bug).
2. Sidecar layout cache + layout job + snapshot API (old viewer still on
   `/graph/full`).
3. Frontend switch to snapshot + LOD + IndexedDB cache; delete physics and
   `/graph/full`.

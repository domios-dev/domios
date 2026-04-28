# Archivarius cross-pollination

Findings from a 5-agent parallel audit comparing CodeTerm against
`/Users/rollacode/Developer/archivarius` (VectorDocs — RAG document management,
Spring Boot + React). Two items stood out as concrete, near-term wins and are
detailed below. A longer idea list lives at the end.

## 1. OpenAPI contract + codegen

### Problem

CodeTerm exposes the same operations through three transports (REST, Socket,
Tauri IPC) plus a CLI and a mesh relay, but has no machine-readable contract
binding them together:

- **Backend:** every Axum handler in `src-tauri/src/api/routes.rs` and
  `src-tauri/src/api/mem_routes.rs` is hand-written. Response shapes exist only
  as Rust structs (`MemItem`, `WorkspaceInfo`, `GraphWorkerInfo`, ...).
- **Frontend:** `src/components/MemoryPanel/memoryApi.ts` and friends are
  hand-typed `fetch` calls that mirror backend shapes. Add a field on a struct
  and TS breaks only at runtime.
- **Docs:** `docs/API.md` is a hand-maintained markdown table with a revision
  counter. It drifts every few commits.
- **CLI:** `cli/src/mesh_cli.rs` manually maps commands to `{method, path, body}`
  tuples. Each new route is another sync point.
- **Mesh relay:** accepts any `{method, path, body}` from peers and forwards it
  unchanged. Bad payloads fail 500 inside the service instead of 400 at the
  edge.

Archivarius solves this contract-first: `openapi/schema.json` is the source of
truth, both Java backend interfaces (`DocumentController implements
DocumentsApi`) and TypeScript types (`frontend/src/types/api.ts`) are generated
from it.

### Goal

One source of truth (`openapi.json`) for every REST operation, generated types
for TS, a served Swagger page for humans, and schema validation at the mesh
boundary.

### Plan

**Step 1 — annotate Rust handlers with `utoipa`**

- Add to `src-tauri/Cargo.toml`:
  - `utoipa = "4"`
  - `utoipa-axum = "0.1"`
  - `utoipa-redoc = { version = "4", features = ["axum"] }`
- Derive `ToSchema` on all DTO structs exposed via REST:
  - `MemItem`, `MemStats`, `MemChunk`, `GraphWorkerInfo`, `GraphEntity`,
    `GraphRelation`, `WorkspaceInfo`, `TabInfo`, `PaneInfo`, `KeepAwakeStatus`,
    `CronJob`, `CronRunInfo`, ... (~30 total)
- Annotate each handler with `#[utoipa::path]` including path, method, params,
  request/response bodies, tags. Organize by tag: `mem`, `workspace`, `pane`,
  `cron`, `keepawake`, `mesh`, `system`.

**Step 2 — serve the spec**

- New route `GET /api/openapi.json` returning the generated spec.
- `GET /api/docs` serving Redoc UI pointed at the spec (via `utoipa-redoc`).

**Step 3 — TS codegen**

- Add `@hey-api/openapi-ts` as a devDependency.
- `package.json` script: `"gen-api": "openapi-ts -i
  http://127.0.0.1:7686/api/openapi.json -o src/api/generated -c
  @hey-api/client-fetch"`.
- Configure the generated client to inject `Authorization: Bearer $token` via
  middleware, so callers stay the same.

**Step 4 — migrate frontend callers**

- Replace `src/components/MemoryPanel/memoryApi.ts` handwritten functions with
  imports from `src/api/generated/services`.
- Replace the raw `fetch` usage in `src/components/MemoryPanel/*` hooks and the
  `MemoryScheduledTab.tsx` cron calls. Types come for free.
- `src/store/multiplexer.tsx` can stay on `invoke<T>` for Tauri IPC — that's
  not a REST path. No change.

**Step 5 — tighten mesh**

- In `mesh::routes::mesh_relay`, before forwarding, deserialize the incoming
  body into the target operation's typed request (via a `Command` enum generated
  from the spec, or by routing through the existing `dispatch::dispatch`).
  Reject with 400 at the edge if it doesn't fit.

**Step 6 — shrink docs/API.md**

- Keep the high-level overview and the "how auth works" section.
- Delete the endpoint-by-endpoint table; point users at `/api/docs`.
- Drop the revision counter — OpenAPI is versioned.

### Files touched

- `src-tauri/Cargo.toml` — 3 new deps
- `src-tauri/src/api/routes.rs` — annotations on ~50 handlers
- `src-tauri/src/api/mem_routes.rs` — annotations on ~30 handlers
- `src-tauri/src/api/mesh_routes.rs` — annotations
- DTO structs — `derive(ToSchema)` on ~30 types
- `src-tauri/src/lib.rs` — register `/api/openapi.json` + `/api/docs` routes
- `package.json` + `package-lock`/`bun.lockb`
- `src/api/generated/` — new directory (gitignored? or committed for IDE
  autocomplete? commit it)
- `src/components/MemoryPanel/memoryApi.ts` — trim or delete
- `src/components/MemoryPanel/*` — swap fetch → generated client calls
- `src/components/MemoryPanel/MemoryScheduledTab.tsx` — same
- `src/hooks/useAgentProviders.ts` — same
- `docs/API.md` — shrink
- `docs/plans/archivarius-crosspollination.md` — this file (mark done after)

### Effort & risks

- **Effort:** 1.5–2 days. The long tail is ~80 handler annotations; each is
  5–15 lines but mostly mechanical.
- **Risks:**
  - `utoipa` macros may not compose cleanly with existing `#[tauri::command]`
    macros on handlers that are dual-use. Worst case, split each dual-use into
    two thin wrappers.
  - Generated TS client's auth-header pattern may not match our `api.token`
    config path — may need a small `openapi-ts` config with a runtime hook.
  - Mesh relay revalidation is an optional final step; safe to ship the
    annotations + codegen first and add validation in a follow-up.

### Payoff

- Any backend schema change breaks the TS build immediately. A whole class of
  drift bugs disappears.
- New contributors can open `/api/docs` to explore the API — no stale `API.md`
  to read.
- The mesh gains input validation for free.
- Remote agents on other machines can consume the spec and generate their own
  clients in any language.

---

## 2. Header-aware chunking

### Problem

`src-tauri/src/mem/embeddings.rs::chunk_text` splits item bodies into ~800-char
chunks using this cascade: paragraph → sentence → word → char. It's correct for
prose. But most `kind=document` and `kind=skill` items are markdown with
headers, and the current chunker ignores them completely.

Example — a SKILL.md body that includes a `## Logs` section followed by `##
Troubleshooting`: today the 800-char window can split mid-section, and the
embedding for that chunk is "average of 3 unrelated topics". A query like
`"where are the logs"` then matches the mixed chunk weakly.

Archivarius solves this in `HeaderAwareChunker.java`: split on `#` first, carry
the header path into each chunk's embedding text, then fall back to the usual
paragraph/sentence cascade within a section. Anthropic's own research (which
Archivarius cites) shows this pattern combined with contextual enrichment
reduces retrieval error by ~35%.

### Goal

Markdown-aware chunker that preserves the document's human-written semantic
structure in the embedding space. Plain-text items (facts, learnings, notes
without headers) keep the existing behavior — no regressions.

### Plan

**Step 1 — detection**

- Check if body contains any `^#{1,6}\s` header lines (multiline regex).
- If not — current `chunk_text` is used unchanged. Fast path.

**Step 2 — header-aware splitter**

- Parse the markdown into a linear list of `(header_path: Vec<String>, body:
  String)` sections. `header_path` is the stack of enclosing headers: `["# Dev
  Run", "## Logs"]`.
- For each section, emit one or more chunks. Each chunk's **embedding text** is
  the joined header path on its own line, blank line, then a paragraph/sentence
  slice of the body no longer than `target_size - len(path_prefix)`.
- The chunk's **display text** is just the body slice (so Memory Panel search
  results and `mem get` look normal — no `#` prefix noise).

**Step 3 — schema**

- `MemChunk` already has `body`/`text` for the display side. Add an
  `embedding_text` column (nullable, falls back to `body` if null).
- Migration: add the column, leave existing rows as-is.

**Step 4 — reindex**

- For every item where the body contains headers, clear its vectors and call
  `queue_embed` again. A CLI `codeterm mem reindex --kind document,skill --only
  markdown` triggers this. Or a one-shot migration step that runs after the
  schema migration.

**Step 5 — tests**

- Inline in `embeddings.rs`:
  - pure-markdown with 3 H2s → 3 chunks, each carrying its H2 path
  - markdown with H1→H2→H3 nesting → chunks carry the full stack
  - long section under one header → multiple chunks, all with the same path
  - no headers at all → falls back to current chunker, same output
  - mixed (short intro, then headers) → intro as one chunk with empty path,
    sections as above

**Step 6 — evaluate**

- Run `evals/mem-recall/orphan-bench.sh` for sanity.
- Extend `evals/mem-recall/benchmark.json` with 3–5 test cases targeting
  headered documents (a seed SKILL.md, queries that match specific sections).
- Compare `mem-bench` recall@5 and MRR against the current baseline.

### Files touched

- `src-tauri/src/mem/embeddings.rs` — new `chunk_markdown` function (~80 lines),
  switch `chunk_text` to delegate when markdown is detected. Unit tests in the
  same file.
- `src-tauri/src/mem/store.rs` — schema migration adding
  `mem_vectors.embedding_text` column.
- `src-tauri/src/mem/embed_queue.rs` — use `embedding_text` when present.
- `cli/src/main.rs` — new `codeterm mem reindex` subcommand (or add to
  `sync`).
- `evals/mem-recall/benchmark.json` — 3–5 new header-targeting cases.

### Effort & risks

- **Effort:** 0.5–1 day coding + tests. Half a day to reindex the dev DB and
  validate via `mem-bench`.
- **Risks:**
  - Markdown parsing edge cases (code fences with `#` inside, headers inside
    blockquotes). Treat `^#{1,6}\s` as a strict signal; don't over-engineer.
  - Old vectors get stale until reindex. Mitigation: ship the reindex CLI in
    the same release and call it once from the dev harness.
  - The extra `embedding_text` column doubles the body payload for documents
    that have both a header prefix and the body. Impact is tiny (< 100 chars
    per chunk), accept.

### Payoff

- Measurable recall improvement on all `kind=document` and `kind=skill` items
  that have markdown structure — which is most of the bundled ones.
- No regressions on flat prose items.
- Sets up the foundation for Anthropic-style contextual enrichment later (the
  same `embedding_text` column can carry LLM-generated context alongside the
  header path).

---

## Other ideas from the audit (for the backlog)

Backlogged, not detailed here. Listed by impact × effort.

- **Per-intent model routing** in `spawn_headless` — haiku for extraction,
  sonnet for dreaming, opus for bootstrap. ~10 lines in `mem/workers.rs`. High
  ROI: immediate cost reduction on the dominant spawn path.
- **Langfuse observability** for headless workers — current `log::info!` only,
  no per-prompt cost/latency visibility. Noticeable pain when extraction
  quality regresses and there's nothing to look at.
- **YAML prompts with provider.model variants** — let `mem_extraction.md`
  ship a shorter haiku variant, long opus variant. Requires a prompt loader
  refactor in `prompts.rs`.
- **Contextual enrichment pre-embed** for `kind=document` > 2 KB. LLM adds a
  ~100-word chunk-position context before embedding. Per Anthropic: −35 %
  retrieval error. Plays well with header-aware chunking above.
- **Newtype IDs** (`PaneId`, `WorkspaceId`, `PtyId`) — makes mixups compile
  errors. Mechanical refactor, no behavior change.
- **`rusqlite_migration`** versioned SQL files replacing `PRAGMA
  table_info`-based schema branching in `store.rs`.
- **Pre-search before dreaming** — inject top-N mem hits into the dream
  worker's task prompt to skip the first LLM turn.
- **React Query** for server-state (mem items, crons, providers), keep the
  Context reducer for client-state (panes, splits).
- **Declarative settings via OpenAPI** — once the spec exists, the Settings
  UI can consume a `SettingField` JSON schema the way Archivarius's
  `ProviderDescriptor` does.

## Ordering recommendation

1. **Header-aware chunking** — isolated, one file + tests, measurable win.
2. **Per-intent model routing** — 10 lines, cost reduction.
3. **OpenAPI + codegen** — architectural win, touches many files.
4. **Langfuse observability** — after #3, simpler to thread traces through a
   typed surface.

Everything else stays backlog until these land.

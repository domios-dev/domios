# CodeTerm Memory System

Persistent memory for agents. Survives restarts, provider switches
(Claude ↔ Codex), and CodeTerm upgrades. Agent-agnostic.

## TL;DR

- **Storage** — single SQLite DB at `~/.codeterm/mem/knowledge.db` + body blobs on disk
- **Search** — FTS5 + sqlite-vec hybrid with RRF fusion
- **Graph** — temporal entity-relation triples (people, projects; NO tech nodes)
- **L1 selection** — pinned items first, then ranked by `importance + log10(1+evidence)*0.5 + ws_match*0.5 + recent_30d*0.3`; budget defaults to 300 tokens (`[memory] l1_budget_tokens`)
- **Pins** — `mem pin <id>` / `mem unpin <id>` set `is_pinned_wakeup`; pinned items always load in L1 (subject to budget)
- **Workers** — hidden headless agents that extract entities, build graph, dedup; `mem worker spawn` for ad-hoc enrichment
- **Skills** — folder skills (`SKILL.md` + scripts) registered via `mem skill new/add`, indexed alongside other items
- **Librarian** — periodic file-sync daemon: re-vectorizes source-tracked items when their on-disk file changes, flags deletions
- **Dreaming** — nightly consolidation at 6 AM with 0..30 min delay-only jitter: four SQL phases (light, deep, normalize, calibration) run server-side in `POST /api/mem/dream`, then the LLM REM worker is spawned to handle structural cleanup + diary; the REM prompt is read-only with respect to calibration (sweep already ran)
- **Lifecycle capture** — `SessionStart` tracks the active session; optional `PreCompact` export POSTs to the authenticated memory endpoint only after separate local-user consent
- **Protocol** — 7 behavioral rules injected into every agent session via `bridge_mem.md`

## Mem vs past-session search

Two stores, two purposes — don't confuse them:

| Surface | Stored where | Asks | Used for |
|---|---|---|---|
| **mem** (`~/.codeterm/mem/knowledge.db`) | SQLite + FTS5 + sqlite-vec | "what did the agent learn / decide / save?" | Curated facts, learnings, skills, conversations the agent chose to keep |
| **sessions** (provider-owned files) | Claude JSONLs / Codex rollouts / OpenCode's sqlite store | "what did we talk about that one time?" | Raw conversation history; on-demand search via `codeterm sessions search`, atomic resume via `codeterm sessions resume` |

Mem is curated and indexed; sessions are raw and provider-owned. The two never share storage. See `codeterm docs API-STATE` for the session API; the binding model lives in the [core essence](../essences/core.md#agents-and-providers).

## Storage layer

### Schema (`src-tauri/src/mem/store.rs`)

**`mem_items`** — every memorable thing
- `id` — slug-derived (`fact_pgbouncer_config`, `skill_deploy_staging`)
- `kind` — `fact` | `learning` | `note` | `conversation` | `skill` | `rule` | `preference` | `peer` | `daily` | `summary`
- `scope` — `global` | `workspace:<id>`; any other value is refused
- `area` — project/topic tag (`auth`, `billing`, `deploy`, `home`, `_dreams`)
- `tags_json` — free-form tags
- `body_inline` (<2KB) / `body_blob_path` (larger → disk blob)
- `importance` — 1-5 float, primary L1 score term
- `evidence_count` — bumped on `get()` for `skill`/`lesson`/`rule`/`fact`/`preference`; secondary L1 score term (skipped when caller sets `X-CodeTerm-Worker-Id` so dreamer sweeps don't feedback)
- `is_pinned_wakeup` — `0`/`1` flag; pinned items always load in L1 wake-up (subject to budget)
- `access_count` — incremented on every `get()` call; used by dreaming promotion
- `accessed_at` / `created_at` / `updated_at`
- `recurrence_count` — incremented when `mem save` finds an unchanged normalized body for the same kind and title

`mem save` uses kind plus title as its identity. An unchanged normalized body increments `recurrence_count` and schedules no extraction. A changed body updates the existing row in place and adds the prior body to history. `--replace` also updates in place and preserves the row's uid and creation time.
- `graph_x` / `graph_y` — persistent graph layout position
- `seed_fingerprint` — `NULL` unless the item came from a bundled seed (`codeterm-mem/src/seed.rs`). A SHA-256 over every field the seeder manages (kind, scope, title, summary, area, tags, body, importance, source_path, pinned — `source` itself is excluded, it's just the provenance label). `seed_bundled` overwrites the item ONLY when this still matches a freshly computed fingerprint of the stored payload — proof nothing has touched it since the seeder last wrote it. `store::update` clears it to `NULL` atomically, in the same write, whenever any managed field actually changes, so a real edit permanently opts the item out of future bundled refreshes. Never inferred from `mem_item_history`/`changed_by` — those are audit telemetry, not overwrite authority.

Pin-only updates emit a row in `mem_item_history` so the audit trail captures explicit user intent (not just body edits).

**`mem_fts`** — FTS5 virtual table (title + summary + body + tags), auto-sync on insert/update.

**`mem_vectors`** (sqlite-vec) — 384-dim embeddings from fastembed (all-MiniLM-L6-v2). Local, offline, free.

**`entities`** — graph nodes (`person_*`, `project_*`). Technologies intentionally NOT entities (they create star patterns).

**`relations`** — temporal triples (`from_id`, `rel`, `to_id`, `valid_from`, `valid_to`, `confidence`). Supports point-in-time queries.

### Why SQLite over ChromaDB / Neo4j

One file, no daemon, WAL mode for concurrent reads, 281 TB max size. Bodies >2KB go to content-addressed blobs (hash-indexed) to keep the DB file small.

## Search & retrieval

Three layers (matching a reference memory system's L0-L3 pattern):

| Layer | What | When | Size |
|---|---|---|---|
| **L0 — Identity** | `~/.codeterm/general/identity.md` | Injected into every GA init | ~100 tok |
| **L1 — Wake-up** | Pinned items + ranked by score (see below) | `codeterm mem wake-up` (GA runs on init) | ~300 tok (config: `[memory] l1_budget_tokens`) |
| **L2 — Targeted** | `search --area X --kind Y` | On-demand when topic comes up | variable |
| **L3 — Hybrid** | FTS5 BM25 + vector recall + RRF fusion | `codeterm mem search` / `recall` | variable |

### Hybrid ranking (`src-tauri/src/mem/search.rs`)

Ported from a reference memory system's `hybrid_v3`:

```
fused = bm25 * (1 - 0.30 * keyword_overlap) * (1 - temporal_boost)
```

- **BM25** — FTS5 native ranking (lower = better)
- **keyword_overlap** — fraction of query terms (≥3 chars, minus stop words) literally present in title/body/tags
- **temporal_boost** — if query mentions a time anchor ("last week", "2 weeks ago"), items near (now - offset) get up to 40% distance reduction
- **importance bump** — highly-curated items (importance ≥4) get up to 20% boost

Vector recall runs in parallel. RRF fusion merges both sources:
```
rrf_score(item) = Σ 1 / (k + rank_in_source)
```

### Decision re-rank

An optional second stage over `mem search`. Stage 1 is the unchanged hybrid recall, widened to a candidate pool; stage 2 has a registered decision adapter grade each candidate and re-sorts the hits by that absolute relevance. It is off by default (`[memory.decision_rerank].enabled`); `codeterm mem search --decision-rerank on|off` overrides the config for one request, and an absent flag leaves the decision to config. The legacy `--rerank` is exclusive with `--decision-rerank on` (the request is refused as invalid).

- **Hits.** An applied search adds a `relevance` fraction (0..1) to each hit, shown as `rel 92%` next to the score in the human render.
- **Envelope.** `--json` returns the response unchanged plus a `decision_rerank` object: `status` is `applied` (with `adapter`, `low_confidence`, `max_relevance`), `fallback` (an adapter failure, deadline or missing adapter; plain recall hits are returned with a `reason`), or `skipped` (`reason: legacy_rerank`). The human render prints one line for fallback/skipped with the reason. Without the key the output is exactly the plain recall render.
- **Graph source.** `[memory.decision_rerank.sources]` sets the pool: `recall` hits (default 30) plus up to `graph` extra candidates (default 3; `0` turns the graph source off). Graph candidates are live mem items linked by an active relation to an entity whose id contains a query keyword, ranked by how many matched entities they link to, then importance and recency; they are found by SQL alone, never by a model or embedding. Items already in the recall pool are not added twice. With `graph > 0` an applied search tags every hit `via: "recall"|"graph"` and the envelope adds `sources: {recall, graph}` (pool counts). With both `graph = 0` and `docs = 0`, applied output carries neither key; every fallback also carries neither and matches the plain recall contract.
- **Docs source.** `[memory.decision_rerank.sources].docs` adds up to 10 embedded documentation sections (default 3; `0` turns the docs source off), selected from one in-memory BM25 index built once. It includes `prod-docs` and the generated CLI reference as `CLI`, chunked by command heading. Applied hits use `{source: "doc", doc, section, anchor, pointer, relevance, via: "docs"}`; prod-docs point to `codeterm docs <NAME>` and CLI sections point to `codeterm <command> --help`. The envelope's `sources` counts include `docs`. The CLI reference remains governed by `docs_cli_md_is_fresh`. The index never reads memory items or records access. Docs candidates appear only in applied output; off and fallback results retain the plain recall contract.
- **Sessions source.** `[memory.decision_rerank.sources].sessions` adds up to five recent matching snippets (default 0; `0` disables the source), using the same in-process provider search as `codeterm sessions search`. The snippet sent to the adapter contains its title, provider, date, and matched excerpt, capped by `max_body_chars`. Search runs under the rerank deadline and skips the source on error, timeout, or worker saturation. Applied hits use `{source: "session", session_id, provider, title, excerpt, pointer, relevance, via: "sessions"}` and point to `codeterm sessions get <id>`. Session and docs hits never record mem access. They appear only in applied output; off and fallback results retain the plain recall contract.
- **Access.** Pool candidates, recall or graph, record no access while they are graded. Only the hits the search returns count as accessed, once each.
- **Low confidence.** `low_confidence: true` means even the best hit scored below `low_confidence_threshold`: nothing relevant was found. The human render says so in one line; treat the hits as weak leads, not answers.
- **Token.** Adapter tokens never enter `config.toml` or argv. `printf %s "$TOKEN" | codeterm mem decision token set --adapter typesafe-jev` (or `pbpaste | ...`) reads stdin only and refuses a terminal; `token clear`, `token status [--adapter]` and `status` show presence, path and `mode_ok`, never the value. The token lives in `<data dir>/decision-models/<adapter>.json` (owner-only), so each instance keeps its own: `~/.codeterm/` for prod, `~/.codeterm-dev/` for dev. These commands use bearer HTTP on the local machine and never cross `--mesh`.

Keys and bounds: `codeterm docs RUNTIME-CONFIG`. Routes: `codeterm docs API-MEMORY`.

### L1 selection (`src-tauri/src/mem/layers/`)

Two-pass selector. L0 identity always loads first; L1 then runs:

**Pass 1 — pinned set** (`LIMIT 50`)
Items with `is_pinned_wakeup = 1` in scope (global, plus current workspace if `--workspace` is set), ordered by `importance DESC, evidence_count DESC, updated_at DESC`. Pinned items render before any ranked item inside their kind section, prefixed with `★`.

**Pass 2 — ranked unpinned** (`LIMIT 100`)
Auto-extracted noise (`tags LIKE '%auto-extracted%'`) and the identity slug are excluded. Remaining candidates are scored in Rust:

| Term | Weight | Notes |
|---|---|---|
| `importance` | 1.0 | 1–5 float on the row |
| `log10(1 + evidence_count) * 0.5` | 0.5 | Saturates fast — prevents one mega-hub from dominating |
| `ws_match` | +0.5 | True when `scope = workspace:<current>` and `--workspace` is set |
| `recent_30d` | +0.3 | True when `updated_at > now() - 30d` |

Final ordering: `score DESC, id ASC` (deterministic for tests / `--explain`).

**Render** — grouped by kind in this priority: rules → preferences → peers → facts → summaries → skills → learnings. Within each kind: pinned first, then ranked. Headers and lines are dropped in reverse kind-priority once the budget is hit.

**Pin precedence** — pins beat score. A pinned `note` outranks an unpinned `fact` with `importance=5`. Pins are user intent; score is a heuristic.

**Eviction warning** — if even the pinned set alone overflows `l1_budget_tokens`, the rendered output starts with an HTML comment:

```
<!-- WARN: 4 pinned items did not fit L1 budget 300 -->
```

and a `log::warn!` fires (`[wake-up] pinned-eviction`). Action: shrink pins (`mem unpin <id>`) or raise the budget. Pins are user intent, so the system never silently drops them — it surfaces the conflict.

**Workspace scoping** — `codeterm mem wake-up` inherits `CODETERM_WORKSPACE_ID` from the calling tab. It loads global items + items pinned/scoped to that workspace, and the `ws_match` score term boosts workspace-local rows by 0.5. `--workspace <ws>` explicitly overrides the caller; without either context, only `scope = global` items participate.

## Graph layer

**Anti-star pattern:** only people and projects are entities. Technologies are searchable via FTS in body text — no graph nodes. This avoids the cascade where popular tech (React, Postgres) become mega-hubs.

**Canonical names:** dreaming worker merges name variants (Tima/Timofey → `person_tima`).

**Temporal validity:** relations have `valid_from`/`valid_to`. Dreaming's deep phase expires stale relations rather than deleting them.

**API:**
- `codeterm mem graph create --id <id> --entity-type Person --scope global` (idempotent)
- `codeterm mem graph relate --from <id> --rel mentions --to person_tima --reason "dreamer-bridge"`
- `codeterm mem graph related --id <id>` — outgoing relations

### Self-repair primitives (`src-tauri/src/mem/graph_repair.rs`)

The Dreamer cleans historical pollution (duplicate relations, near-duplicate entities, orphan vectors) via typed primitives only — no raw SQL surface for the LLM. Every destructive op writes a full payload to `graph_purge_log` for reversibility.

| CLI | What it does | Audit-logged |
|---|---|---|
| `mem graph find-duplicate-relations --group-by …` | Read-only: list groups by whitelisted columns | no |
| `mem graph delete-relations --rowids 1,2,3` | Delete explicit rowids | yes (payload preserved) |
| `mem graph merge-relations --keep R --merge R1,R2` | Fold rows into `keep`, max(confidence), merge properties (canonical wins) | yes |
| `mem graph find-duplicate-entities --strategy exact-name\|alias-chain\|vector --threshold 0.85 --entity-type Tech` | Surface candidates. Vector strategy uses cosine-sim ≥ threshold | no |
| `mem graph alias --alias A --canonical C` | Merge entity A into C: rewire all relations, fold A's properties into C, delete A + its mem_vectors | yes (since #22) |
| `mem graph augment-entity --id E --patch '{"city":"Riga"}'` | PATCH entity properties + re-embed | yes (count=0) |
| `mem graph delete-entity --id E [--force] [--purge-history]` | Direct entity delete with safety net (errors if active relations and no `--force`). `--purge-history` also drops invalidated relations | yes |
| `mem graph repair-orphan-vectors [--dry-run]` | Sweep `mem_vectors` rows whose mem_id has no matching entity/mem_item | yes |
| `mem graph prune-thin-vectors [--dry-run]` | Delete vectors whose mem_item has an empty/title-only body (below the embed threshold) — the bodyless embeddings. Never touches a real-body vector or the item itself | yes |
| `mem graph collapse-synonym-edges [--dry-run]` | One-pass: per node-pair keep ONE membership predicate (about>applies_to>belongs_to>part_of>documented_in), invalidate the rest. Distinct predicates (uses, runs_on, …) untouched. Deflates hubs without hundreds of agent round-trips | yes |
| `mem graph prune-dream-hub-edges [--dry-run]` | One-pass: invalidate every dream-diary (kind=daily) → entity backlink via a noise predicate (about/applies_to/works_on/analyzes/…). Keeps the dreams anchor. Stops diaries inflating project/topic hubs | yes |
| `mem graph repair-orphan-blobs [--dry-run]` | CAS-aware sweep of `sha256_*.txt` blob files no `mem_item.body_blob_path` references | yes |

Whitelist enums (`RelationColumn`, `EntitySimilarityStrategy`) make SQL injection a compile-time impossibility — the Dreamer cannot pass arbitrary WHERE clauses. See `src-tauri/prompts/dreaming_rem.md` Stage A.4 for how the Dreamer chains these.

### Hub report

`mem graph hubs [--threshold 20] [--limit 5] [--sample 0]` is read-only and local. It returns at most `limit` nodes whose active degree is at or above `threshold`, highest degree first, as `{threshold, limit, sample, total_hubs, hubs, excluded, existing_anchors}`. `total_hubs` counts every hub before the limit; `excluded` lists the dreams anchor `project_dreams` (`{id, degree, reason}`), which is never a hub to decompose. Each hub is `{id, degree, rel_dist, popular_source, sub_anchors, outbound_only, unclustered, clusters, neighbors}`:

- `clusters` groups the hub's inbound neighbours (the edges `introduce-topic` can re-route) by shared area, then title keyword, then kind. A group must split the remaining neighbours and must not just repeat the hub's own name; existing `topic_*`/`role_*` neighbours count as `sub_anchors` instead. Each cluster is `{suggested_topic, entity_type, existing, basis, key, members}`: `suggested_topic` reuses an existing `topic_<key>` (`existing: true`) or names `topic_<hub-name>_<key>`, and the cluster feeds `introduce-topic --topic-id <suggested_topic> --type <entity_type> --sources <members> --anchor <hub>` directly.
- `popular_source` is true when every edge is `extracted_from` (provenance, no clusters).
- `neighbors` is the raw `{id, rel, dir, title}` sample, empty unless `--sample N`.

### Deadline finalization

A dreaming worker that hits its hard deadline is finalized by the host, not the agent: the run's diary (the bound `diary_id`, else the newest `dream-diary` daily created since the run spawned) gets a deterministic insight naming the last logged stage, every still-pending log field becomes `cut_short`, a `DEADLINE:` log field is appended, and the run records finalization `partial` with reason `deadline`. A complete diary is never rewritten and a finalization the agent already recorded is kept; a later success claim over a deadline-cut diary still reconciles to `partial`.

## Auto-flush from agent hooks

The mem flush API (`POST /api/mem/flush`) is the entry point providers call from their lifecycle hooks. There is no auth-exempt loopback path — every hook carries `Authorization: Bearer $CODETERM_API_TOKEN` (CodeTerm injects the token into every PTY at spawn). Claude's standard integration installs three hook handlers. `PreCompact` memory export is a separate opt-in because it exports conversation content:

| Hook | Endpoint | Purpose |
|---|---|---|
| `Notification` | `POST /api/tabs/{id}/hook/permission` | Surface a permission prompt without waiting for PTY auto-detect |
| `Stop` | `POST /api/tabs/{id}/hook/stop` | Mark task complete, fire `agent.task_complete` webhook |
| `PreCompact` (opt-in) | `POST /api/mem/flush` | Persist conversation summary to mem before Claude compacts the context window |
| `SessionStart` | `POST /api/tabs/{id}/hook/session_start` | Track the live `session_id` so `/new` and `/clear` inside the TUI don't desync the persisted resume id |

A flush obeys the same 1 MB body cap as `mem save`: scrollback beyond it keeps its most recent tail, and the item gets the `truncated` tag with the kept and original sizes in its summary. Vector indexing embeds at most 128 chunks per item, spread across the whole body, and serves items round-robin, so a large flush never delays a small item's vectors. Deleting or replacing an item cancels its pending chunks, and a chunk still in flight is refused once its row is gone.

Hooks are installed through the provider integration action into `~/.claude/settings.json`. The action shows the target and proposed changes, records consent, makes a recoverable backup, and preserves foreign hooks.

## Related documents

Skills and background services: `codeterm docs MEMORY-SERVICES`. Agent behavior and command reference: `codeterm docs MEMORY-USAGE`.

<!-- revision: 28 -->

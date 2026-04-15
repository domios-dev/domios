# CodeTerm Memory System

<!-- revision: 2 -->

Persistent memory for agents. Survives restarts, provider switches
(Claude ↔ Codex), and CodeTerm upgrades. Agent-agnostic.

## TL;DR

- **Storage** — single SQLite DB at `~/.codeterm/mem/knowledge.db` + body blobs on disk
- **Search** — FTS5 + sqlite-vec hybrid with RRF fusion
- **Graph** — temporal entity-relation triples (people, projects; NO tech nodes)
- **Workers** — hidden headless agents that extract entities, build graph, dedup; `mem worker spawn` for ad-hoc enrichment
- **Skills** — folder skills (`SKILL.md` + scripts) registered via `mem skill new/add`, indexed alongside other items
- **Librarian** — periodic file-sync daemon: re-vectorizes source-tracked items when their on-disk file changes, flags deletions
- **Dreaming** — nightly consolidation (SQL phases + LLM REM phase in a worker)
- **Auto-flush** — `PreCompact` and `SessionStart` hooks POST to authenticated CodeTerm endpoints to capture conversation state without manual saves
- **Protocol** — 7 behavioral rules injected into every agent session via `bridge_mem.md`

## Mem vs past-session search

Two stores, two purposes — don't confuse them:

| Surface | Stored where | Asks | Used for |
|---|---|---|---|
| **mem** (`~/.codeterm/mem/knowledge.db`) | SQLite + FTS5 + sqlite-vec | "what did the agent learn / decide / save?" | Curated facts, learnings, skills, conversations the agent chose to keep |
| **sessions** (provider-owned files) | Claude JSONLs / Codex rollouts | "what did we talk about that one time?" | Raw conversation history; on-demand search via `codeterm sessions search`, atomic resume via `codeterm sessions resume` |

Mem is curated and indexed; sessions are raw and provider-owned. The two never share storage. See `docs/API.md` (`/api/sessions/*`) and `docs/plans/session-tracking.md` for the session side.

## Storage layer

### Schema (`src-tauri/src/mem/store.rs`)

**`mem_items`** — every memorable thing
- `id` — slug-derived (`fact_pgbouncer_config`, `skill_deploy_rekap`)
- `kind` — `fact` | `learning` | `note` | `conversation` | `skill` | `rule` | `preference` | `peer` | `daily` | `summary`
- `scope` — `global` | `workspace:<id>`
- `area` — project/topic tag (`rekap`, `soundcalm`, `auth`, `home`, `_dreams`)
- `tags_json` — free-form tags
- `body_inline` (<2KB) / `body_blob_path` (larger → disk blob)
- `importance` — 1-5 float, affects L1 wake-up ranking
- `access_count` — incremented on every `get()` call; used by dreaming promotion
- `accessed_at` / `created_at` / `updated_at`
- `recurrence_count` — set by dreaming when a topic recurs
- `graph_x` / `graph_y` — persistent graph layout position

**`mem_fts`** — FTS5 virtual table (title + summary + body + tags), auto-sync on insert/update.

**`mem_vectors`** (sqlite-vec) — 384-dim embeddings from fastembed (all-MiniLM-L6-v2). Local, offline, free.

**`entities`** — graph nodes (`person_*`, `project_*`). Technologies intentionally NOT entities (they create star patterns).

**`relations`** — temporal triples (`from_id`, `rel`, `to_id`, `valid_from`, `valid_to`, `confidence`). Supports point-in-time queries.

### Why SQLite over ChromaDB / Neo4j

One file, no daemon, WAL mode for concurrent reads, 281 TB max size. Bodies >2KB go to content-addressed blobs (hash-indexed) to keep the DB file small.

## Search & retrieval

Three layers (matching MemPalace's L0-L3 pattern):

| Layer | What | When | Size |
|---|---|---|---|
| **L0 — Identity** | `~/.codeterm/general/identity.md` | Injected into every GA init | ~100 tok |
| **L1 — Wake-up** | Top-N by importance + recent activity | `codeterm mem wake-up` (GA runs on init) | ~600-800 tok |
| **L2 — Targeted** | `search --area X --kind Y` | On-demand when topic comes up | variable |
| **L3 — Hybrid** | FTS5 BM25 + vector recall + RRF fusion | `codeterm mem search` / `recall` | variable |

### Hybrid ranking (`src-tauri/src/mem/search.rs`)

Ported from MemPalace's `hybrid_v3`:

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

### Wake-up priority eviction

Identity first, then (in order): rules → preferences → peers → facts → summaries. Hard token cap prevents context explosion.

## Graph layer

**Anti-star pattern:** only people and projects are entities. Technologies are searchable via FTS in body text — no graph nodes. This avoids the cascade where popular tech (React, Postgres) become mega-hubs.

**Canonical names:** dreaming worker merges name variants (Tima/Timofey → `person_tima`).

**Temporal validity:** relations have `valid_from`/`valid_to`. Dreaming's deep phase expires stale relations rather than deleting them.

**API:**
- `codeterm mem graph create --id <id> --entity-type Person --scope global` (idempotent)
- `codeterm mem graph relate --from <id> --rel mentions --to person_tima`
- `codeterm mem graph related --id <id>` — outgoing relations

## Auto-flush from agent hooks

The mem flush API (`POST /api/mem/flush`) is the entry point providers call from their lifecycle hooks. There is no auth-exempt loopback path — every hook carries `Authorization: Bearer $CODETERM_API_TOKEN` (CodeTerm injects the token into every PTY at spawn). Today Claude installs four hook handlers:

| Hook | Endpoint | Purpose |
|---|---|---|
| `Notification` | `POST /api/panes/{id}/hook/permission` | Surface a permission prompt without waiting for PTY auto-detect |
| `Stop` | `POST /api/panes/{id}/hook/stop` | Mark task complete, fire `agent.task_complete` webhook |
| `PreCompact` | `POST /api/mem/flush` | Persist conversation summary to mem before Claude compacts the context window |
| `SessionStart` | `POST /api/panes/{id}/hook/session_start` | Track the live `session_id` so `/new` and `/clear` inside the TUI don't desync the persisted resume id |

Hooks are installed by `commands.rs::install_hooks_v2` into `~/.claude/hooks.json`. The installer is idempotent and version-tagged so old loopback-auth shapes get replaced.

## Skill folders (`mem skill new/add`)

A skill is a **folder**, not a single file. `SKILL.md` is the index that mem ingests and vectorizes; the folder may also carry executable scripts, example payloads, snippets — anything an agent needs to actually do the thing the skill describes.

```
<skills-dir>/deploy-to-staging/
  SKILL.md           ← indexed in mem (kind=skill, source-tracked)
  scripts/
    deploy.sh        ← real bash, callable from SKILL.md steps
    rollback.sh
  examples/
    sample-payload.json
```

CLI:

- `codeterm mem skill new <slug> [--area X]` — scaffold a folder under the right `<skills-dir>` (dev: `~/.codeterm-dev/skills/`, prod: `~/.codeterm/skills/`), seed `SKILL.md` with frontmatter + sections, and register in mem.
- `codeterm mem skill add <folder>` — register an existing folder (cloned repo, copied from another machine).
- `codeterm mem skill list` — list every registered skill folder.

Inside `SKILL.md`, reference scripts by relative path (`./scripts/deploy.sh`). When the user invokes the skill, agents `cat SKILL.md` to recall the procedure, then execute the scripts directly. Edit any file in the folder → Librarian re-indexes on its next scan; `codeterm mem sync` triggers immediately.

## Librarian (file sync daemon)

A background loop that periodically scans every mem item carrying `source_path` (documents loaded with `--source-path`, skill `SKILL.md` files, etc.):

- File mtime > index mtime → re-read body, re-vectorize, update FTS5.
- File deleted → flag the item (kept searchable, not removed).
- New skill folder dropped into `<skills-dir>/` without registration → auto-discovered and indexed.

Manual trigger: `codeterm mem sync` (or `POST /api/mem/sync`). Code lives in `src-tauri/src/mem/librarian.rs`.

## Workers (`src-tauri/src/mem/workers.rs`)

Hidden headless agents. Live in Memory Panel, invisible in tab bar.

**`spawn_headless()`** — generic function used by:
- **Extraction workers** — auto-spawned on `mem save` for rich items (note, fact, learning, daily, conversation); extract entities, build graph
- **Dreaming REM worker** — spawned after SQL phases for dedup/classify/insights/diary

**How it works:**
1. Create tab + pane with `hidden=true`, `show_in_tabbar=false`
2. Spawn PTY from backend with env vars (`CODETERM_API_URL`, `CODETERM_API_TOKEN`, `CODETERM_PANE_ID`)
3. Start backend reader (own tokio runtime)
4. Launch agent via provider's `build_launch_command()` with `model_launch_args()` for the configured model
5. Register in `GraphWorkerRegistry` with provider/model/intent metadata
6. Worker calls `codeterm mem save`/`delete`/`graph create` — standard CLI
7. On completion, `codeterm mem worker report` marks done, full PTY log saved to `~/.codeterm/mem/workers/<id>.log`

**Limits:** max 200 workers in registry, oldest done + log files auto-evicted.

**UI:** `MemoryWorkerPanel` shows live list. Click worker → `WorkerLogPopup` with live output (2s poll for running) or saved log (for done). Status color by state (green/yellow/red).

## Dreaming

Runs nightly at 3 AM (±90 min jitter) via protected cron. Configurable agent + model in Settings.

### Phase 1 — SQL Light (`dreaming::light_phase`)

Weighted promotion (inspired by OpenClaw's scoring):
```
score = frequency×0.30 + recency×0.25 + importance×0.25 + age×0.20
```
- frequency: `access_count / 10` (capped)
- recency: 14-day half-life exponential decay from `accessed_at`
- importance: `importance / 5`
- age: `days_since_creation / 7` (capped)

Threshold: `score ≥ 0.5` AND `access_count ≥ 3` → promote learning → fact, bump importance to ≥4.

### Phase 2 — SQL Deep (`dreaming::deep_phase`)

Expire stale conversations: `kind=conversation`, `access_count=0`, `created_at > 30 days`, `importance < 4` → set `importance = 0`. Item stays searchable but drops from L1 wake-up.

### Phase 3 — REM Worker (`dreaming::spawn_rem_worker`)

Headless worker spawned after SQL phases. Task:

1. Scan all items (`codeterm mem search "" --limit 50`)
2. **Classify** — items without `area` get one via `mem update`
3. **Dedup** — items with very similar titles + same kind: read both, delete lower-importance
4. **Insights** — 2+ items on same topic → create `kind=fact title="insight: <pattern>" tags=dream-insight`
5. **Diary** — 80-150 word narrative reflection saved as `kind=daily area=_dreams tags=dream-diary`

### Data helper (`dreaming::prepare_rem_data`)

SQL precomputes optimization targets so agent doesn't re-scan everything:
- Unclassified items (missing `area`)
- Duplicate candidates (same kind + title prefix match)
- Hot items (last 7d, `access_count ≥ 2`)
- Area distribution

Available at `GET /api/mem/dream/report`.

## Behavioral protocol

Injected into every agent session via `src-tauri/prompts/bridge_mem.md`:

1. **BEFORE RESPONDING** about a person/project/past event — search first. Never guess. Wrong is worse than slow.
2. **IF UNSURE** — say "let me check" and search.
3. **FOR IDENTITY QUESTIONS** — `codeterm mem wake-up` (never guess mem_item ids like `preference_identity`).
4. **NEVER GUESS IDS** — search first, then `get` a real id.
5. **ON "remember"** — `codeterm mem save`; extraction workers handle graph automatically.
6. **WHEN FACTS CHANGE** — save new fact; old stays in history.
7. **SESSION END** — PreCompact hook auto-flushes conversation.

**Injection:**
- Claude → `install_bridge_rule` writes `~/.claude/rules/codeterm.md`
- Codex → inlined into `~/.codeterm/general/AGENTS.md` on GA start

Files regenerate on every dev/prod start.

## CLI reference

```bash
# Search
codeterm mem search "<query>" [--kind <k>] [--area <a>] [--scope workspace] [--limit <n>]
codeterm mem recall "<query>"        # semantic-first
codeterm mem get <id>                # full item body
codeterm mem wake-up                 # L0 identity + L1 critical

# Write
codeterm mem save --kind <k> --title "<t>" [--area <a>] [--tags <csv>] "<body>"
codeterm mem update <id> [--area <a>] [--title <t>] [--body <b>]
codeterm mem delete <id>
codeterm mem forget <area>           # batch delete by area

# Graph
codeterm mem graph create --id <id> --entity-type Person --scope global
codeterm mem graph relate --from <id> --rel mentions --to <id>
codeterm mem graph related --id <id>

# Workers
codeterm mem worker progress "<status>"
codeterm mem worker report "<summary>"
codeterm mem worker spawn --intent "<name>" --task "<prompt>"    # ad-hoc graph worker for custom enrichment
codeterm mem extract-pending [--limit N]                         # re-queue extraction for items that missed processing (API quota, interrupts)

# Skill folders
codeterm mem skill new <slug> [--area X]    # scaffold + register
codeterm mem skill add <folder>             # import existing
codeterm mem skill list

# File sync (Librarian)
codeterm mem sync                    # trigger re-index of source-tracked items

# Dreaming
codeterm mem dream                   # SQL phases + spawn REM worker
```

## REST API

`/api/mem/*` — parallel to CLI:
- `GET /search`, `/recall`, `/items/{id}`, `/stats`, `/wake-up`, `/graph/full`, `/dream/report`
- `POST /items`, `/dream`
- `PATCH /items/{id}`, `/positions`
- `DELETE /items/{id}`, `/forget/{area}`
- `GET /workers`, `/workers/{id}/log`

Memory Panel UI reads this directly.

## Memory Panel UI (`src/components/MemoryPanel/`)

4 tabs:

- **List** — paginated item browser (50/page, infinite scroll), filter by kind/area, click → editor modal
- **Graph** — force-directed interactive canvas, pinned-layout for incremental positions, all nodes as dots (kind → color)
- **Activity** — live mem-activity event stream (writes/updates/deletes, not searches — would loop)
- **Dreams** — next dream countdown, worker run history (with click-to-log), dream diary entries. Live updates via `graph-worker-*` events.

**Worker panel** — sidebar showing all hidden workers with status, model, click → `WorkerLogPopup`.

**Item editor** — click any item → modal with title/body/area/tags editing. `Cmd+S` saves (re-vectorizes body automatically, `DELETE + INSERT` in mem_vectors — no duplicates).

## Key design decisions

1. **Single SQLite over specialized DBs** — operational simplicity beats peak performance for personal-scale memory
2. **Blobs on disk for large bodies** — keeps the DB file small for fast opens
3. **People+projects only as entities** — prevents star patterns; tech is searchable via FTS
4. **Headless workers everywhere** — extraction and dreaming both live in Memory Panel, never pollute tab bar
5. **LLM only for REM phase** — SQL does deterministic work, LLM does pattern matching and narrative
6. **Agent-agnostic via `AgentProvider` trait** — zero hardcoded `== "claude"` checks; providers declare their own CLI flags (`model_launch_args`, `build_launch_command`)
7. **Protocol over format** — bridge_mem.md tells agents when/how to use memory, not just available commands
8. **Verbatim storage** — never summarize writes; summaries are a read-time concern

## References

- `src-tauri/src/mem/` — all memory code
- `src-tauri/src/api/mem_routes.rs` — REST API
- `src-tauri/prompts/bridge_mem.md` — behavioral protocol
- `src/components/MemoryPanel/` — UI
- `docs/plans/references/mempalace.md` — what we adopted from MemPalace
- `docs/plans/mem-architecture.md` — original design doc (historical)

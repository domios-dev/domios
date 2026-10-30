# Memory usage

Agent behavior, CLI commands, REST access, and UI.

## Behavioral protocol

Injected into every agent session:

1. **BEFORE RESPONDING** about a person/project/past event — search first. Never guess. Wrong is worse than slow.
2. **IF UNSURE** — say "let me check" and search.
3. **FOR IDENTITY QUESTIONS** — `domios mem wake-up` (never guess mem_item ids like `preference_identity`).
4. **NEVER GUESS IDS** — search first, then `get` a real id.
5. **DURABLE ONLY** — Save only durable facts. Instructions for the current session or task (model or worker choice for now, temporary priorities, 'this time', 'for this release') stay in the conversation or plan, never in memory. If durability is uncertain, do not save; ask only when the user says remember. Even an explicit remember of a session-scoped instruction requires the user to say it is permanent. On "remember" of a durable fact, save now with `domios mem save`; extraction workers handle graph automatically.
6. **WHEN FACTS CHANGE** — save new fact; old stays in history.
7. **SESSION END** — save useful facts and decisions explicitly; terminal buffers stay out of memory.

## CLI reference

```bash
# Search
domios mem search "<query>" [--kind <k>] [--area <a>] [--scope workspace] [--limit <n>]   # default limit 5
domios mem search "<query>" --supporter on|off      # per-request override of Supporter Agent memory re-rank
domios supporter status|models
domios supporter use <adapter> [--model <id>]
printf %s "$TOKEN" | domios supporter token set [--adapter <id>]   # stdin only; also token clear|status
domios mem get <id>                               # full item body
domios mem wake-up                                # L0 identity + L1 critical
domios mem wake-up                                # inherit the calling tab's workspace
domios mem wake-up --workspace <ws>               # explicitly override workspace scope
domios mem wake-up --explain                      # append per-item score breakdown
domios mem wake-up [--budget N] [--l0-only] [--no-l0]   # total L0+L1 token budget, or one layer

# Write
domios mem save --kind <k> --title "<t>" [--area <a>] [--tags <csv>] "<body>"
domios mem save --kind fact --title "X" --pin --workspace <ws> "<body>"   # atomic create + pin + scope
domios mem save ... [--importance N] [--source-path P] [--replace] [--skip-extract]
domios mem update <id> [--area <a>] [--title <t>] [--body <b>] [--summary <s>] [--tags <csv>] [--importance N] [--changed-by ACTOR]
domios mem archive <id> --reason "<why>"           # reversible; use mem restore
domios mem delete <id>
domios mem forget <area>                          # batch delete by area
domios mem history <id>                           # full audit trail of one item
domios mem area merge <from> --into <to> [--dry-run] # reversible area move; preview counts per kind
domios mem restore <id> [--version <history-row>] # undo an area merge or restore an item/body
domios mem taxonomy [--workspace <ws>]            # {area -> {kind -> count}}
domios mem tail [--follow] [--limit N]            # recent writes, newest first (default 20)
domios mem stats                                  # counts by kind + blob sizes

# Learnings (kind=learning)
domios mem learn "<text>" [--kind insight|error|feature] [--tags a,b]   # --error/--feature <text> are shortcuts
domios mem learn-list
domios mem learn-promote <id>                     # learning -> fact, raises importance

# Pins (always-loaded L1)
domios mem pin <id>                               # set is_pinned_wakeup = 1
domios mem unpin <id>                             # clear it
domios mem pinned [--limit N]                     # list pinned items (default 100, max 500)

# Graph
domios mem graph create --id <id> --entity-type Person --scope global
domios mem graph relate --from <id> --rel mentions --to <id>
domios mem graph related --id <id> [--as-of ISO]
domios mem graph timeline --entity <id>          # full temporal history
domios mem graph traverse --from <id> --hops 2 [--as-of ISO]   # BFS
domios mem graph path --from <id> --to <id>      # shortest path
domios mem graph tunnels                         # entities bridging 2+ areas

# Workers
domios mem worker progress "<status>"
domios mem worker report "<summary>" [--diary <id>] [--finalization succeeded|partial|failed] [--reason "<why>"]
domios mem worker spawn --intent "<name>" --task "<prompt>"    # ad-hoc graph worker for custom enrichment
domios mem extract-pending [--limit N]                         # re-queue extraction for items that missed processing (API quota, interrupts)

# Skill folders
domios mem skill new <slug> [--area X]    # scaffold + register
domios mem skill add <folder>             # import existing
domios mem skill list

# File sync (Librarian)
domios mem sync [--path P]         # trigger re-index of source-tracked items

# Calibration
domios mem calibration log [--since ISO] [--limit N] [--count]
domios mem calibration sweep [--force]

# Dreaming
domios mem dream                   # SQL phases + spawn REM worker
```

`domios mem search <query>` prints compact rows with 80-character snippets; `--verbose` includes scores, relevance and recall diagnostics. `domios mem search <query> --json` returns hit rows under `result`, with recall metadata under `meta` and warnings under `warnings`. It keeps hit payloads compact: an item id (or a documentation pointer), source/via, baseline score, and Supporter relevance. It omits titles, bodies, summaries, and tags. Fetch a complete memory record with `domios mem get <id> --json`.

Save identity is the exact kind and title. Saving an unchanged body after whitespace normalization reports `unchanged`, increments `recurrence_count`, and does not start extraction. A different body reports `updated`, preserves the item's id, uid, and creation time, and records the previous body and changed fields in history. A first save reports `created`. `--replace` uses the same in-place update and history behavior.

## Saving well

| Kind | Use for | Example body |
|---|---|---|
| `fact` | Verified decisions, configs, architecture choices | "PgBouncer with transaction mode, max_client_conn=400" |
| `learning` | Bug fixes, gotchas, things you figured out | "JWT kid header required by Auth0 even though RFC says optional" |
| `preference` | User preferences and style choices | "Always use vim keybindings, prefer Dracula theme" |
| `note` | Meeting notes, conversations, context | "Talked to Tima about audio engine architecture" |
| `skill` | How-to instructions, reusable procedures | "How to deploy a web service: build the image, then publish it..." |
| `rule` | Hard never/always constraints | "Never commit .env files" |
| `peer` | Machine or service info | "Build server runs Docker with the CI cache" |
| `document` | Files tracked on disk | Saved with `--source-path`, re-synced by the Librarian |

Valid `--kind` filters also include `conversation`, `script`, `daily` and `summary`. The body is chunked and embedded, so name people, projects, technologies and context: "fixed the bug" is useless. Never save a title-only or empty-body item: it gets no vector, stays invisible to semantic search and is noise; record a bare connection as a graph relation instead. Relations are free-form verb predicates (`works_on`, `uses`, `shipped`, `runs_on`); `--as-of` queries filter out stale and invalidated facts. Index a file with `cat doc.md | domios mem save --kind document --source-path doc.md --title "searchable title"`.

`mem delete` also removes the item's vectors and relations; prefer `mem archive`, which refuses pinned, skill, rule and preference items. `mem get` counts as evidence for skill, lesson, rule, fact and preference items, except from worker tabs (`CODETERM_WORKER_ID`).

## When search misses

Search fuses FTS5 keyword and vector results with RRF. Start with a short keyword (`"home automation"`), then restate as a natural phrase (`"what controls the lights at home"`), then follow graph relations with `mem graph related --id <id>`. One empty query is not proof of absence.

## Wake-up scoring

L1 loads pinned items first, each marked `★` inside its kind section, then fills the budget by `importance + log10(1 + evidence_count) * 0.5 + ws_match * 0.5 + recent_30d * 0.3`. The L0 identity row and `auto-extracted` items are excluded. When pins alone overflow the budget, output starts with `<!-- WARN: N pinned items did not fit L1 budget … -->` and the log records `[wake-up] pinned-eviction`; unpin or raise `[memory] l1_budget_tokens`.

## Sunday calibration

The Dreamer's calibration stage reviews items not updated for 14 days:

| Condition | Action | `changed_by` |
|---|---|---|
| `evidence_count >= 10`, importance < 4, accessed within 30 days | importance +0.5 | `dreamer-calibration:promote` |
| `evidence_count <= 1`, importance >= 4, created over 60 days ago | importance -0.5 | `dreamer-calibration:demote` |
| `evidence_count = 1`, importance <= 2, created over 90 days ago, kind `fact` or `note` | archive | `dreamer-calibration:archive` |

It runs only on Sunday (server-side; `--force` bypasses for testing), applies at most 30 changes, aborts above 100 candidates, and refuses when a calibration change landed in the last 6 hours. Staleness archive: `domios docs MEMORY`.

## REST API

`/api/mem/*` — parallel to CLI:
- `GET /search`, `/recall`, `/items/{id}`, `/stats`, `/wake-up`, `/graph/full`, `/dream/report`
- `GET /search?supporter=true|false` — per-request Supporter Agent memory re-rank override; `/api/supporter/*` exposes status, models, selection, feature switches and bearer-only token operations (contract in `domios docs API-MEMORY`)
- `GET /pinned?limit=N` — list pinned items (default 100, max 500)
- `GET /wake-up?workspace=<ws>&explain=1` — workspace-scoped L1 + score breakdown
- `POST /items` (`{..., pinned: true, scope: "workspace:<ws>"}` accepted; other scope values are refused)
- `POST /dream`
- `PATCH /items/{id}` (`{pinned: true|false}` toggles `is_pinned_wakeup`, audited)
- `POST /items/{id}/archive` preserves an item and history; restore it with `POST /items/{id}/restore`
- `DELETE /items/{id}`, `/forget/{area}`
- `GET /workers`, `/workers/{id}/log`

The Memory tab reads this directly.

## Memory tab

4 tabs:

- **List** — paginated item browser (50/page, infinite scroll), filter by kind/area, click → editor modal
- **Graph** — force-directed interactive canvas, pinned-layout for incremental positions, all nodes as dots (kind → color)
- **Activity** — live mem-activity event stream (writes/updates/deletes, not searches — would loop)
- **Dreams** — next dream countdown, worker run history (with click-to-log), dream diary entries. Live updates via `graph-worker-*` events.

**Workers** — sidebar showing all hidden workers with status and model; click one for its log.

**Item editor** — click any item → modal with title/body/area/tags editing. `Cmd+S` saves and re-indexes the body automatically.

Memory preserves the body you save; later edits retain history.

<!-- revision: 17 -->

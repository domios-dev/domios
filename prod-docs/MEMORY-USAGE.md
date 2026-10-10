# Memory usage

Agent behavior, CLI commands, REST access, and UI.

## Behavioral protocol

Injected into every agent session:

1. **BEFORE RESPONDING** about a person/project/past event — search first. Never guess. Wrong is worse than slow.
2. **IF UNSURE** — say "let me check" and search.
3. **FOR IDENTITY QUESTIONS** — `codeterm mem wake-up` (never guess mem_item ids like `preference_identity`).
4. **NEVER GUESS IDS** — search first, then `get` a real id.
5. **ON "remember"** — `codeterm mem save`; extraction workers handle graph automatically.
6. **WHEN FACTS CHANGE** — save new fact; old stays in history.
7. **SESSION END** — save useful facts and decisions explicitly; terminal buffers stay out of memory.

## CLI reference

```bash
# Search
codeterm mem search "<query>" [--kind <k>] [--area <a>] [--scope workspace] [--limit <n>]   # default limit 5
codeterm mem search "<query>" --supporter on|off      # per-request override of Supporter Agent memory re-rank
codeterm supporter status|models
codeterm supporter use <adapter> [--model <id>]
printf %s "$TOKEN" | codeterm supporter token set [--adapter <id>]   # stdin only; also token clear|status
codeterm mem get <id>                               # full item body
codeterm mem wake-up                                # L0 identity + L1 critical
codeterm mem wake-up                                # inherit the calling tab's workspace
codeterm mem wake-up --workspace <ws>               # explicitly override workspace scope
codeterm mem wake-up --explain                      # append per-item score breakdown
codeterm mem wake-up [--budget N] [--l0-only] [--no-l0]   # total L0+L1 token budget, or one layer

# Write
codeterm mem save --kind <k> --title "<t>" [--area <a>] [--tags <csv>] "<body>"
codeterm mem save --kind fact --title "X" --pin --workspace <ws> "<body>"   # atomic create + pin + scope
codeterm mem save ... [--importance N] [--source-path P] [--replace] [--skip-extract]
codeterm mem update <id> [--area <a>] [--title <t>] [--body <b>] [--summary <s>] [--tags <csv>] [--importance N] [--changed-by ACTOR]
codeterm mem archive <id> --reason "<why>"           # reversible; use mem restore
codeterm mem delete <id>
codeterm mem forget <area>                          # batch delete by area
codeterm mem history <id>                           # full audit trail of one item
codeterm mem area merge <from> --into <to> [--dry-run] # reversible area move; preview counts per kind
codeterm mem restore <id> [--version <history-row>] # undo an area merge or restore an item/body
codeterm mem taxonomy [--workspace <ws>]            # {area -> {kind -> count}}
codeterm mem tail [--follow] [--limit N]            # recent writes, newest first (default 20)
codeterm mem stats                                  # counts by kind + blob sizes

# Learnings (kind=learning)
codeterm mem learn "<text>" [--kind insight|error|feature] [--tags a,b]   # --error/--feature <text> are shortcuts
codeterm mem learn-list
codeterm mem learn-promote <id>                     # learning -> fact, raises importance

# Pins (always-loaded L1)
codeterm mem pin <id>                               # set is_pinned_wakeup = 1
codeterm mem unpin <id>                             # clear it
codeterm mem pinned [--limit N]                     # list pinned items (default 100, max 500)

# Graph
codeterm mem graph create --id <id> --entity-type Person --scope global
codeterm mem graph relate --from <id> --rel mentions --to <id>
codeterm mem graph related --id <id> [--as-of ISO]
codeterm mem graph timeline --entity <id>          # full temporal history
codeterm mem graph traverse --from <id> --hops 2 [--as-of ISO]   # BFS
codeterm mem graph path --from <id> --to <id>      # shortest path
codeterm mem graph tunnels                         # entities bridging 2+ areas

# Workers
codeterm mem worker progress "<status>"
codeterm mem worker report "<summary>" [--diary <id>] [--finalization succeeded|partial|failed] [--reason "<why>"]
codeterm mem worker spawn --intent "<name>" --task "<prompt>"    # ad-hoc graph worker for custom enrichment
codeterm mem extract-pending [--limit N]                         # re-queue extraction for items that missed processing (API quota, interrupts)

# Skill folders
codeterm mem skill new <slug> [--area X]    # scaffold + register
codeterm mem skill add <folder>             # import existing
codeterm mem skill list

# File sync (Librarian)
codeterm mem sync [--path P]         # trigger re-index of source-tracked items

# Calibration
codeterm mem calibration log [--since ISO] [--limit N] [--count]
codeterm mem calibration sweep [--force]

# Dreaming
codeterm mem dream                   # SQL phases + spawn REM worker
```

`codeterm mem search --json` keeps hit payloads compact: an item id (or a documentation pointer), source/via, baseline score, and Supporter relevance. It omits titles, bodies, summaries, and tags. Fetch a complete memory record with `codeterm mem get <id> --json`.

Save identity is the exact kind and title. Saving an unchanged body after whitespace normalization reports `unchanged`, increments `recurrence_count`, and does not start extraction. A different body reports `updated`, preserves the item's id, uid, and creation time, and records the previous body and changed fields in history. A first save reports `created`. `--replace` uses the same in-place update and history behavior.

## Saving well

| Kind | Use for | Example body |
|---|---|---|
| `fact` | Verified decisions, configs, architecture choices | "PgBouncer with transaction mode, max_client_conn=400" |
| `learning` | Bug fixes, gotchas, things you figured out | "JWT kid header required by Auth0 even though RFC says optional" |
| `preference` | User preferences and style choices | "Always use vim keybindings, prefer Dracula theme" |
| `note` | Meeting notes, conversations, context | "Talked to Tima about audio engine architecture" |
| `skill` | How-to instructions, reusable procedures | "How to deploy to fly.io: run ./build.sh build then..." |
| `rule` | Hard never/always constraints | "Never commit .env files" |
| `peer` | Machine or service info | "Mac Mini runs Docker with HA, Mosquitto, a gateway" |
| `document` | Files tracked on disk | Saved with `--source-path`, re-synced by the Librarian |

Valid `--kind` filters also include `conversation`, `script`, `daily` and `summary`. The body is chunked and embedded, so name people, projects, technologies and context: "fixed the bug" is useless. Never save a title-only or empty-body item: it gets no vector, stays invisible to semantic search and is noise; record a bare connection as a graph relation instead. Relations are free-form verb predicates (`works_on`, `uses`, `shipped`, `runs_on`); `--as-of` queries filter out stale and invalidated facts. Index a file with `cat doc.md | codeterm mem save --kind document --source-path doc.md --title "searchable title"`.

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

It runs only on Sunday (server-side; `--force` bypasses for testing), applies at most 30 changes, aborts above 100 candidates, and refuses when a calibration change landed in the last 6 hours. Staleness archive: `codeterm docs MEMORY`.

## REST API

`/api/mem/*` — parallel to CLI:
- `GET /search`, `/recall`, `/items/{id}`, `/stats`, `/wake-up`, `/graph/full`, `/dream/report`
- `GET /search?supporter=true|false` — per-request Supporter Agent memory re-rank override; `/api/supporter/*` exposes status, models, selection, feature switches and bearer-only token operations (contract in `codeterm docs API-MEMORY`)
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

<!-- revision: 12 -->

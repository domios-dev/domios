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
7. **SESSION END** — if the user enabled PreCompact export, the hook flushes the conversation.

## CLI reference

```bash
# Search
codeterm mem search "<query>" [--kind <k>] [--area <a>] [--scope workspace] [--limit <n>]
codeterm mem search "<query>" --supporter on|off      # per-request override of Supporter Agent memory re-rank
codeterm supporter enable|disable|status|models
codeterm supporter use <adapter> [--model <id>]
codeterm supporter features [--memory-rerank on|off] [--tab-state on|off]
printf %s "$TOKEN" | codeterm supporter token set [--adapter <id>]   # stdin only; also token clear|status
codeterm mem get <id>                               # full item body
codeterm mem wake-up                                # L0 identity + L1 critical
codeterm mem wake-up                                # inherit the calling tab's workspace
codeterm mem wake-up --workspace <ws>               # explicitly override workspace scope
codeterm mem wake-up --explain                      # append per-item score breakdown

# Write
codeterm mem save --kind <k> --title "<t>" [--area <a>] [--tags <csv>] "<body>"
codeterm mem save --kind fact --title "X" --pin --workspace <ws> "<body>"   # atomic create + pin + scope
codeterm mem update <id> [--area <a>] [--title <t>] [--body <b>]
codeterm mem archive <id> --reason "<why>"           # reversible; use mem restore
codeterm mem delete <id>
codeterm mem forget <area>                          # batch delete by area

# Pins (always-loaded L1)
codeterm mem pin <id>                               # set is_pinned_wakeup = 1
codeterm mem unpin <id>                             # clear it
codeterm mem pinned [--limit N]                     # list pinned items (default 100, max 500)

# Graph
codeterm mem graph create --id <id> --entity-type Person --scope global
codeterm mem graph relate --from <id> --rel mentions --to <id>
codeterm mem graph related --id <id>

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
codeterm mem sync                    # trigger re-index of source-tracked items

# Dreaming
codeterm mem dream                   # SQL phases + spawn REM worker
```

`codeterm mem search --json` keeps hit payloads compact: an item id (or a documentation pointer), source/via, baseline score, and Supporter relevance. It omits titles, bodies, summaries, and tags. Fetch a complete memory record with `codeterm mem get <id> --json`.

Save identity is the exact kind and title. Saving an unchanged body after whitespace normalization reports `unchanged`, increments `recurrence_count`, and does not start extraction. A different body reports `updated`, preserves the item's id, uid, and creation time, and records the previous body and changed fields in history. A first save reports `created`. `--replace` uses the same in-place update and history behavior.

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

<!-- revision: 8 -->

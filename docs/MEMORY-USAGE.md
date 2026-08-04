# Memory usage

Agent behavior, CLI commands, REST access, UI, and design decisions.

## Behavioral protocol

Injected into every agent session via `src-tauri/prompts/bridge_mem.md`:

1. **BEFORE RESPONDING** about a person/project/past event — search first. Never guess. Wrong is worse than slow.
2. **IF UNSURE** — say "let me check" and search.
3. **FOR IDENTITY QUESTIONS** — `codeterm mem wake-up` (never guess mem_item ids like `preference_identity`).
4. **NEVER GUESS IDS** — search first, then `get` a real id.
5. **ON "remember"** — `codeterm mem save`; extraction workers handle graph automatically.
6. **WHEN FACTS CHANGE** — save new fact; old stays in history.
7. **SESSION END** — if the user enabled PreCompact export, the hook flushes the conversation.

**Injection:**
- Claude → `install_bridge_rule` writes `~/.claude/rules/codeterm.md`
- Codex → inlined into `~/.codeterm/general/AGENTS.md` on GA start

Files regenerate on every dev/prod start.

## CLI reference

```bash
# Search
codeterm mem search "<query>" [--kind <k>] [--area <a>] [--scope workspace] [--limit <n>]
codeterm mem get <id>                               # full item body
codeterm mem wake-up                                # L0 identity + L1 critical
codeterm mem wake-up                                # inherit the calling tab's workspace
codeterm mem wake-up --workspace <ws>               # explicitly override workspace scope
codeterm mem wake-up --explain                      # append per-item score breakdown

# Write
codeterm mem save --kind <k> --title "<t>" [--area <a>] [--tags <csv>] "<body>"
codeterm mem save --kind fact --title "X" --pin --workspace <ws> "<body>"   # atomic create + pin + scope
codeterm mem update <id> [--area <a>] [--title <t>] [--body <b>]
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
- `GET /pinned?limit=N` — list pinned items (default 100, max 500)
- `GET /wake-up?workspace=<ws>&explain=1` — workspace-scoped L1 + score breakdown
- `POST /items` (`{..., pinned: true, scope: "workspace:<ws>"}` accepted)
- `POST /dream`
- `PATCH /items/{id}` (`{pinned: true|false}` toggles `is_pinned_wakeup`, audited)
- `DELETE /items/{id}`, `/forget/{area}`
- `GET /workers`, `/workers/{id}/log`

Memory Tabl UI reads this directly.

## Memory Tabl UI (`src/components/MemoryTabl/`)

4 tabs:

- **List** — paginated item browser (50/page, infinite scroll), filter by kind/area, click → editor modal
- **Graph** — force-directed interactive canvas, pinned-layout for incremental positions, all nodes as dots (kind → color)
- **Activity** — live mem-activity event stream (writes/updates/deletes, not searches — would loop)
- **Dreams** — next dream countdown, worker run history (with click-to-log), dream diary entries. Live updates via `graph-worker-*` events.

**Worker tabl** — sidebar showing all hidden workers with status, model, click → `WorkerLogPopup`.

**Item editor** — click any item → modal with title/body/area/tags editing. `Cmd+S` saves (re-vectorizes body automatically, `DELETE + INSERT` in mem_vectors — no duplicates).

## Key design decisions

1. **Single SQLite over specialized DBs** — operational simplicity beats peak performance for personal-scale memory
2. **Blobs on disk for large bodies** — keeps the DB file small for fast opens
3. **People+projects only as entities** — prevents star patterns; tech is searchable via FTS
4. **Headless workers everywhere** — extraction and dreaming both live in Memory Tabl, never pollute tab bar
5. **LLM only for REM phase** — SQL does deterministic work, LLM does pattern matching and narrative
6. **Agent-agnostic via `AgentProvider` trait** — zero hardcoded `== "claude"` checks; providers declare their own CLI flags (`model_launch_args`, `build_launch_command`)
7. **Protocol over format** — bridge_mem.md tells agents when/how to use memory, not just available commands
8. **Verbatim storage** — never summarize writes; summaries are a read-time concern

## References

- `src-tauri/src/mem/` — all memory code
- `src-tauri/src/api/mem_routes.rs` — REST API
- `src-tauri/prompts/bridge_mem.md` — behavioral protocol
- `src/components/MemoryTabl/` — UI

<!-- revision: 1 -->

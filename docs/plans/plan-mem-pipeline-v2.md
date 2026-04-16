# Mem Pipeline v2 — Plan

> **Status (2026-04-13):** ~70% done. Phase 1 (worker always runs on save) + Phase 2 (worker dedup + free-form entity extraction) + Dreaming (light/deep/REM phases) shipped and verified (0% orphans on orphan-bench). Phase 3 (Documents + `source_path` + Librarian cron) shipped core logic; inline Settings UI for Librarian partial. Phase 4.5 polish pending.

## Context for implementor

### Key files

| File | What it does |
|---|---|
| `src-tauri/src/mem/auto_extract.rs` | Builds extraction prompt + spawns worker |
| `src-tauri/prompts/mem_extraction.md` | Worker prompt template (rewrite in this plan) |
| `src-tauri/prompts/bridge_mem.md` | GA-facing mem instructions (simplify in this plan) |
| `src-tauri/src/api/mem_routes.rs` | REST `POST /api/mem/items` — save handler |
| `src-tauri/src/mem/store.rs` | SQLite schema, MemKind enum, insert/update |
| `src-tauri/src/mem/embeddings.rs` | Vector indexing, recall(), chunk_text() |
| `src-tauri/src/mem/graph.rs` | Entity + relation tables, insert/query |
| `src-tauri/src/agent/run_watcher.rs` | Auto-complete watcher (result event + idle + timeout) |
| `src-tauri/src/plugin/agent_claude/provider.rs` | Claude launch command builder (headless flags) |
| `src-tauri/src/service/spawn.rs` | Hidden agent spawn (wide PTY, agent_run_pty_map) |
| `src-tauri/src/config/schema.rs` | MemoryConfig fields |
| `cli/src/main.rs` | CLI `codeterm mem save/search/recall/sync` |
| `src/components/AgentRun/` | Unified popup (streamParser, EventRenderer, useAgentRunStream) |
| `src/components/MemoryPanel/useGraphWorkers.ts` | Worker list hook (safety poll) |
| `src/components/Settings/SettingsPanel.tsx` | Memory settings tab |
| `.claude/skills/dev-testing/SKILL.md` | How to test on dev daemon (:7686) via curl |

### Gotchas discovered in this session

- **`--allowedTools Bash` is REQUIRED** for claude `-p` mode to use tools. Without it, worker can't call codeterm CLI and does nothing. Already added to headless launch in `provider.rs`.
- **`message_stop` fires per assistant turn**, not per run. Watcher must only match top-level `{"type":"result"}`. Already fixed in `run_watcher.rs`.
- **Wide PTY (9999 cols)** for hidden agents — ConPTY at 120 cols wraps stream-json mid-line with cursor-positioning escapes that corrupt JSON boundaries. Already in `spawn.rs`.
- **`[Console]::OutputEncoding = UTF8`** in UNSET_CLAUDECODE prefix — needed for non-ASCII in PowerShell pipe. Already in `shell.rs`. BUT doesn't fix argv encoding from claude's own Bash tool calls (that's a deeper Win32 issue, documented in Known bugs).
- **`codeterm` CLI routes to prod by default** — does NOT read `CODETERM_API_URL`. Routes by `CODETERM_SOCKET_PATH` containing "codeterm-dev". For dev testing always use raw `curl` to `:7686` with dev token.
- **Frontend setState floods freeze Tauri** — throttle to ~120ms batches in SSE hooks. Already done in `useAgentRunStream.ts`.
- **Worker log popup 404 after restart** — `output_buf` is in-memory, wiped on restart. `agent_run_pty_map` also in-memory. Endpoints fall back to disk files (worker `.log` / cron `{job}/{run}.log`). Already done in `routes.rs`.

### Reference code

Mempalace at `references/mempalace/` — useful for:
- `mcp_server.py` — MCP tools API surface (19 tools)
- `knowledge_graph.py` — free-form relation model (subject → predicate → object)
- `entity_detector.py` — pattern-based entity detection (Person/Project signals)
- `general_extractor.py` — keyword-based memory typing (no LLM)
- `searcher.py` — ChromaDB vector search

### Dev testing

Read `.claude/skills/dev-testing/SKILL.md` for the full recipe. Short version:
```bash
TOKEN=$(grep -A1 '^\[api\]' ~/.codeterm-dev/config.toml | grep token | sed 's/.*= *"\(.*\)"/\1/')
API="http://127.0.0.1:7686"
# Fire test note
curl -s -X POST "$API/api/mem/items" -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{"kind":"note","title":"test","body":"..."}'
# Watch workers
curl -s "$API/api/mem/workers" -H "Authorization: Bearer $TOKEN"
# Read worker log
curl -s "$API/api/agent-runs/<pane_id>/log" -H "Authorization: Bearer $TOKEN"
```

---

## Architecture: 4 roles

| Role | Who | Responsibility |
|---|---|---|
| **Writer** | GA / any agent | Saves freeform. Body = content + context + intent, all mixed naturally. No schema burden. |
| **Processor** | Extraction worker (ALWAYS) | Vector dedup, entity extraction, graph relations, area classification, source_path tracking. |
| **Curator** | Dreaming worker (nightly cron) | Cross-session consolidation, stale detection, pattern recognition, learning→fact promotion, graph cleanup (invalidate stale relations, resolve conflicts). |
| **Librarian** | File sync (hourly cron) | Checks source_path items for file changes, re-indexes modified, flags missing as `source-missing`. |

GA stays dumb about mem schema. All quality responsibility is on the worker pipeline.

---

## Graph model: free-form relations (mempalace-style)

##Important:
All texts in memory should be in english only english in memories all prompts should hardly contain that thing that everything we are writing into our mem all is in english

### Current state (too narrow)

Only `person_*` and `project_*` entities. Relations limited to `mentions`, `extracted_from`, `works_on`. Prompt bans everything else.

### New model

**Entities** — any type, auto-created on first mention:
- `person_andrey`, `person_tima` (type: Person)
- `project_codeterm` (type: Project)
- `tech_docker`, `tech_postgresql` (type: Tech)
- `machine_blackstar`, `machine_macmini` (type: Machine)
- Schema is open (`entity_type: String`), worker sticks to these 4 types in 95% of cases.

**Relations** — free-form `subject → predicate → object` with temporal validity:
```
person_andrey  →  scored        →  point           (2026-04-11)
person_andrey  →  travels_to    →  china           (2026-04, planned)
person_tima    →  shipped       →  worker_pool_fix (2026-04-11)
person_tima    →  works_on      →  project_codeterm
tech_docker    →  runs_on       →  machine_blackstar
project_codeterm → uses         →  tech_postgresql
```

Both subject and object become entities automatically (INSERT OR IGNORE). Predicate is a free-form string, normalized to `lowercase_underscore`.

Our schema already supports this — `Entity.entity_type: String` and `Relation.relation_type: String` are unconstrained. The restriction was only in the extraction prompt.

### Worker rules for graph quality

Worker creates relations, but with constraints to prevent garbage:

**DO create for:**
- Concrete facts about named entities ("Tima shipped X", "Andrey loves coffee")
- Relationships between entities ("Tima works on codeterm", "Docker runs on BLACKSTAR")
- Temporal events with clear dates/periods ("trip to China in April 2026")
- Decisions tied to projects ("we decided to use PostgreSQL for codeterm")

**DO NOT create for:**
- Abstract/generic statements ("codeterm is software", "worker has bug")
- Opinions without a named subject ("this approach is better")
- Implementation details that belong in body text, not graph
- Relations where subject or object is vague ("the system", "the thing", "it")
- Duplicate of what's already in body — relation must ADD navigational value

**Predicate naming:**
- Verb-based: `shipped`, `works_on`, `uses`, `decided`, `loves`, `runs_on`, `travels_to`
- NOT noun-based: avoid `has_bug`, `is_type_of`, `belongs_to_category`
- Snake_case, lowercase, max 30 chars

**Entity ID format:**
- `<type>_<slug>`: `person_andrey`, `tech_docker`, `machine_blackstar`, `project_codeterm`
- Auto-created on first mention, idempotent
- Type inferred from context: named person → Person, repo/product → Project, tool/framework → Tech, hostname/server → Machine

### Dreaming worker graph responsibilities

In addition to existing duties (consolidate, promote, prune), Dreaming now also:
- **Invalidate stale relations** — `valid_to` set when facts are no longer true
- **Resolve conflicts** — two relations contradict each other → keep newer, invalidate older
- **Clean orphan entities** — entities with 0 relations after cleanup → delete

---

## Phase 1 — Simplified save + worker ALWAYS runs

### 1.1 GA saves freeform, one line

GA just writes everything into body. No `--area`, `--people`, `--projects` needed:

```bash
codeterm mem save --kind note --title "china trip" \
  "We're planning a trip to China with Anya, first time for both. User asked to remember this during casual chat about vacation plans."
```

GA doesn't need to know mem schema, area tables, entity id formats.

Remove `people` and `projects` fields from `SaveBody` — they were Hybrid C hints, no longer needed since worker always runs and extracts from body.

### 1.2 Worker ALWAYS runs

Remove `provided_structure → skip extraction` condition in `mem_save` handler. `spawn_extraction_worker` is called for EVERY save.

### 1.3 Save pipeline

```
mem_save handler
  │
  ├─ 1. insert item into DB (instant)
  │
  └─ 2. spawn_extraction_worker(svc, id, title, body, kind)
         │
         └─ build_extraction_prompt(svc, id, title, body)
              │
              ├─ {existing_areas} = taxonomy list (already implemented)
              ├─ {body}, {title}, {id}
              └─ → prompt string for worker
```

No inline processing. Worker does ALL the work: dedup, extraction, graph, area.

Worker has these tools via Bash:
- `codeterm mem recall "<title + body snippet>"` — vector search for similar items
- `codeterm mem search "<keywords>"` — FTS search
- `codeterm mem get <id>` — read full item body for comparison
- `codeterm mem update <id> --body "merged content"` — merge into existing
- `codeterm mem delete <id>` — remove confirmed duplicate
- `codeterm mem graph create/relate` — entity + relation management
- `codeterm mem worker report` — final report

`spawn_extraction_worker` signature stays as-is. No changes needed to the function — all dedup intelligence lives in the worker prompt.

### 1.4 Update bridge_mem.md

Remove "Inline structured save (preferred)" section. Replace with:

```
When saving: just write what you know into the body naturally. Include
who was mentioned, what was decided, why it matters — all in one text.
A background worker will handle area classification, entity extraction,
graph linking, and dedup. You don't need to worry about schema. Just save.
```

Drop area decision table from GA-facing prompt (stays in worker prompt only).

### 1.5 GA learns to save files

Add to bridge_mem.md:

```
## Saving files and documents

When the user asks to "remember this file" or "index this doc":

  cat /path/to/doc.md | codeterm mem save --kind document \
    --source-path "/path/to/doc.md" \
    --title "descriptive title for search"

--source-path tells the system where the original lives. A background
Librarian checks hourly: if the file changed on disk, content + vectors
are automatically re-indexed. If deleted, item is flagged not removed.
```

Note: `--source-path` and `kind=document` require Phase 3 implementation.

---

## Phase 2 — Worker does everything

No inline processing at save time. Worker handles dedup + extraction + graph + area.

### Worker prompt steps

1. **Dedup search** — `codeterm mem recall "<title + body snippet>" --limit 5`
   Worker sees vector-similar items with distances. For each:
   - Distance very low (< 0.15) → probable duplicate, read body via `mem get`, compare
   - If one contains the other → `mem update` to merge into the more complete, `mem delete` the lesser
   - If different angles on same topic → `mem graph relate --rel similar_to` between them
   - If clearly different → ignore, proceed

2. **Entity extraction + graph relations** — find people, projects, tech, machines in body → create entities + relations

3. **Area classification** — pick from `{existing_areas}` list or leave empty

4. **Final report** — `codeterm mem worker report "<summary>"`

### Dedup threshold

Our `embeddings::recall()` returns cosine distance (lower = more similar). Mempalace uses `1 - distance` with threshold 0.9 (= distance 0.1).

Worker uses distance < 0.15 as guideline (slightly lenient). This is in the PROMPT, not config — worker has judgment to override based on body content.

Config: `memory.dedup_threshold: f64 = 0.15` — injected into prompt as `{dedup_threshold}` so it's tunable without prompt change.

---

## Phase 3 — Documents + source_path

### 3.1 Add `kind=Document` to MemKind enum

New variant in `store.rs::MemKind`. No schema migration needed (kind is TEXT).

### 3.2 New columns in mem_items

```sql
ALTER TABLE mem_items ADD COLUMN source_path TEXT;
ALTER TABLE mem_items ADD COLUMN source_size INTEGER;
ALTER TABLE mem_items ADD COLUMN source_mtime TEXT;
```

### 3.3 CLI + REST fields

```bash
codeterm mem save --kind document --source-path /path/to/file.md "$(cat file.md)"
```

On save with `--source-path`: daemon records file size + mtime. chunk_text() + index_embedding() runs. source_path/size/mtime stored for Librarian sync.

Stdin for large files:
```bash
cat /path/to/api-ref.md | codeterm mem save --kind document \
  --source-path "/path/to/api-ref.md" --title "API Reference v2"
```

---

## Phase 4 — File sync cron (Librarian)

### 4.1 What it does

For each mem_item where `source_path IS NOT NULL`:

| File state | Action |
|---|---|
| Exists + mtime AND size unchanged | Skip |
| Exists + mtime OR size changed | Re-read → update body → re-chunk → re-embed → update size/mtime |
| File NOT found | Add tag `source-missing`, render badge in UI. Do NOT delete. |
| File reappears | Re-sync, remove `source-missing` tag |

Detection is pure `stat()` — mtime + file_size, no file reads unless something changed. No hashing. On 100 tracked files this is <1ms total.

No extraction worker on re-sync — just re-vectorize. Smart re-extraction is future scope.

### 4.2 Three trigger paths

1. **Periodic cron** — `mem-file-sync`, default hourly, Rust function (no LLM)
2. **Manual from UI** — "Sync Now" button in Settings → Periodic Tasks
3. **Agent-triggered** — `codeterm mem sync` (all) or `codeterm mem sync --path "/path/to/file.md"` (one)

### 4.3 Skills

Skills live at `~/.codeterm/skills/<id>/SKILL.md` (or `~/.codeterm-dev/skills/` for dev). They're the **single source of skills for all agent providers** (claude, codex, etc).

In mem, skills are stored as `kind=skill` items with:
- `source_path` pointing to the SKILL.md file on disk
- Body = skill content (for vector search + chunking)
- Graph = pointer (id + vector) so agents can FIND the skill, then load from disk

**Disk is source of truth.** `mem update` on a skill item doesn't change the file. Librarian syncs disk → mem, not the other way.

### 4.4 Stale items in UI

Items with `source-missing` tag:
- Badge in Memory panel item list (small warning icon or colored dot)
- Filter in Memory panel: "source-missing" as a filterable tag
- Dreaming digest: "5 items have missing source files"

---

## Phase 4.5 — Settings UI: Periodic Tasks section

Merge Dreaming + Librarian into one **Periodic Tasks** group in Settings → Memory:

```
┌─ Periodic Tasks ────────────────────────────────────────────────┐
│                                                                  │
│  ── Dreaming (Nightly Optimizer) ──────────────────────────────  │
│  ☑ Enable dreaming                         [toggle]              │
│  Agent provider: [Claude Code ▾]           [select]              │
│  Model: [Haiku ▾]                          [select]              │
│  Schedule: ~3 AM ±90 min jitter            [read-only]           │
│  Last run: 11 Apr 2026, 03:12 — Success    [read-only]           │
│  [Run Now]                                 [button]              │
│  Uses LLM — consolidates, deduplicates, promotes learnings,     │
│  cleans stale graph relations, resolves conflicts.               │
│                                                                  │
│  ── File Sync (Librarian) ─────────────────────────────────────  │
│  ☑ Enable file sync                        [toggle]              │
│  Sync interval: [60] minutes               [number, min 5]       │
│  Tracked files: 12 (3 stale)               [read-only]           │
│  Last sync: 12 Apr 2026, 14:30             [read-only]           │
│  Result: 2 re-indexed, 1 source missing    [read-only]           │
│  [Sync Now]                                [button]              │
│  No LLM — pure Rust hash/mtime comparison + re-vectorization.   │
│                                                                  │
└──────────────────────────────────────────────────────────────────┘
```

Config: `file_sync_enabled: bool = true`, `file_sync_interval_minutes: u64 = 60`

REST: `GET /api/mem/sync/status`, `POST /api/mem/sync`

---

## Phase 5 — Worker prompt engineering

Current prompt (`mem_extraction.md`) needs a full rewrite to teach the worker its new tools and workflow.

### What the worker needs to learn

**Dedup workflow:**
- Use `codeterm mem recall` (vector search, not just FTS `mem search`) as the first step
- Understand what cosine distance means: < 0.15 = very similar, investigate
- Know how to read a similar item: `codeterm mem get <id>` → compare body
- Know how to merge: `codeterm mem update <id> --body "merged content"` + `codeterm mem delete <other_id>`
- Know when to link instead of merge: `codeterm mem graph relate --from <id1> --rel similar_to --to <id2>`
- Know when to leave both as-is (different enough)

**Graph relation creation:**
- Free-form entities: Person, Project, Tech, Machine (not just person/project)
- Free-form predicates: verb-based (`shipped`, `works_on`, `uses`, `travels_to`)
- Both sides of a relation become entities automatically
- Rules for what to create vs what to skip (see Graph model section above)

**Area classification:**
- `{existing_areas}` injected into prompt — pick from list or leave empty
- Worker picks, NOT invents new areas

**Tool use discipline:**
- `--allowedTools Bash` is set — worker CAN call all tools
- Every action MUST be a Bash tool call, not a code block in text
- Final step MUST be `codeterm mem worker report`

### Prompt eval

Use `skill-creator:skill-creator` eval skill or a custom `scripts/eval_extraction_prompt.sh`:
- 5-10 test cases (short personal, long technical, duplicate of existing, Russian, decision, problem)
- Measure per case: tool_use count, code-block-as-text count, dedup correct?, area correct?, relations clean?
- Iterate prompt wording until 100% tool_use + correct dedup + clean relations on all cases
- Reference mempalace's `entity_detector.py` for entity detection signal patterns (dialogue, action verbs, direct address) to strengthen prompt hints

---

## Implementation

All steps ship together in one branch. Test end-to-end after all pieces are in place.

| Step | What |
|---|---|
| 1 | Worker ALWAYS runs (revert Hybrid C skip) |
| 2 | Remove `people`/`projects` from SaveBody |
| 3 | Free-form graph relations (remove person/project-only prompt restriction) |
| 4 | Worker prompt rewrite: dedup via `mem recall`, graph relation rules, area from taxonomy |
| 5 | Prompt eval + iteration (ensure worker uses tools correctly, dedup works, relations are clean) |
| 6 | bridge_mem.md rewrite (freeform save, file save docs) |
| 7 | `kind=Document` enum |
| 8 | `source_path/hash/mtime` columns + CLI/REST |
| 9 | File sync Rust function + cron + CLI `mem sync` |
| 10 | Settings UI: Periodic Tasks section |

---

## Important: GA prompt review before changes

Before touching bridge_mem.md or any GA-facing prompt:
- Read the FULL GA system prompt chain: `general_agent.md` + `bridge_mem.md` + identity injection + wake-up L0/L1
- Map every instruction that mentions mem save / graph / area / skills / dedup
- Identify overlaps and contradictions with the new pipeline
- Do NOT duplicate concepts — if bridge_mem says "worker handles area", general_agent.md must NOT also have area instructions
- Single source of truth for each concept: one place teaches it, others reference

The GA prompt is already long. Adding new mem pipeline instructions must REPLACE old ones, not stack on top. Every line has a token cost.

---

## What NOT to do

- Don't add MCP server now — REST + CLI is functionally equivalent for claude code
- Don't delete extraction worker — fix its behavior
- Don't build a custom embedding model — use fastembed + chunk_text()
- Don't over-engineer file watching — hourly cron + hash comparison is enough

---

## Known bugs (from this session)

### CLI `codeterm agent spawn` doesn't support `--model`

```
codeterm agent spawn claude --model sonnet --title "Worker" --task "..."
→ error: unexpected argument '--model' found
```

clap has no `--model` arg. Fix: add it, pass to `AgentSpawnSpec.model` (REST already supports it).

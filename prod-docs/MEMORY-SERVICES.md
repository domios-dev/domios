# Memory services

Skill folders, file synchronization, workers, and dreaming.

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

- `codeterm mem skill new <slug> [--area X]` — scaffold a folder under the right `<skills-dir>` (`skills/` inside the active instance data dir: `~/.codeterm/` for prod, `~/.codeterm-dev/` for dev, `~/.codeterm-dev-<name>/` for a named instance), seed `SKILL.md` with frontmatter + sections, and register in mem.
- `codeterm mem skill add <folder>` — register an existing folder (cloned repo, copied from another machine).
- `codeterm mem skill list` — list every registered skill folder.
- Removing a skill: `codeterm mem delete <id>`. The delete records a tombstone for the folder's `SKILL.md`, so the Librarian never re-registers it; the folder stays on disk. `codeterm mem skill add <folder>` clears the tombstone.

Inside `SKILL.md`, reference scripts by relative path (`./scripts/deploy.sh`). When the user invokes the skill, agents `cat SKILL.md` to recall the procedure, then execute the scripts directly. Edit any file in the folder → Librarian re-indexes on its next scan; `codeterm mem sync` triggers immediately.

## Librarian (file sync daemon)

A background loop that periodically scans every mem item carrying `source_path` (documents loaded with `--source-path`, skill `SKILL.md` files, etc.):

- File mtime > index mtime → re-read body, re-vectorize, update FTS5.
- File deleted → flag the item (kept searchable, not removed).
- New skill folder dropped into `<skills-dir>/` without registration and without a tombstone → auto-discovered and indexed during the Librarian pass (not by `codeterm mem sync`). One folder maps to one item; duplicate items for the same `SKILL.md` are merged, keeping the explicitly registered one.

Manual trigger: `codeterm mem sync` (or `POST /api/mem/sync`). Code lives in `src-tauri/src/mem/librarian.rs`.

## Workers (`src-tauri/src/mem/workers.rs`)

Hidden headless agents. Live in Memory Tabl, invisible in tab bar.

**`spawn_headless()`** — generic function used by:
- **Extraction workers** — auto-spawned on `mem save` for rich items (note, fact, learning, daily, conversation); extract entities, build graph
- **Dreaming REM worker** — spawned after SQL phases for dedup/classify/insights/diary

**How it works:**
1. Create tab + Tab with `hidden=true`, `show_in_tabbar=false`
2. Spawn PTY from backend with env vars (`CODETERM_API_URL`, `CODETERM_API_TOKEN`, `CODETERM_TAB_ID`)
3. Start backend reader (own tokio runtime)
4. Launch agent via provider's `build_launch_command()` with `model_launch_args()` for the configured model
5. Register in `GraphWorkerRegistry` with provider/model/intent metadata
6. Worker calls `codeterm mem save`/`delete`/`graph create` — standard CLI
7. On completion, `codeterm mem worker report` records that execution ended (and, with `--diary`/`--finalization`, whether the artifact actually landed); full PTY log saved to `~/.codeterm/mem/workers/<id>.log`

**Limits:** max 200 workers in registry, oldest done + log files auto-evicted.

**UI:** `MemoryWorkerTabl` shows live list. Click worker → `WorkerLogPopup` with live output (2s poll for running) or saved log (for done). Status color by state (green/yellow/red).

## Dreaming

Runs nightly at 6 AM via protected cron with a 0..30 min delay-only jitter window, so it fires between 6:00 and 6:30 AM local time. Configurable agent + model in Settings. Legacy installs that still hold the old `0 3 * * *` schedule with 0..90 min delay-only jitter are auto-migrated to the new values on startup (`ensure_dreaming_cron` in `src-tauri/src/mem/dreaming.rs`).

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

### Phase 4 — LLM Recluster (`dreaming::recluster::run_recluster_phase`) — opt-in

Hand-invoked only via `codeterm mem dream run --phase recluster`. Never part of the cron-driven nightly pipeline. The phase collects top-N hubs by active degree, their incident relation distributions, neighbor titles/summaries, and the existing `topic_*`/`role_*` anchor list, then asks the configured `dreaming_model` (default `sonnet`) to propose 0–5 cluster operations. The validated proposals are written as a markdown plan to `~/.codeterm/mem/dream-plans/<utc-ts>-recluster.md`. The user reviews the file and runs `codeterm mem dream apply <file>` to execute the `introduce_topic` mutations.

Requires `ANTHROPIC_API_KEY`. The phase is read-only against the DB; all mutations happen in the user-gated apply step.

### Data helper (`dreaming::prepare_rem_data`)

SQL precomputes optimization targets so agent doesn't re-scan everything:
- Unclassified items (missing `area`)
- Duplicate candidates (same kind + title prefix match)
- Hot items (last 7d, `access_count ≥ 2`)
- Area distribution

Available at `GET /api/mem/dream/report`.

<!-- revision: 4 -->

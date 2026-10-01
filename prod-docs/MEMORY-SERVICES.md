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

Manual trigger: `codeterm mem sync` (or `POST /api/mem/sync`).

## Workers

Extraction and REM run as hidden workers; inspect their status/logs and report execution and diary finalization separately.

- **Extraction workers** — auto-spawned on `mem save` for rich items (note, fact, learning, daily, conversation); they extract entities and build the graph.
- **Dreaming REM worker** — spawned after the SQL phases.
- On completion, `codeterm mem worker report` records that execution ended (and, with `--diary`/`--finalization`, whether the artifact actually landed); the full log is saved to `~/.codeterm/mem/workers/<id>.log`.
- **Limits:** at most 200 workers are kept; the oldest finished workers and their logs are evicted.
- The Memory tab lists workers with live or saved logs.

## Dreaming

Dreaming runs nightly between 06:00 and 06:30 local time, can promote useful learning and lower stale conversation importance without deleting it; choose its agent/model in Settings. REM classifies, consolidates through reversible merges (`codeterm docs MEMORY`, Reversible hygiene), and writes a diary.

### LLM recluster (opt-in)

Hand-invoked only via `codeterm mem dream run --phase recluster`. Never part of the cron-driven nightly pipeline. The phase collects top-N hubs by active degree, their incident relation distributions, neighbor titles/summaries, and the existing `topic_*`/`role_*` anchor list, then asks the configured `dreaming_model` (default `sonnet`) to propose 0–5 cluster operations. The validated proposals are written as a markdown plan to `~/.codeterm/mem/dream-plans/<utc-ts>-recluster.md`. The user reviews the file and runs `codeterm mem dream apply <file>` to execute the `introduce_topic` mutations.

Requires `ANTHROPIC_API_KEY`. The phase is read-only against the DB; all mutations happen in the user-gated apply step.

`GET /api/mem/dream/report` returns the prepared targets and summary for the run.

<!-- revision: 5 -->
